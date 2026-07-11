//! Incremental Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;
use std::io::Read;

use crate::image_format::{FormatConfidence, ImageFormat, ImageFormatSource};

use super::{AcceptedImage, ArchiveImageSource, ImageWriteWarning};

// SVG inspection searches 1,024 bytes after an optional three-byte UTF-8 BOM.
const FORMAT_EVIDENCE_LIMIT: u64 = 1027;

/// One source's accepted payload and phase-ordered discovery warning facts.
pub(super) struct DiscoveredImage {
    pub(super) image: Option<AcceptedImage>,
    pub(super) warnings: Vec<ImageWriteWarning>,
}

/// Acquires one source incrementally and accepts it when its format is requested.
///
/// Only bounded evidence is read for rejected or filtered sources. Accepted
/// sources retain that prefix and append the remaining payload. Read failures
/// become structured warning facts and discard any partial bytes.
pub(super) fn discover_image(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
    if !is_source_safe(source) {
        return DiscoveredImage {
            image: None,
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
            image: None,
            warnings,
        };
    }

    let Some(identified) = ImageFormat::identify_source(ImageFormatSource {
        data: &data,
        source_name: source.format_source_name.as_deref(),
        mime: source.mime.as_deref(),
    }) else {
        return DiscoveredImage {
            image: None,
            warnings,
        };
    };

    match identified.confidence {
        FormatConfidence::ExtensionFallback => {
            if let Some(source_name) = &source.format_source_name {
                warnings.push(ImageWriteWarning::ExtensionFallback {
                    source_name: source_name.clone(),
                    format: identified.format,
                });
            }
        }
        FormatConfidence::Magic | FormatConfidence::MimeFallback => {}
    }

    if !allowed_formats.contains(&identified.format) {
        return DiscoveredImage {
            image: None,
            warnings,
        };
    }

    if let Err(error) = reader.read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            source.diagnostic_name.clone(),
            error,
        ));
        return DiscoveredImage {
            image: None,
            warnings,
        };
    }

    DiscoveredImage {
        image: Some(AcceptedImage {
            data,
            format: identified.format,
        }),
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
