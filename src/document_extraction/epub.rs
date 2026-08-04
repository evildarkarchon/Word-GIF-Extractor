//! EPUB file processing module

mod resource_archive;

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use super::DocumentExtractionPolicy;
use crate::document_selection::SelectedEpub;
use crate::epub_declarations::EpubDeclarations;
use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWriteOutcome, ImageWritePipeline,
    ImageWriteRequest, ImageWriteResult, RequiredCoverWriteOutcome, RequiredCoverWriteRequest,
};

use self::resource_archive::{
    ArchiveResourceIdentity, EpubResourceArchive, EpubResourceArchiveSession, ResourceAcquisition,
    ResourceKey,
};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// Consumes one authoritative Selected EPUB and applies its Document extraction policy.
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
    policy: DocumentExtractionPolicy,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let (input_path, output_dir, base_name, retained_declarations) =
        document.into_extraction_parts();
    let acquired_declarations;
    let declarations = match retained_declarations.as_ref() {
        Some(declarations) => declarations,
        None => {
            acquired_declarations =
                EpubDeclarations::acquire(&input_path).map_err(anyhow::Error::new)?;
            &acquired_declarations
        }
    };
    // ADR-0001 keeps payload acquisition on an independent direct ZIP handle,
    // even when declaration facts were retained earlier by Document selection.
    EpubResourceArchive::open(&input_path, declarations.resources(), |mut archive| {
        let plan = archive
            .resources()
            .iter()
            .map(EpubImagePlan::from_catalog)
            .collect::<Vec<_>>();
        match policy {
            DocumentExtractionPolicy::NormalImages => extract_all_images(
                &mut archive,
                &plan,
                &HashSet::new(),
                &output_dir,
                &base_name,
                pipeline,
            ),
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images,
            } => extract_required_cover(
                &mut archive,
                &plan,
                declarations.cover_id(),
                &output_dir,
                &base_name,
                fallback_to_normal_images,
                pipeline,
            ),
        }
    })?
}

/// Session-local extraction plan built only from payload-free catalog facts.
///
/// Keys and identities retain the archive session's invariant brand, while the
/// cloned strings end the catalog borrow before keyed acquisition mutably borrows
/// the session and retain the document-facing facts needed to construct Image sources.
struct EpubImagePlan<'session> {
    key: ResourceKey<'session>,
    identity: ArchiveResourceIdentity<'session>,
    id: String,
    manifest_path: String,
    mime: String,
}

impl<'session> EpubImagePlan<'session> {
    /// Copies one branded catalog entry into the session-local extraction plan.
    fn from_catalog(resource: &resource_archive::EpubResource<'session>) -> Self {
        Self {
            key: resource.key(),
            identity: resource.identity(),
            id: resource.id().to_string(),
            manifest_path: resource.manifest_path().to_string(),
            mime: resource.mime().to_string(),
        }
    }

    /// Builds the normal-image source facts used by shared archive traversal.
    fn normal_source(&self) -> ArchiveImageSource {
        ArchiveImageSource::named(&self.manifest_path).with_mime(&self.mime)
    }

    /// Builds the required-cover source facts used by the cover Image write purpose.
    fn required_cover_source(&self) -> ArchiveImageSource {
        ArchiveImageSource::required_cover(&self.manifest_path, &self.mime)
    }
}

/// Extracts every non-excluded planned resource in deterministic resolved-path order.
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
    archive: &mut EpubResourceArchiveSession<'session>,
    plan: &[EpubImagePlan<'session>],
    excluded_identities: &HashSet<ArchiveResourceIdentity<'session>>,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    pipeline.write_from(
        ImageWriteRequest::normal_images(output_base_dir, base_name),
        |visitor| {
            for candidate in plan {
                if excluded_identities.contains(&candidate.identity) {
                    continue;
                }
                visit_resource(archive, candidate, visitor)?;
            }
            Ok(())
        },
    )
}

/// Searches for a cover image by filename when the declared cover identity fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the first matching deterministic manifest candidate.
fn find_cover_by_filename<'resource, 'session>(
    resources: &'resource [EpubImagePlan<'session>],
) -> Option<&'resource EpubImagePlan<'session>> {
    resources.iter().find(|candidate| {
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
fn extract_required_cover<'session>(
    archive: &mut EpubResourceArchiveSession<'session>,
    resources: &[EpubImagePlan<'session>],
    cover_id: Option<&str>,
    output_base_dir: &Path,
    base_name: &str,
    fallback_to_normal_images: bool,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let metadata_cover = cover_id.and_then(|id| resources.iter().find(|item| item.id == id));
    let filename_cover = find_cover_by_filename(resources);
    let mut attempted_identities = HashSet::new();
    let mut aggregate = ImageWriteResult::default();

    for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
        if !attempted_identities.insert(candidate.identity) {
            continue;
        }

        let outcome = match pipeline.write_required_cover(
            RequiredCoverWriteRequest::new(output_base_dir, base_name),
            |visitor| {
                let source = candidate.required_cover_source();
                let acquisition = archive.acquire(candidate.key, |mut payload| {
                    visitor.visit(source.clone(), &mut payload)
                })?;
                if let ResourceAcquisition::Unavailable(error) = acquisition {
                    visitor.unreadable(source, error)?;
                }
                Ok(())
            },
        ) {
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
        let fallback = match extract_all_images(
            archive,
            resources,
            &attempted_identities,
            output_base_dir,
            base_name,
            pipeline,
        ) {
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

/// Visits one keyed normal-image resource while keeping its payload borrow scoped.
///
/// # Errors
///
/// Returns an error when the pipeline cannot emit an accepted image. Resource
/// lookup, open, and read failures are recorded on the visitor and return `Ok(())`.
fn visit_resource<'session>(
    archive: &mut EpubResourceArchiveSession<'session>,
    candidate: &EpubImagePlan<'session>,
    visitor: &mut ArchiveImageVisitor<'_, '_>,
) -> Result<()> {
    let source = candidate.normal_source();
    let acquisition = archive.acquire(candidate.key, |mut payload| {
        visitor.visit(source.clone(), &mut payload)
    })?;
    if let ResourceAcquisition::Unavailable(error) = acquisition {
        visitor.unreadable(source, error);
    }

    Ok(())
}

#[cfg(test)]
mod tests;
