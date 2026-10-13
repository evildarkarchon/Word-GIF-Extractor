//! EPUB cover extraction: candidate ordering, identity exclusion, retry, and fallback.
//!
//! The cover decision, [`extract_required_cover`], names no archive type and no
//! Image write pipeline entry point: acquiring a payload and writing it are supplied
//! by an injected [`CoverAttempts`] adapter. The production adapter,
//! [`EpubCoverAttempts`], lives beside the decision in this module; its fallback runs
//! the sibling normal-image traversal, so cover extraction never calls its parent.
//!
//! ADR-0005 records why the identity type stays generic instead of naming the
//! branded Archive resource identity directly.

use std::collections::HashSet;
use std::hash::Hash;
use std::path::Path;

use crate::image_write_pipeline::{
    ImageWriteOutcome, ImageWritePipeline, ImageWriteResult, RequiredCoverWriteOutcome,
};
use crate::output_placement::OutputPlacement;

use super::normal_images;
use super::resource_archive::{
    ArchiveResourceIdentity, EpubPayloadReader, EpubResource, ResourceAcquisition,
};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// One declared manifest resource considered for cover use.
///
/// Several distinct candidates can share one Archive resource identity, which is
/// why `position` and `identity` are separate fields: `position` addresses the
/// caller's own catalog, while `identity` is what exclusion is decided on.
pub(super) struct CoverCandidate<I> {
    /// Index back into the caller's catalog, used to reach a candidate's payload.
    position: usize,
    /// Manifest identifier compared against the EPUB's declared cover.
    declared_id: String,
    /// Manifest path the JPEG-family filename heuristic reads.
    manifest_path: String,
    /// Payload identity shared by aliasing references to one archive entry.
    identity: I,
}

impl<I> CoverCandidate<I> {
    /// Records one declared manifest resource as a candidate for cover use.
    pub(super) fn new(
        position: usize,
        declared_id: &str,
        manifest_path: &str,
        identity: I,
    ) -> Self {
        Self {
            position,
            declared_id: declared_id.to_string(),
            manifest_path: manifest_path.to_string(),
            identity,
        }
    }

    /// Returns this candidate's index in the catalog the caller built it from.
    pub(super) fn position(&self) -> usize {
        self.position
    }
}

/// Injected access to the archive and the Image write pipeline for cover attempts.
///
/// One trait with a `&mut self` receiver rather than two closures: both operations
/// need the same mutable archive session, and two closure parameters cannot each
/// capture it mutably.
pub(super) trait CoverAttempts<I> {
    /// Acquires and writes one cover candidate, yielding its completion disposition.
    ///
    /// # Errors
    ///
    /// Returns an error when emitting the cover fails, retaining any facts the
    /// attempt produced before the failure.
    fn attempt(
        &mut self,
        candidate: &CoverCandidate<I>,
    ) -> ImageWriteOutcome<RequiredCoverWriteOutcome>;

    /// Extracts normal images, skipping every already-attempted payload identity.
    ///
    /// # Errors
    ///
    /// Returns an error when normal-image emission fails, retaining the facts
    /// produced before the failure.
    fn fallback(&mut self, excluded_identities: &HashSet<I>) -> ImageWriteOutcome;
}

/// Copies the catalog into the cover candidates EPUB cover extraction orders.
///
/// The catalog index becomes the candidate position, which is how [`EpubCoverAttempts`]
/// gets back to a catalog entry in constant time. Manifest-path lookup was rejected:
/// it is a linear scan, and aliasing spellings make it non-unique.
pub(super) fn cover_candidates<'session>(
    catalog: &[EpubResource<'session>],
) -> Vec<CoverCandidate<ArchiveResourceIdentity<'session>>> {
    catalog
        .iter()
        .enumerate()
        .map(|(position, resource)| {
            CoverCandidate::new(
                position,
                resource.id(),
                resource.manifest_path(),
                resource.identity(),
            )
        })
        .collect()
}

/// Production adapter giving EPUB cover extraction one archive session and pipeline.
///
/// It lives beside the cover decision because its fallback operation runs the sibling
/// normal-image traversal rather than the EPUB adapter's own code; owning it in the
/// EPUB adapter would route cover extraction back through its parent, the mutual
/// dependency ADR-0004 removed elsewhere in this crate.
pub(super) struct EpubCoverAttempts<'attempts, 'session> {
    pub(super) catalog: &'attempts [EpubResource<'session>],
    pub(super) payloads: &'attempts mut EpubPayloadReader<'session>,
    pub(super) placement: &'attempts OutputPlacement,
    pub(super) pipeline: &'attempts ImageWritePipeline,
}

impl<'session> CoverAttempts<ArchiveResourceIdentity<'session>>
    for EpubCoverAttempts<'_, 'session>
{
    /// Acquires one cover candidate's catalog entry and writes it through the cover purpose.
    ///
    /// # Panics
    ///
    /// Panics when the candidate was not built by [`cover_candidates`] from this
    /// adapter's catalog, since its position indexes that catalog directly.
    fn attempt(
        &mut self,
        candidate: &CoverCandidate<ArchiveResourceIdentity<'session>>,
    ) -> ImageWriteOutcome<RequiredCoverWriteOutcome> {
        // Destructured so the acquisition closure's catalog and pipeline borrows stay
        // disjoint from the mutable payload reader borrow `acquire` takes around it.
        let Self {
            catalog,
            payloads,
            placement,
            pipeline,
        } = self;
        let resource = &catalog[candidate.position()];

        // The consumer never fails: the pipeline's whole outcome, including an
        // emission failure and the facts it retains, travels back as the acquired
        // value. `acquire` can therefore only return `Ok` here, and `?` exists to
        // satisfy its signature rather than to handle a reachable error.
        let acquisition = payloads.acquire(resource, |payload| {
            Ok(pipeline.write_required_cover(
                placement,
                resource.manifest_path(),
                resource.mime(),
                payload,
            ))
        })?;
        match acquisition {
            ResourceAcquisition::Acquired(outcome) => outcome,
            ResourceAcquisition::Unavailable(error) => {
                Ok(pipeline.required_cover_unavailable(resource.manifest_path(), error))
            }
        }
    }

    /// Runs normal-image traversal over the same catalog, skipping attempted payloads.
    fn fallback(
        &mut self,
        excluded_identities: &HashSet<ArchiveResourceIdentity<'session>>,
    ) -> ImageWriteOutcome {
        normal_images::extract_all_images(
            self.catalog,
            self.payloads,
            excluded_identities,
            self.placement,
            self.pipeline,
        )
    }
}

/// Extracts one required EPUB cover and optionally falls back to normal images.
///
/// The declared metadata cover precedes the first deterministic JPEG-family
/// filename candidate. Archive resource identities are recorded before attempts,
/// suppress aliases, and exclude every attempted payload from normal fallback.
/// Only retry dispositions advance; every completed Image write decision is terminal.
///
/// # Errors
///
/// Returns an error when cover or fallback emission fails. Facts from earlier cover
/// attempts and successful normal-image writes remain attached to the failure.
pub(super) fn extract_required_cover<I, A>(
    candidates: &[CoverCandidate<I>],
    declared_cover_id: Option<&str>,
    fallback_to_normal_images: bool,
    attempts: &mut A,
) -> ImageWriteOutcome
where
    I: Copy + Eq + Hash,
    A: CoverAttempts<I>,
{
    let metadata_cover = declared_cover(candidates, declared_cover_id);
    let filename_cover = find_cover_by_filename(candidates);
    let mut attempted_identities = HashSet::new();
    let mut aggregate = ImageWriteResult::default();

    for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
        if !attempted_identities.insert(candidate.identity) {
            continue;
        }

        let outcome = match attempts.attempt(candidate) {
            Ok(outcome) => outcome,
            Err(mut failure) => {
                failure.prepend(aggregate);
                return Err(failure);
            }
        };

        match outcome {
            RequiredCoverWriteOutcome::Retry(result) => aggregate.append(result),
            RequiredCoverWriteOutcome::Completed(result) => {
                aggregate.append(result);
                return Ok(aggregate);
            }
        }
    }

    if fallback_to_normal_images {
        let fallback = match attempts.fallback(&attempted_identities) {
            Ok(fallback) => fallback,
            Err(mut failure) => {
                failure.prepend(aggregate);
                return Err(failure);
            }
        };
        aggregate.append(fallback);
    }

    Ok(aggregate)
}

/// Finds the candidate whose manifest identifier is the EPUB's declared cover.
fn declared_cover<'candidate, I>(
    candidates: &'candidate [CoverCandidate<I>],
    declared_cover_id: Option<&str>,
) -> Option<&'candidate CoverCandidate<I>> {
    declared_cover_id.and_then(|id| {
        candidates
            .iter()
            .find(|candidate| candidate.declared_id == id)
    })
}

/// Searches for a cover image by filename when the declared cover identity fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the first matching deterministic manifest candidate.
fn find_cover_by_filename<I>(candidates: &[CoverCandidate<I>]) -> Option<&CoverCandidate<I>> {
    candidates.iter().find(|candidate| {
        let path = Path::new(&candidate.manifest_path);
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());

        file_stem.as_deref() == Some("cover")
            && extension
                .as_deref()
                .is_some_and(|ext| JPEG_EXTENSIONS.contains(&ext))
    })
}

#[cfg(test)]
mod tests;
