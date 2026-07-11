//! Required EPUB cover decision policy.

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::conversion::ConversionOutcome;
use crate::image_format::{ImageFormat, ImageFormatSource};
use crate::image_write_pipeline::{ImageWritePipeline, ImageWriteResult, ImageWriteWarning};

use super::{EpubArchiveIdentity, EpubResourceCandidate, append_result};

/// Bounded, non-owning facts used to decide whether a cover payload is needed.
pub(super) struct CoverEvidence<'evidence> {
    pub(super) data: &'evidence [u8],
    pub(super) mime: &'evidence str,
}

/// Cover policy's typed response to bounded archive evidence.
pub(super) enum CoverEvidenceDecision {
    /// The candidate has a final non-emitting outcome; no remaining bytes are needed.
    Completed(ImageWriteResult),
    /// The candidate is accepted and its remaining payload must be acquired.
    Acquire {
        format: ImageFormat,
        warnings: Vec<ImageWriteWarning>,
    },
}

/// Decision callback used by acquisition without exposing archive readers to policy.
pub(super) trait CoverEvidencePolicy {
    /// Returns whether bounded evidence completes the outcome or requires the payload.
    fn decide(&mut self, evidence: CoverEvidence<'_>) -> CoverEvidenceDecision;
}

/// Typed facts for one fully acquired cover image.
pub(super) struct AcquiredCover {
    data: Vec<u8>,
    format: ImageFormat,
    warnings: Vec<ImageWriteWarning>,
}

/// Typed result returned by the direct ZIP acquisition adapter.
pub(super) enum CoverAcquisitionOutcome {
    /// Archive lookup or incremental reading failed, so another candidate may be tried.
    AcquisitionFailed(ImageWriteResult),
    /// Bounded evidence produced a final non-emitting cover outcome.
    Completed(ImageWriteResult),
    /// The accepted cover payload was acquired completely.
    Acquired(AcquiredCover),
}

impl CoverAcquisitionOutcome {
    /// Creates the only cover acquisition outcome that permits candidate fallback.
    pub(super) fn acquisition_failed(
        source_name: impl Into<String>,
        error: impl std::fmt::Display,
    ) -> Self {
        Self::AcquisitionFailed(ImageWriteResult::from_warning(
            ImageWriteWarning::archive_image_acquisition_failed(source_name, error),
        ))
    }

    /// Creates a failed acquisition while preserving earlier evidence warnings.
    pub(super) fn acquisition_failed_after_evidence(
        mut warnings: Vec<ImageWriteWarning>,
        source_name: impl Into<String>,
        error: impl std::fmt::Display,
    ) -> Self {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            source_name,
            error,
        ));
        Self::AcquisitionFailed(ImageWriteResult::from_warnings(warnings))
    }

    /// Creates typed acquired-cover facts after the adapter completes the payload.
    pub(super) fn acquired(
        data: Vec<u8>,
        format: ImageFormat,
        warnings: Vec<ImageWriteWarning>,
    ) -> Self {
        Self::Acquired(AcquiredCover {
            data,
            format,
            warnings,
        })
    }
}

/// Image write request facts used after a cover payload is accepted.
pub(super) struct CoverWriteRequest<'request> {
    pub(super) output_dir: &'request Path,
    pub(super) base_name: &'request str,
    pub(super) pipeline: &'request ImageWritePipeline,
}

/// Direct-resource operations needed by EPUB cover extraction policy.
pub(super) trait CoverResourceAdapter {
    /// Acquires one candidate through a scoped archive reader, asking cover policy
    /// whether bounded evidence requires the remaining payload.
    ///
    /// Returns typed acquisition or evidence-decision facts. Archive lookup and
    /// read failures are non-fatal outcomes; unexpected adapter errors are returned.
    fn acquire_cover(
        &mut self,
        candidate: &EpubResourceCandidate,
        policy: &mut dyn CoverEvidencePolicy,
    ) -> Result<CoverAcquisitionOutcome>;

    /// Runs normal-image fallback while excluding already attempted archive identities.
    ///
    /// Returns the complete fallback Image write outcome. Traversal and Image file
    /// emission failures are returned and abort the document.
    fn extract_normal_images(
        &mut self,
        excluded_identities: &HashSet<EpubArchiveIdentity>,
    ) -> Result<ImageWriteResult>;
}

/// Applies the ordered required-cover policy and optional normal-image fallback.
///
/// Resolved archive identity suppresses aliases across both cover attempts and
/// normal-image fallback. Only `AcquisitionFailed` advances to another candidate.
/// Errors, including Image file emission failures, abort the document immediately.
pub(super) fn extract_required_cover<'candidate>(
    metadata_cover: Option<&'candidate EpubResourceCandidate>,
    filename_cover: Option<&'candidate EpubResourceCandidate>,
    cover_fallback: bool,
    write_request: CoverWriteRequest<'_>,
    resource_adapter: &mut impl CoverResourceAdapter,
) -> Result<ImageWriteResult> {
    let mut attempted_identities = HashSet::new();
    let mut aggregate = ImageWriteResult::default();

    for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
        if !attempted_identities.insert(candidate.archive_identity.clone()) {
            continue;
        }

        let mut evidence_policy = RequiredCoverEvidencePolicy {
            pipeline: write_request.pipeline,
        };
        match resource_adapter.acquire_cover(candidate, &mut evidence_policy)? {
            CoverAcquisitionOutcome::AcquisitionFailed(result) => {
                append_result(&mut aggregate, result);
            }
            CoverAcquisitionOutcome::Completed(result) => {
                append_result(&mut aggregate, result);
                return Ok(aggregate);
            }
            CoverAcquisitionOutcome::Acquired(acquired) => {
                let result = complete_acquired_cover(acquired, &write_request)?;
                append_result(&mut aggregate, result);
                return Ok(aggregate);
            }
        }
    }

    if cover_fallback {
        let fallback = resource_adapter.extract_normal_images(&attempted_identities)?;
        append_result(&mut aggregate, fallback);
    }

    Ok(aggregate)
}

/// Required-cover format and filtering policy applied to bounded evidence.
struct RequiredCoverEvidencePolicy<'pipeline> {
    pipeline: &'pipeline ImageWritePipeline,
}

impl CoverEvidencePolicy for RequiredCoverEvidencePolicy<'_> {
    /// Returns a final filtered outcome or a request for the accepted payload.
    fn decide(&mut self, evidence: CoverEvidence<'_>) -> CoverEvidenceDecision {
        let mut warnings = Vec::new();
        let identified = ImageFormat::identify_source(ImageFormatSource {
            data: evidence.data,
            source_name: None,
            mime: Some(evidence.mime),
        });
        let format = identified
            .map(|identified| identified.format)
            .unwrap_or_else(|| {
                warnings.push(ImageWriteWarning::CoverDefaultToJpeg {
                    mime: evidence.mime.to_string(),
                });
                ImageFormat::Jpg
            });

        if !self.pipeline.accepts_format(format) {
            warnings.push(ImageWriteWarning::UnsupportedCoverFormat { format });
            CoverEvidenceDecision::Completed(ImageWriteResult::from_warnings(warnings))
        } else {
            CoverEvidenceDecision::Acquire { format, warnings }
        }
    }
}

/// Applies cover-specific conversion and delegates one accepted image to emission.
///
/// Routed GIFs preserve their original bytes. Conversion or format outcomes are
/// returned as typed warnings; Image file emission failures remain fatal errors.
fn complete_acquired_cover(
    acquired: AcquiredCover,
    request: &CoverWriteRequest<'_>,
) -> Result<ImageWriteResult> {
    let AcquiredCover {
        data,
        format,
        mut warnings,
    } = acquired;
    let (data, format, converted) = if request.pipeline.routes_gif(format) {
        (data, format, false)
    } else {
        match request.pipeline.conversion_policy() {
            Some(conversion) => match conversion.convert(&data, format) {
                Ok(ConversionOutcome::Converted(data, format)) => (data, format, true),
                Ok(ConversionOutcome::PreservedMatchingSource) => (data, format, false),
                Ok(ConversionOutcome::UnsupportedSource(format)) => {
                    warnings.push(ImageWriteWarning::CoverConversionSkipped { format });
                    return Ok(ImageWriteResult::from_warnings(warnings));
                }
                Err(error) => {
                    warnings.push(ImageWriteWarning::CoverConversionFailed {
                        message: error.to_string(),
                    });
                    return Ok(ImageWriteResult::from_warnings(warnings));
                }
            },
            None => (data, format, false),
        }
    };

    let mut emitted = request.pipeline.emit_single_image(
        request.output_dir,
        request.base_name,
        data,
        format,
        converted,
    )?;
    warnings.append(&mut emitted.warnings);
    emitted.warnings = warnings;
    Ok(emitted)
}
