//! EPUB cover candidate ordering, acquisition retry, and optional batch fallback.

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use zip::ZipArchive;

use crate::image_write_pipeline::{
    ArchiveImageSource, ImageWritePipeline, ImageWriteResult, RequiredCoverWriteOutcome,
    RequiredCoverWriteRequest,
};

use super::{EpubResourceCandidate, append_result, candidate_resource_index, extract_all_images};

/// EPUB facts needed to resolve and write one required cover.
pub(super) struct EpubCoverRequest<'request> {
    pub(super) resources: &'request [EpubResourceCandidate],
    pub(super) metadata_cover: Option<&'request EpubResourceCandidate>,
    pub(super) filename_cover: Option<&'request EpubResourceCandidate>,
    pub(super) cover_fallback: bool,
    pub(super) output_dir: &'request Path,
    pub(super) base_name: &'request str,
    pub(super) pipeline: &'request ImageWritePipeline,
}

/// Applies ordered EPUB cover acquisition and optional normal-image fallback.
///
/// Resolved archive identity suppresses aliases across cover attempts and fallback.
/// Only a retry disposition advances to another candidate. Image file emission
/// failures remain fatal and abort the document.
pub(super) fn extract_required_cover(
    archive: &mut ZipArchive<fs::File>,
    request: EpubCoverRequest<'_>,
) -> Result<ImageWriteResult> {
    let EpubCoverRequest {
        resources,
        metadata_cover,
        filename_cover,
        cover_fallback,
        output_dir,
        base_name,
        pipeline,
    } = request;
    let mut attempted_identities = HashSet::new();
    let mut aggregate = ImageWriteResult::default();

    for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
        if !attempted_identities.insert(candidate.archive_identity.clone()) {
            continue;
        }

        let outcome = pipeline.write_required_cover(
            RequiredCoverWriteRequest::new(output_dir, base_name),
            |visitor| {
                let source = ArchiveImageSource::required_cover(
                    candidate.source_name.clone(),
                    candidate.mime.clone(),
                );
                let index = match candidate_resource_index(candidate) {
                    Ok(index) => index,
                    Err(error) => return visitor.unreadable(source, error),
                };

                match archive.by_index(index) {
                    Ok(mut entry) => visitor.visit(source, &mut entry),
                    Err(error) => visitor.unreadable(source, error),
                }
            },
        )?;

        match outcome {
            RequiredCoverWriteOutcome::Retry(result) => append_result(&mut aggregate, result),
            RequiredCoverWriteOutcome::Completed(result) => {
                append_result(&mut aggregate, result);
                return Ok(aggregate);
            }
        }
    }

    if cover_fallback {
        let fallback = extract_all_images(
            archive,
            resources,
            &attempted_identities,
            output_dir,
            base_name,
            pipeline,
        )?;
        append_result(&mut aggregate, fallback);
    }

    Ok(aggregate)
}
