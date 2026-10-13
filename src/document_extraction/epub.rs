//! EPUB file processing module

mod cover_extraction;
mod normal_images;
mod resource_archive;

use std::collections::HashSet;

use super::EpubCoverPolicy;
use crate::document_selection::SelectedEpub;
use crate::epub_declarations::{EpubDeclarations, EpubFileDeclarations};
use crate::image_write_pipeline::{ImageWriteOutcome, ImageWritePipeline};

use self::resource_archive::EpubResourceArchive;

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
                None => normal_images::extract_all_images(
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
                    let candidates = cover_extraction::cover_candidates(catalog);
                    let mut attempts = cover_extraction::EpubCoverAttempts {
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

#[cfg(test)]
mod tests;
