//! EPUB file processing module

mod cover_extraction;
mod resource_archive;

use anyhow::Result;
use std::collections::HashSet;

use super::EpubCoverPolicy;
use crate::document_selection::SelectedEpub;
use crate::epub_declarations::{EpubDeclarations, EpubFileDeclarations};
use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWriteOutcome, ImageWritePipeline,
    RequiredCoverWriteOutcome,
};
use crate::output_placement::OutputPlacement;

use self::cover_extraction::{CoverAttempts, CoverCandidate};
use self::resource_archive::{
    ArchiveResourceIdentity, EpubPayloadReader, EpubResource, EpubResourceArchive,
    ResourceAcquisition,
};

/// Consumes one authoritative Selected EPUB and applies any EPUB cover policy.
///
/// No cover policy means normal images, which is why this is the only document
/// kind that receives the value at all.
///
/// Retained declarations remain authoritative; when selection retained none, extraction
/// retries declaration acquisition without revising the selected output placement or base
/// name. Resource payloads are read through an independently opened archive with scoped
/// readers, and any partial Image write facts are preserved by the returned outcome.
///
/// # Errors
///
/// Returns an error when EPUB declarations cannot be acquired, the resource archive cannot
/// be opened, or collision-safe output emission cannot create or complete a file.
pub(super) fn extract(
    document: SelectedEpub,
    cover_policy: Option<EpubCoverPolicy>,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let (input_path, placement, retained_declarations) = document.into_extraction_parts();
    // ADR-0008 leaves this reacquisition on the real file reader: extraction opens
    // the archive for payloads under ADR-0001 regardless, so a substitute buys nothing.
    let declarations = EpubDeclarations::retained_or_acquire(
        retained_declarations,
        &input_path,
        &EpubFileDeclarations,
    )
    .map_err(anyhow::Error::new)?;
    // ADR-0001 keeps payload acquisition on an independent direct ZIP handle,
    // even when declaration facts were retained earlier by Document selection.
    EpubResourceArchive::open(
        &input_path,
        declarations.resources(),
        |catalog, mut payloads| {
            match cover_policy {
                None => extract_all_images(
                    catalog,
                    &mut payloads,
                    &HashSet::new(),
                    &placement,
                    pipeline,
                ),
                Some(cover_policy) => {
                    // ADR-0005 keeps cover extraction taking a plain bool rather than a
                    // Document extraction type, so the fallback decision is flattened here.
                    let fallback_to_normal_images =
                        matches!(cover_policy, EpubCoverPolicy::CoverThenNormalImages);
                    let candidates = cover_candidates(catalog);
                    let mut attempts = EpubCoverAttempts {
                        catalog,
                        payloads: &mut payloads,
                        placement: &placement,
                        pipeline,
                    };
                    cover_extraction::extract_required_cover(
                        &candidates,
                        declarations.cover_id(),
                        fallback_to_normal_images,
                        &mut attempts,
                    )
                }
            }
        },
    )?
}

/// Builds the normal-image source facts used by shared archive traversal.
fn normal_source(resource: &EpubResource<'_>) -> ArchiveImageSource {
    ArchiveImageSource::named(resource.manifest_path()).with_mime(resource.mime())
}

/// Extracts every non-excluded catalog resource in deterministic resolved-path order.
///
/// Weak labels are intentional inputs: byte-first discovery can recover images
/// from resources whose extension and MIME type do not identify them. Cover
/// fallback passes attempted Archive resource identities so aliases are not revisited.
///
/// # Errors
///
/// Returns an error only when output emission fails; per-resource lookup and read
/// failures become warning facts and traversal continues.
fn extract_all_images<'session>(
    catalog: &[EpubResource<'session>],
    payloads: &mut EpubPayloadReader<'session>,
    excluded_identities: &HashSet<ArchiveResourceIdentity<'session>>,
    placement: &OutputPlacement,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    pipeline.write_from(placement, |visitor| {
        for resource in catalog {
            if excluded_identities.contains(&resource.identity()) {
                continue;
            }
            visit_resource(payloads, resource, visitor)?;
        }
        Ok(())
    })
}

/// Copies the catalog into the cover candidates EPUB cover extraction orders.
///
/// The catalog index becomes the candidate position, which is how [`EpubCoverAttempts`]
/// gets back to a catalog entry in constant time. Manifest-path lookup was rejected:
/// it is a linear scan, and aliasing spellings make it non-unique.
fn cover_candidates<'session>(
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
/// It lives here rather than in the child module because the fallback operation runs
/// this file's normal-image traversal; owning it there would make the child call its
/// parent, which is the mutual dependency ADR-0004 removed elsewhere in this crate.
struct EpubCoverAttempts<'attempts, 'session> {
    catalog: &'attempts [EpubResource<'session>],
    payloads: &'attempts mut EpubPayloadReader<'session>,
    placement: &'attempts OutputPlacement,
    pipeline: &'attempts ImageWritePipeline,
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
        extract_all_images(
            self.catalog,
            self.payloads,
            excluded_identities,
            self.placement,
            self.pipeline,
        )
    }
}

/// Visits one normal-image catalog resource while keeping its payload borrow scoped.
///
/// # Errors
///
/// Returns an error when the pipeline cannot emit an accepted image. Resource
/// lookup, open, and read failures are recorded on the visitor and return `Ok(())`.
fn visit_resource<'session>(
    payloads: &mut EpubPayloadReader<'session>,
    resource: &EpubResource<'session>,
    visitor: &mut ArchiveImageVisitor<'_, '_>,
) -> Result<()> {
    let source = normal_source(resource);
    let acquisition =
        payloads.acquire(resource, |payload| visitor.visit(source.clone(), payload))?;
    if let ResourceAcquisition::Unavailable(error) = acquisition {
        visitor.unreadable(source, error);
    }

    Ok(())
}

#[cfg(test)]
mod tests;
