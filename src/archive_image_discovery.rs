//! Archive image discovery for deciding which archive resources become images.

use std::collections::HashSet;

use crate::image_format::{FormatConfidence, FormatFallbackPolicy, ImageFormat, ImageFormatSource};
use crate::image_writer::ImageToWrite;

/// Why an archive image source is being discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveImagePurpose {
    /// A normal document image discovered during batch extraction.
    BatchImage,
    /// A required EPUB cover image with cover-only fallback semantics.
    RequiredCover,
}

/// Raw archive resource facts consumed by Archive image discovery.
///
/// The source owns its bytes so callers do not leak lifetimes through the
/// module interface. `source_name` is validated when present; cover sources may
/// omit it and rely on MIME and cover fallback policy.
#[derive(Debug, Clone)]
pub struct ArchiveImageSource {
    /// Raw resource bytes.
    pub data: Vec<u8>,
    /// Optional archive path or resource name.
    pub source_name: Option<String>,
    /// Optional MIME type supplied by the document adapter.
    pub mime: Option<String>,
    /// Discovery behavior for this source.
    pub purpose: ArchiveImagePurpose,
}

impl ArchiveImageSource {
    /// Creates a normal batch image source from archive resource facts.
    pub fn batch(data: Vec<u8>, source_name: String, mime: Option<String>) -> Self {
        Self {
            data,
            source_name: Some(source_name),
            mime,
            purpose: ArchiveImagePurpose::BatchImage,
        }
    }

    /// Creates a required EPUB cover source.
    pub fn required_cover(data: Vec<u8>, mime: String) -> Self {
        Self {
            data,
            source_name: None,
            mime: Some(mime),
            purpose: ArchiveImagePurpose::RequiredCover,
        }
    }
}

/// User-visible warnings discovered while accepting archive image sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveImageDiscoveryWarning {
    /// Magic-byte detection failed and the source extension was used.
    ExtensionFallback {
        /// Archive path or resource name used for extension fallback.
        source_name: String,
        /// Image format accepted from the fallback extension.
        format: ImageFormat,
    },
    /// A required cover had no identifiable image facts and defaulted to JPEG.
    CoverDefaultToJpeg {
        /// MIME value that could not identify the cover format.
        mime: String,
    },
    /// A required cover's identified format was not requested by the user.
    UnsupportedCoverFormat {
        /// Identified cover format that was filtered out.
        format: ImageFormat,
    },
}

impl ArchiveImageDiscoveryWarning {
    /// Formats this warning using the existing terminal wording.
    pub fn message(&self) -> String {
        match self {
            ArchiveImageDiscoveryWarning::ExtensionFallback {
                source_name,
                format,
            } => format!(
                "Magic detection failed for {}; falling back to .{} extension",
                source_name,
                format.extension()
            ),
            ArchiveImageDiscoveryWarning::CoverDefaultToJpeg { mime } => format!(
                "Cover image MIME '{}' could not be identified; defaulting to .jpg extension.",
                mime
            ),
            ArchiveImageDiscoveryWarning::UnsupportedCoverFormat { format } => format!(
                "Cover image format '{}' not in allowed formats, skipping.",
                format.extension()
            ),
        }
    }
}

/// Accepted images and warnings produced by Archive image discovery.
#[derive(Debug, Default)]
pub struct DiscoveredImages {
    /// Images accepted for the Image write pipeline.
    pub images: Vec<ImageToWrite>,
    /// User-visible warning facts discovered while accepting sources.
    pub warnings: Vec<ArchiveImageDiscoveryWarning>,
}

/// Discovers writable images from archive source facts.
///
/// The module owns archive source safety, Image format identification, fallback
/// warning facts, and requested-format filtering. Unknown non-cover sources and
/// unsafe source names are skipped silently to preserve existing extraction
/// behavior.
pub fn discover_images(
    sources: Vec<ArchiveImageSource>,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImages {
    let mut result = DiscoveredImages::default();

    for source in sources {
        if source
            .source_name
            .as_deref()
            .is_some_and(|name| !is_safe_archive_path(name))
        {
            continue;
        }

        let fallback_policy = match source.purpose {
            ArchiveImagePurpose::BatchImage => FormatFallbackPolicy::SkipUnknown,
            ArchiveImagePurpose::RequiredCover => FormatFallbackPolicy::DefaultCoverToJpeg,
        };

        let Some(identified) = ImageFormat::identify_source(ImageFormatSource {
            data: &source.data,
            source_name: source.source_name.as_deref(),
            mime: source.mime.as_deref(),
            fallback_policy,
        }) else {
            continue;
        };

        match identified.confidence {
            FormatConfidence::ExtensionFallback => {
                if let Some(source_name) = &source.source_name {
                    result
                        .warnings
                        .push(ArchiveImageDiscoveryWarning::ExtensionFallback {
                            source_name: source_name.clone(),
                            format: identified.format,
                        });
                }
            }
            FormatConfidence::CoverDefault => {
                result
                    .warnings
                    .push(ArchiveImageDiscoveryWarning::CoverDefaultToJpeg {
                        mime: source.mime.clone().unwrap_or_default(),
                    });
            }
            FormatConfidence::Magic | FormatConfidence::MimeFallback => {}
        }

        if !allowed_formats.contains(&identified.format) {
            if source.purpose == ArchiveImagePurpose::RequiredCover {
                result
                    .warnings
                    .push(ArchiveImageDiscoveryWarning::UnsupportedCoverFormat {
                        format: identified.format,
                    });
            }
            continue;
        }

        result.images.push(ImageToWrite {
            data: source.data,
            format: identified.format,
        });
    }

    result
}

/// Validates that an archive entry path is safe for extraction decisions.
///
/// Returns `true` if the path is safe, `false` if it contains potentially
/// malicious patterns. Checks for null bytes, path traversal, absolute paths,
/// Windows drive letters, and Windows alternate data streams.
fn is_safe_archive_path(name: &str) -> bool {
    // Reject null bytes.
    if name.contains('\0') {
        return false;
    }
    // Reject path traversal.
    if name.contains("..") {
        return false;
    }
    // Reject absolute paths.
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    // Reject colons because they are invalid in Windows filenames and enable drive/ADS syntax.
    if name.contains(':') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR";

    #[test]
    fn skips_unsafe_source_names_silently() {
        let discovered = discover_images(
            vec![ArchiveImageSource::batch(
                MINIMAL_PNG.to_vec(),
                "../media/image.png".to_string(),
                None,
            )],
            &HashSet::from([ImageFormat::Png]),
        );

        assert!(discovered.images.is_empty());
        assert!(discovered.warnings.is_empty());
    }

    #[test]
    fn recognizes_safe_archive_paths() {
        assert!(is_safe_archive_path("word/media/image1.png"));
        assert!(is_safe_archive_path("image.jpg"));
        assert!(is_safe_archive_path("nested/folder/file.gif"));
    }

    #[test]
    fn rejects_path_traversal_archive_paths() {
        assert!(!is_safe_archive_path("../etc/passwd"));
        assert!(!is_safe_archive_path("foo/../bar"));
        assert!(!is_safe_archive_path(".."));
    }

    #[test]
    fn rejects_absolute_archive_paths() {
        assert!(!is_safe_archive_path("/etc/passwd"));
        assert!(!is_safe_archive_path("\\Windows\\System32"));
    }

    #[test]
    fn rejects_windows_drive_and_ads_paths() {
        assert!(!is_safe_archive_path("C:\\Windows\\System32\\calc.exe"));
        assert!(!is_safe_archive_path("D:/file.txt"));
        assert!(!is_safe_archive_path("D:file.txt"));
        assert!(!is_safe_archive_path("E:"));
        assert!(!is_safe_archive_path("a:config.json"));
        assert!(!is_safe_archive_path("file.txt::$DATA"));
        assert!(!is_safe_archive_path("file::stream"));
    }

    #[test]
    fn rejects_null_bytes_in_archive_paths() {
        assert!(!is_safe_archive_path("file\0.txt"));
    }

    #[test]
    fn magic_bytes_beat_extension_and_mime() {
        let discovered = discover_images(
            vec![ArchiveImageSource::batch(
                MINIMAL_PNG.to_vec(),
                "media/image.jpg".to_string(),
                Some("image/jpeg".to_string()),
            )],
            &HashSet::from([ImageFormat::Png]),
        );

        assert_eq!(discovered.images.len(), 1);
        assert_eq!(discovered.images[0].format, ImageFormat::Png);
        assert!(discovered.warnings.is_empty());
    }

    #[test]
    fn extension_fallback_returns_warning_fact() {
        let discovered = discover_images(
            vec![ArchiveImageSource::batch(
                b"unknown bytes".to_vec(),
                "media/image.jpeg".to_string(),
                None,
            )],
            &HashSet::from([ImageFormat::Jpg]),
        );

        assert_eq!(discovered.images.len(), 1);
        assert_eq!(discovered.images[0].format, ImageFormat::Jpg);
        assert_eq!(
            discovered.warnings,
            vec![ArchiveImageDiscoveryWarning::ExtensionFallback {
                source_name: "media/image.jpeg".to_string(),
                format: ImageFormat::Jpg,
            }]
        );
    }

    #[test]
    fn unknown_non_cover_source_is_skipped_silently() {
        let discovered = discover_images(
            vec![ArchiveImageSource::batch(
                b"unknown bytes".to_vec(),
                "media/image.bin".to_string(),
                Some("application/octet-stream".to_string()),
            )],
            &ImageFormat::all_set(),
        );

        assert!(discovered.images.is_empty());
        assert!(discovered.warnings.is_empty());
    }

    #[test]
    fn required_cover_defaults_to_jpeg_when_source_facts_are_unknown() {
        let discovered = discover_images(
            vec![ArchiveImageSource::required_cover(
                b"unknown bytes".to_vec(),
                "application/octet-stream".to_string(),
            )],
            &HashSet::from([ImageFormat::Jpg]),
        );

        assert_eq!(discovered.images.len(), 1);
        assert_eq!(discovered.images[0].format, ImageFormat::Jpg);
        assert_eq!(
            discovered.warnings,
            vec![ArchiveImageDiscoveryWarning::CoverDefaultToJpeg {
                mime: "application/octet-stream".to_string(),
            }]
        );
    }

    #[test]
    fn allowed_format_filtering_happens_after_cover_fallback() {
        let discovered = discover_images(
            vec![ArchiveImageSource::required_cover(
                b"unknown bytes".to_vec(),
                "application/octet-stream".to_string(),
            )],
            &HashSet::from([ImageFormat::Png]),
        );

        assert!(discovered.images.is_empty());
        assert_eq!(
            discovered.warnings,
            vec![
                ArchiveImageDiscoveryWarning::CoverDefaultToJpeg {
                    mime: "application/octet-stream".to_string(),
                },
                ArchiveImageDiscoveryWarning::UnsupportedCoverFormat {
                    format: ImageFormat::Jpg,
                }
            ]
        );
    }
}
