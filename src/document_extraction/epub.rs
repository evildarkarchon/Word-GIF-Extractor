//! EPUB file processing module

mod cover_extraction;
mod resource_archive;

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use super::EpubCoverPolicy;
use crate::document_selection::SelectedEpub;
use crate::epub_declarations::EpubDeclarations;
use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWriteOutcome, ImageWritePipeline,
    ImageWriteRequest, RequiredCoverWriteOutcome, RequiredCoverWriteRequest,
};

use self::cover_extraction::{CoverAttempts, CoverCandidate};
use self::resource_archive::{
    ArchiveResourceIdentity, EpubResourceArchive, EpubResourceArchiveSession, ResourceAcquisition,
    ResourceKey,
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
    let (target, retained_declarations) = document.into_extraction_inputs();
    let input_path = target.get_source();
    let output_dir = target.get_placement().get_dir();
    let base_name = target.get_placement().get_base_name();
    let acquired_declarations;
    let declarations = match retained_declarations.as_ref() {
        Some(declarations) => declarations,
        None => {
            acquired_declarations =
                EpubDeclarations::acquire(input_path).map_err(anyhow::Error::new)?;
            &acquired_declarations
        }
    };
    // ADR-0001 keeps payload acquisition on an independent direct ZIP handle,
    // even when declaration facts were retained earlier by Document selection.
    EpubResourceArchive::open(input_path, declarations.resources(), |mut archive| {
        let plan = archive
            .resources()
            .iter()
            .map(EpubImagePlan::from_catalog)
            .collect::<Vec<_>>();
        match cover_policy {
            None => extract_all_images(
                &mut archive,
                &plan,
                &HashSet::new(),
                output_dir,
                base_name,
                pipeline,
            ),
            Some(cover_policy) => {
                // ADR-0005 keeps cover extraction taking a plain bool rather than a
                // Document extraction type, so the fallback decision is flattened here.
                let fallback_to_normal_images =
                    matches!(cover_policy, EpubCoverPolicy::CoverThenNormalImages);
                let candidates = cover_candidates(&plan);
                let mut attempts = EpubCoverAttempts {
                    archive: &mut archive,
                    plan: &plan,
                    output_base_dir: output_dir,
                    base_name,
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

/// Copies the session-local plan into the cover candidates EPUB cover extraction orders.
///
/// The plan index becomes the candidate position, which is how [`EpubCoverAttempts`]
/// gets back to a resource key in constant time. Manifest-path lookup was rejected:
/// it is a linear scan, and aliasing spellings make it non-unique.
fn cover_candidates<'session>(
    plan: &[EpubImagePlan<'session>],
) -> Vec<CoverCandidate<ArchiveResourceIdentity<'session>>> {
    plan.iter()
        .enumerate()
        .map(|(position, resource)| {
            CoverCandidate::new(
                position,
                &resource.id,
                &resource.manifest_path,
                resource.identity,
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
    archive: &'attempts mut EpubResourceArchiveSession<'session>,
    plan: &'attempts [EpubImagePlan<'session>],
    output_base_dir: &'attempts Path,
    base_name: &'attempts str,
    pipeline: &'attempts ImageWritePipeline,
}

impl<'session> CoverAttempts<ArchiveResourceIdentity<'session>>
    for EpubCoverAttempts<'_, 'session>
{
    /// Acquires one planned cover candidate and writes it through the cover purpose.
    ///
    /// # Panics
    ///
    /// Panics when the candidate was not built by [`cover_candidates`] from this
    /// adapter's plan, since its position addresses that plan directly.
    fn attempt(
        &mut self,
        candidate: &CoverCandidate<ArchiveResourceIdentity<'session>>,
    ) -> ImageWriteOutcome<RequiredCoverWriteOutcome> {
        // Destructured so the traversal closure's mutable archive borrow stays
        // disjoint from the plan and pipeline borrows taken around it.
        let Self {
            archive,
            plan,
            output_base_dir,
            base_name,
            pipeline,
        } = self;
        let resource = &plan[candidate.position()];

        pipeline.write_required_cover(
            RequiredCoverWriteRequest::new(output_base_dir, base_name),
            |visitor| {
                let source = resource.required_cover_source();
                let acquisition = archive.acquire(resource.key, |mut payload| {
                    visitor.visit(source.clone(), &mut payload)
                })?;
                if let ResourceAcquisition::Unavailable(error) = acquisition {
                    visitor.unreadable(source, error)?;
                }
                Ok(())
            },
        )
    }

    /// Runs normal-image traversal over the same plan, skipping attempted payloads.
    fn fallback(
        &mut self,
        excluded_identities: &HashSet<ArchiveResourceIdentity<'session>>,
    ) -> ImageWriteOutcome {
        extract_all_images(
            self.archive,
            self.plan,
            excluded_identities,
            self.output_base_dir,
            self.base_name,
            self.pipeline,
        )
    }
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
