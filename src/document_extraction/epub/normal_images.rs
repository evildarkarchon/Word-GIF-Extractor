//! EPUB normal-image traversal over the EPUB resource archive's catalog.
//!
//! A sibling of EPUB cover extraction rather than part of the EPUB adapter: the
//! adapter runs it when no cover is sought, and cover fallback runs it with the
//! Archive resource identities already attempted. Keeping it out of the adapter
//! lets EPUB cover extraction reach it as a sibling, so the dependency runs from
//! the adapter to its children and never from a child back to its parent
//! (ADR-0021).

use anyhow::Result;
use std::collections::HashSet;

use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWriteOutcome, ImageWritePipeline,
};
use crate::output_placement::OutputPlacement;

use super::resource_archive::{
    ArchiveResourceIdentity, EpubPayloadReader, EpubResource, ResourceAcquisition,
};

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
pub(super) fn extract_all_images<'session>(
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

/// Builds the normal-image source facts used by shared archive traversal.
fn normal_source(resource: &EpubResource<'_>) -> ArchiveImageSource {
    ArchiveImageSource::named(resource.manifest_path()).with_mime(resource.mime())
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
