//! EPUB cover extraction: candidate ordering, identity exclusion, retry, and fallback.
//!
//! One concrete function, [`extract_required_cover`], over the real EPUB resource
//! archive catalog and payload reader and the real Image write pipeline. It decides
//! which cover candidate to attempt and when to stop; the pipeline's scoped cover
//! entry folds every fact the sequence produces. Fallback runs the sibling
//! normal-image traversal, so cover extraction never calls its parent.
//!
//! ADR-0021 records why this is no longer generic over the Archive resource identity
//! behind an injected attempt seam, as ADR-0005 had it.

use std::collections::HashSet;
use std::path::Path;

use crate::document_extraction::EpubCoverPolicy;
use crate::image_write_pipeline::{
    ImageWriteOutcome, ImageWritePipeline, RequiredCoverDisposition,
};
use crate::output_placement::OutputPlacement;

use super::normal_images;
use super::resource_archive::{EpubPayloadReader, EpubResource, ResourceAcquisition};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// Extracts one required EPUB cover and optionally falls back to normal images.
///
/// The declared metadata cover precedes the first deterministic JPEG-family
/// filename candidate. Archive resource identities are recorded before attempts,
/// suppress aliases, and exclude every attempted payload from normal fallback.
/// Only retry dispositions advance; every completed Image write decision is terminal.
/// Fallback runs only when `cover_policy` asks for normal images after a cover.
///
/// # Errors
///
/// Returns an error when cover or fallback emission fails. Facts from earlier cover
/// attempts and successful normal-image writes remain attached to the failure.
pub(super) fn extract_required_cover<'session>(
    catalog: &[EpubResource<'session>],
    payloads: &mut EpubPayloadReader<'session>,
    pipeline: &ImageWritePipeline,
    placement: &OutputPlacement,
    declared_cover_id: Option<&str>,
    cover_policy: EpubCoverPolicy,
) -> ImageWriteOutcome {
    let metadata_cover = declared_cover(catalog, declared_cover_id);
    let filename_cover = find_cover_by_filename(catalog);
    let mut attempted_identities = HashSet::new();

    pipeline.write_required_cover(placement, |cover_scope| {
        for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
            if !attempted_identities.insert(candidate.identity()) {
                continue;
            }

            // An emission failure inside the attempt comes back through `acquire`
            // unchanged and ends the sequence; the attempt has already folded the
            // facts it recorded before failing into the cover scope.
            let acquisition = payloads.acquire(candidate, |payload| {
                cover_scope.attempt(candidate.manifest_path(), candidate.mime(), payload)
            })?;
            let disposition = match acquisition {
                ResourceAcquisition::Acquired(disposition) => disposition,
                ResourceAcquisition::Unavailable(error) => {
                    cover_scope.unreadable(candidate.manifest_path(), error)
                }
            };
            if disposition == RequiredCoverDisposition::Completed {
                return Ok(());
            }
        }

        match cover_policy {
            EpubCoverPolicy::CoverOnly => Ok(()),
            EpubCoverPolicy::CoverThenNormalImages => cover_scope.fallback(|visitor| {
                normal_images::visit_all(catalog, payloads, &attempted_identities, visitor)
            }),
        }
    })
}

/// Finds the catalog entry whose manifest identifier is the EPUB's declared cover.
fn declared_cover<'catalog, 'session>(
    catalog: &'catalog [EpubResource<'session>],
    declared_cover_id: Option<&str>,
) -> Option<&'catalog EpubResource<'session>> {
    declared_cover_id.and_then(|id| catalog.iter().find(|resource| resource.id() == id))
}

/// Searches for a cover image by filename when the declared cover identity fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the first matching deterministic manifest candidate.
fn find_cover_by_filename<'catalog, 'session>(
    catalog: &'catalog [EpubResource<'session>],
) -> Option<&'catalog EpubResource<'session>> {
    catalog.iter().find(|resource| {
        let path = Path::new(resource.manifest_path());
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
