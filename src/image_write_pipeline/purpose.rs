//! Purpose-specific semantic decisions for the Image write pipeline.

use std::fmt;

use crate::image_format::ImageFormat;

use super::{ArchiveImageSource, ImageWriteWarning};

/// Whether Archive image discovery may inspect one source.
pub(super) enum SourceEligibility {
    Inspect,
    Reject,
}

/// Purpose-selected continuation after unidentified Image format evidence.
pub(super) enum UnidentifiedFormatAction {
    ContinueWith(ImageFormat),
    Complete,
}

/// Purpose-selected completion after an identified format is filtered out.
pub(super) enum FilteredFormatAction {
    CompleteWithoutEmission,
}

/// Purpose-selected continuation after conversion cannot produce requested bytes.
pub(super) enum ConversionAction {
    PreserveOriginal,
    CompleteWithoutEmission,
}

/// One typed purpose action and its optional existing warning fact.
pub(super) struct PurposeDecision<Action> {
    pub(super) action: Action,
    pub(super) warning: Option<ImageWriteWarning>,
}

/// Private semantic boundary implemented by each real Image write purpose.
pub(super) trait ImageWritePurpose {
    /// Decides whether discovery may touch the source reader.
    fn source_eligibility(&self, source: &ArchiveImageSource) -> SourceEligibility;

    /// Decides how discovery proceeds when available evidence identifies no format.
    fn unidentified_format(
        &self,
        source: &ArchiveImageSource,
    ) -> PurposeDecision<UnidentifiedFormatAction>;

    /// Decides how discovery completes when the identified format is filtered out.
    fn filtered_format(&self, format: ImageFormat) -> PurposeDecision<FilteredFormatAction>;

    /// Decides whether an unsupported conversion source may be emitted unchanged.
    fn unsupported_conversion(
        &self,
        base_name: &str,
        format: ImageFormat,
    ) -> PurposeDecision<ConversionAction>;

    /// Decides whether bytes from a failed conversion may be emitted unchanged.
    fn failed_conversion(
        &self,
        base_name: &str,
        error: &dyn fmt::Display,
    ) -> PurposeDecision<ConversionAction>;
}

/// Statically selected purpose for plural normal-image traversal.
#[derive(Debug, Clone, Copy)]
pub(super) struct NormalImages;

impl ImageWritePurpose for NormalImages {
    /// Rejects unsafe normal-image sources before their readers are touched.
    fn source_eligibility(&self, source: &ArchiveImageSource) -> SourceEligibility {
        if is_normal_source_safe(source) {
            SourceEligibility::Inspect
        } else {
            SourceEligibility::Reject
        }
    }

    /// Completes unidentified normal-image discovery without a warning.
    fn unidentified_format(
        &self,
        _source: &ArchiveImageSource,
    ) -> PurposeDecision<UnidentifiedFormatAction> {
        PurposeDecision {
            action: UnidentifiedFormatAction::Complete,
            warning: None,
        }
    }

    /// Completes filtered normal-image discovery without a warning.
    fn filtered_format(&self, _format: ImageFormat) -> PurposeDecision<FilteredFormatAction> {
        PurposeDecision {
            action: FilteredFormatAction::CompleteWithoutEmission,
            warning: None,
        }
    }

    /// Preserves unsupported normal-image bytes and records a skipped conversion.
    fn unsupported_conversion(
        &self,
        base_name: &str,
        format: ImageFormat,
    ) -> PurposeDecision<ConversionAction> {
        PurposeDecision {
            action: ConversionAction::PreserveOriginal,
            warning: Some(ImageWriteWarning::ConversionSkipped {
                base_name: base_name.to_string(),
                format,
            }),
        }
    }

    /// Preserves normal-image bytes after a failed conversion and records the failure.
    fn failed_conversion(
        &self,
        base_name: &str,
        error: &dyn fmt::Display,
    ) -> PurposeDecision<ConversionAction> {
        PurposeDecision {
            action: ConversionAction::PreserveOriginal,
            warning: Some(ImageWriteWarning::ConversionFailed {
                base_name: base_name.to_string(),
                detail: error.to_string(),
            }),
        }
    }
}

/// Statically selected purpose for one required EPUB cover attempt.
#[derive(Debug, Clone, Copy)]
pub(super) struct RequiredCover;

impl ImageWritePurpose for RequiredCover {
    /// Allows the required-cover reader supplied by the EPUB adapter.
    fn source_eligibility(&self, _source: &ArchiveImageSource) -> SourceEligibility {
        SourceEligibility::Inspect
    }

    /// Defaults unidentified required-cover evidence to JPEG with the existing warning.
    fn unidentified_format(
        &self,
        source: &ArchiveImageSource,
    ) -> PurposeDecision<UnidentifiedFormatAction> {
        PurposeDecision {
            action: UnidentifiedFormatAction::ContinueWith(ImageFormat::Jpg),
            warning: Some(ImageWriteWarning::CoverDefaultToJpeg {
                mime: source.declared_mime().unwrap_or_default().to_string(),
            }),
        }
    }

    /// Completes a filtered required cover with the existing unsupported warning.
    fn filtered_format(&self, format: ImageFormat) -> PurposeDecision<FilteredFormatAction> {
        PurposeDecision {
            action: FilteredFormatAction::CompleteWithoutEmission,
            warning: Some(ImageWriteWarning::UnsupportedCoverFormat { format }),
        }
    }

    /// Completes unsupported required-cover conversion without emitting original bytes.
    fn unsupported_conversion(
        &self,
        _base_name: &str,
        format: ImageFormat,
    ) -> PurposeDecision<ConversionAction> {
        PurposeDecision {
            action: ConversionAction::CompleteWithoutEmission,
            warning: Some(ImageWriteWarning::CoverConversionSkipped { format }),
        }
    }

    /// Completes failed required-cover conversion without emitting original bytes.
    fn failed_conversion(
        &self,
        _base_name: &str,
        error: &dyn fmt::Display,
    ) -> PurposeDecision<ConversionAction> {
        PurposeDecision {
            action: ConversionAction::CompleteWithoutEmission,
            warning: Some(ImageWriteWarning::CoverConversionFailed {
                detail: error.to_string(),
            }),
        }
    }
}

/// Returns whether discovery would inspect this normal-image source.
fn is_normal_source_safe(source: &ArchiveImageSource) -> bool {
    source
        .path_evidence_name()
        .is_some_and(is_safe_archive_path)
}

/// Returns whether an archive path is safe to use as image source evidence.
fn is_safe_archive_path(name: &str) -> bool {
    if name.contains('\0') || name.contains("..") {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    // Colons enable drive-letter and alternate-data-stream syntax on Windows.
    if name.contains(':') {
        return false;
    }
    true
}
