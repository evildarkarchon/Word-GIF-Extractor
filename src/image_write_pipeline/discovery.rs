//! Incremental Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;
use std::io::Read;

use crate::image_format::{FormatConfidence, ImageFormat, ImageFormatSource};

use super::{AcceptedImage, ArchiveImageSource, ImageWritePurpose, ImageWriteWarning};

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

/// Acquires one normal source incrementally and accepts it when its format is requested.
///
/// Only bounded evidence is read for rejected or filtered sources. Accepted
/// sources retain that prefix and append the remaining payload.
pub(super) fn discover_image(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
    discover_image_for_purpose(
        source,
        reader,
        allowed_formats,
        ImageWritePurpose::NormalImages,
    )
}

/// Acquires one required cover incrementally using cover-specific discovery outcomes.
pub(super) fn discover_required_cover(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
    discover_image_for_purpose(
        source,
        reader,
        allowed_formats,
        ImageWritePurpose::RequiredCover,
    )
}

/// Shares bounded acquisition and format identification across Image write purposes.
///
/// `purpose` selects source-safety, unidentified-cover, and filtered-cover policy.
/// The result distinguishes accepted payloads, final non-emitting decisions, and
/// acquisition failures without requiring callers to infer state from warnings.
fn discover_image_for_purpose(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
    purpose: ImageWritePurpose,
) -> DiscoveredImage {
    if matches!(purpose, ImageWritePurpose::NormalImages) && !is_source_safe(source) {
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
        None if matches!(purpose, ImageWritePurpose::RequiredCover) => {
            warnings.push(ImageWriteWarning::CoverDefaultToJpeg {
                mime: source.mime.clone().unwrap_or_default(),
            });
            (ImageFormat::Jpg, None)
        }
        None => {
            return DiscoveredImage {
                outcome: ArchiveImageDiscoveryOutcome::Completed,
                warnings,
            };
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
        if matches!(purpose, ImageWritePurpose::RequiredCover) {
            warnings.push(ImageWriteWarning::UnsupportedCoverFormat { format });
        }
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

/// Returns whether discovery would inspect this source.
pub(super) fn is_source_safe(source: &ArchiveImageSource) -> bool {
    source
        .format_source_name
        .as_deref()
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
