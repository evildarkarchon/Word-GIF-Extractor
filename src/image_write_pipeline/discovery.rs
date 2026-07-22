//! Incremental Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;
use std::io::Read;

use crate::image_format::{FormatConfidence, ImageFormat, ImageFormatSource};

use super::purpose::{
    FilteredFormatAction, ImageWritePurpose, SourceEligibility, UnidentifiedFormatAction,
};
use super::{AcceptedImage, ArchiveImageSource, ImageWriteWarning};

// SVG inspection searches 1,024 bytes after an optional three-byte UTF-8 BOM.
pub(super) const FORMAT_EVIDENCE_LIMIT: u64 = 1027;

/// One source's typed discovery outcome and phase-ordered warning facts.
pub(super) struct DiscoveredImage {
    pub(super) outcome: ArchiveImageDiscoveryOutcome,
    pub(super) warnings: Vec<ImageWriteWarning>,
}

/// Purpose-independent completion state from one archive source discovery attempt.
pub(super) enum ArchiveImageDiscoveryOutcome {
    /// The complete payload was acquired and accepted by Image write policy.
    Accepted(AcceptedImage),
    /// Evidence produced a final non-emitting decision.
    Completed,
    /// Reading the source failed, permitting required-cover candidate retry.
    AcquisitionFailed,
}

/// Acquires one source incrementally through a statically selected Image write purpose.
///
/// Only bounded evidence is read for rejected or filtered sources. Accepted
/// sources retain that prefix and append the remaining payload.
pub(super) fn discover_image<P: ImageWritePurpose>(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
    purpose: &P,
) -> DiscoveredImage {
    if matches!(
        purpose.source_eligibility(source),
        SourceEligibility::Reject
    ) {
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings: Vec::new(),
        };
    }

    let mut warnings = Vec::new();
    let mut data = Vec::new();
    if let Err(error) = reader.take(FORMAT_EVIDENCE_LIMIT).read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            source.diagnostic_name.clone(),
            error,
        ));
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            warnings,
        };
    }

    let identified = ImageFormat::identify_source(ImageFormatSource {
        data: &data,
        source_name: source.format_source_name.as_deref(),
        mime: source.mime.as_deref(),
    });
    let (format, confidence) = match identified {
        Some(identified) => (identified.format, Some(identified.confidence)),
        None => {
            let decision = purpose.unidentified_format(source);
            if let Some(warning) = decision.warning {
                warnings.push(warning);
            }
            match decision.action {
                UnidentifiedFormatAction::ContinueWith(format) => (format, None),
                UnidentifiedFormatAction::Complete => {
                    return DiscoveredImage {
                        outcome: ArchiveImageDiscoveryOutcome::Completed,
                        warnings,
                    };
                }
            }
        }
    };

    match confidence {
        Some(FormatConfidence::ExtensionFallback) => {
            if let Some(source_name) = &source.format_source_name {
                warnings.push(ImageWriteWarning::ExtensionFallback {
                    source_name: source_name.clone(),
                    format,
                });
            }
        }
        Some(FormatConfidence::Magic | FormatConfidence::MimeFallback) | None => {}
    }

    if !allowed_formats.contains(&format) {
        let decision = purpose.filtered_format(format);
        if let Some(warning) = decision.warning {
            warnings.push(warning);
        }
        let FilteredFormatAction::CompleteWithoutEmission = decision.action;
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings,
        };
    }

    if let Err(error) = reader.read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            source.diagnostic_name.clone(),
            error,
        ));
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            warnings,
        };
    }

    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Accepted(AcceptedImage { data, format }),
        warnings,
    }
}
