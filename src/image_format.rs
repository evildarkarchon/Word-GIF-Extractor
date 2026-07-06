//! Project-level image format vocabulary and detection.
//!
//! The module owns the canonical image format set, user-facing aliases, MIME
//! mapping, magic-byte detection, and conversion support policy. Callers should
//! pass `ImageFormat` values across module seams instead of raw extensions.

use std::collections::HashSet;
use std::path::Path;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1A\n";
const JPG_MAGIC: &[u8] = b"\xFF\xD8\xFF";
const GIF_MAGIC: &[u8] = b"GIF8";
const BMP_MAGIC: &[u8] = b"BM";
const TIFF_LE_MAGIC: &[u8] = b"II\x2A\x00";
const TIFF_BE_MAGIC: &[u8] = b"MM\x00\x2A";
const RIFF_MAGIC: &[u8] = b"RIFF";
const WEBP_MAGIC: &[u8] = b"WEBP";
const ICO_MAGIC: &[u8] = b"\x00\x00\x01\x00";
const WMF_PLACEABLE_MAGIC: &[u8] = b"\xD7\xCD\xC6\x9A";
const WMF_STANDARD_MAGIC: &[u8] = b"\x01\x00\x09\x00";
const EMF_PREFIX_MAGIC: &[u8] = b"\x01\x00\x00\x00";
const EMF_SIGNATURE_OFFSET: usize = 40;
const EMF_SIGNATURE: &[u8] = b" EMF";
const SVG_SEARCH_LIMIT: usize = 1024;

const ALL_FORMATS: [ImageFormat; 10] = [
    ImageFormat::Jpg,
    ImageFormat::Png,
    ImageFormat::Gif,
    ImageFormat::Bmp,
    ImageFormat::Tiff,
    ImageFormat::Svg,
    ImageFormat::Wmf,
    ImageFormat::Emf,
    ImageFormat::Webp,
    ImageFormat::Ico,
];

/// Canonical image formats supported by the extractor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// JPEG image data.
    Jpg,
    /// PNG image data.
    Png,
    /// GIF image data.
    Gif,
    /// BMP image data.
    Bmp,
    /// TIFF image data.
    Tiff,
    /// SVG image data.
    Svg,
    /// Windows Metafile image data.
    Wmf,
    /// Enhanced Metafile image data.
    Emf,
    /// WebP image data.
    Webp,
    /// Icon image data.
    Ico,
}

/// How an image format was identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatConfidence {
    /// The format was identified from file header/content bytes.
    Magic,
    /// The format was identified from a supported path extension.
    ExtensionFallback,
    /// The format was identified from a MIME type.
    MimeFallback,
    /// The format used the EPUB cover compatibility default.
    CoverDefault,
}

/// The result of identifying an image format and the source of that decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentifiedFormat {
    /// Canonical image format.
    pub format: ImageFormat,
    /// Source used to identify the image format.
    pub confidence: FormatConfidence,
}

/// Compatibility policy used when bytes, source name, and MIME cannot identify an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatFallbackPolicy {
    /// Unknown sources are skipped when no supported format can be identified.
    SkipUnknown,
    /// Unknown EPUB cover images default to JPEG to preserve legacy cover extraction.
    DefaultCoverToJpeg,
}

/// Source facts used to identify an image format.
///
/// Magic bytes always win. If magic detection fails, `source_name` is checked
/// before `mime`, preserving existing non-cover EPUB precedence while allowing
/// covers to pass only bytes and MIME before applying their cover fallback.
#[derive(Debug, Clone, Copy)]
pub struct ImageFormatSource<'a> {
    /// Raw image bytes to inspect for magic values.
    pub data: &'a [u8],
    /// Optional document/archive source name used for extension fallback.
    pub source_name: Option<&'a str>,
    /// Optional MIME type used when magic and extension identification fail.
    pub mime: Option<&'a str>,
    /// Final compatibility behavior when no source fact identifies a format.
    pub fallback_policy: FormatFallbackPolicy,
}

impl ImageFormat {
    /// Returns every extractable image format.
    pub fn all() -> &'static [ImageFormat] {
        &ALL_FORMATS
    }

    /// Returns every extractable image format as a set.
    pub fn all_set() -> HashSet<ImageFormat> {
        Self::all().iter().copied().collect()
    }

    /// Returns the canonical file extension for this image format.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Jpg => "jpg",
            ImageFormat::Png => "png",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Svg => "svg",
            ImageFormat::Wmf => "wmf",
            ImageFormat::Emf => "emf",
            ImageFormat::Webp => "webp",
            ImageFormat::Ico => "ico",
        }
    }

    /// Parses a user-facing image format token into the canonical format.
    pub fn from_user_format(input: &str) -> Option<ImageFormat> {
        ImageFormat::from_extension(input)
    }

    /// Parses a filename extension into the canonical image format.
    pub fn from_extension(extension: &str) -> Option<ImageFormat> {
        let normalized = extension
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();

        match normalized.as_str() {
            "jpg" | "jpeg" => Some(ImageFormat::Jpg),
            "png" => Some(ImageFormat::Png),
            "gif" => Some(ImageFormat::Gif),
            "bmp" => Some(ImageFormat::Bmp),
            "tiff" | "tif" => Some(ImageFormat::Tiff),
            "svg" => Some(ImageFormat::Svg),
            "wmf" => Some(ImageFormat::Wmf),
            "emf" => Some(ImageFormat::Emf),
            "webp" => Some(ImageFormat::Webp),
            "ico" => Some(ImageFormat::Ico),
            _ => None,
        }
    }

    /// Parses a MIME type into the canonical image format.
    pub fn from_mime(mime: &str) -> Option<ImageFormat> {
        let normalized = mime
            .split(';')
            .next()
            .unwrap_or(mime)
            .trim()
            .to_ascii_lowercase();

        match normalized.as_str() {
            "image/jpeg" => Some(ImageFormat::Jpg),
            "image/png" => Some(ImageFormat::Png),
            "image/gif" => Some(ImageFormat::Gif),
            "image/bmp" => Some(ImageFormat::Bmp),
            "image/tiff" => Some(ImageFormat::Tiff),
            "image/svg+xml" => Some(ImageFormat::Svg),
            "image/x-emf" | "image/emf" => Some(ImageFormat::Emf),
            "image/x-wmf" | "image/wmf" => Some(ImageFormat::Wmf),
            "image/webp" => Some(ImageFormat::Webp),
            "image/x-icon" | "image/vnd.microsoft.icon" => Some(ImageFormat::Ico),
            _ => None,
        }
    }

    /// Identifies a supported image format from a MIME type.
    // Retained for low-churn call sites even when all current extraction paths use identify_source.
    #[allow(dead_code)]
    pub fn identify_mime(mime: &str) -> Option<IdentifiedFormat> {
        ImageFormat::identify_source(ImageFormatSource {
            data: &[],
            source_name: None,
            mime: Some(mime),
            fallback_policy: FormatFallbackPolicy::SkipUnknown,
        })
    }

    /// Identifies a supported image format from bytes, falling back to a path extension.
    // Retained for low-churn call sites and direct Image format tests.
    #[allow(dead_code)]
    pub fn identify(data: &[u8], source_name: &str) -> Option<IdentifiedFormat> {
        ImageFormat::identify_source(ImageFormatSource {
            data,
            source_name: Some(source_name),
            mime: None,
            fallback_policy: FormatFallbackPolicy::SkipUnknown,
        })
    }

    /// Identifies a supported image format from source facts using project precedence.
    ///
    /// The interface is intentionally request-shaped so DOCX and EPUB callers
    /// cross the same seam. It returns `None` only when the supplied facts and
    /// fallback policy cannot identify an extractable image format.
    pub fn identify_source(source: ImageFormatSource<'_>) -> Option<IdentifiedFormat> {
        if let Some(format) = ImageFormat::from_magic(source.data) {
            return Some(IdentifiedFormat {
                format,
                confidence: FormatConfidence::Magic,
            });
        }

        if let Some(source_name) = source.source_name
            && let Some(format) = Path::new(source_name)
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(ImageFormat::from_extension)
        {
            return Some(IdentifiedFormat {
                format,
                confidence: FormatConfidence::ExtensionFallback,
            });
        }

        if let Some(mime) = source.mime
            && let Some(format) = ImageFormat::from_mime(mime)
        {
            return Some(IdentifiedFormat {
                format,
                confidence: FormatConfidence::MimeFallback,
            });
        }

        match source.fallback_policy {
            FormatFallbackPolicy::SkipUnknown => None,
            FormatFallbackPolicy::DefaultCoverToJpeg => Some(IdentifiedFormat {
                format: ImageFormat::Jpg,
                confidence: FormatConfidence::CoverDefault,
            }),
        }
    }

    /// Detects a supported image format from header/content bytes.
    pub fn from_magic(data: &[u8]) -> Option<ImageFormat> {
        if data.starts_with(PNG_MAGIC) {
            return Some(ImageFormat::Png);
        }
        if data.starts_with(JPG_MAGIC) {
            return Some(ImageFormat::Jpg);
        }
        if data.starts_with(GIF_MAGIC) {
            return Some(ImageFormat::Gif);
        }
        if data.starts_with(BMP_MAGIC) {
            return Some(ImageFormat::Bmp);
        }
        if data.starts_with(TIFF_LE_MAGIC) || data.starts_with(TIFF_BE_MAGIC) {
            return Some(ImageFormat::Tiff);
        }
        if data.len() >= 12 && data.starts_with(RIFF_MAGIC) && &data[8..12] == WEBP_MAGIC {
            return Some(ImageFormat::Webp);
        }
        if data.starts_with(ICO_MAGIC) {
            return Some(ImageFormat::Ico);
        }
        if data.starts_with(WMF_PLACEABLE_MAGIC) || data.starts_with(WMF_STANDARD_MAGIC) {
            return Some(ImageFormat::Wmf);
        }
        if is_emf(data) {
            return Some(ImageFormat::Emf);
        }
        if is_svg(data) {
            return Some(ImageFormat::Svg);
        }

        None
    }

    /// Returns whether this image format can be decoded for conversion.
    pub fn can_convert(self) -> bool {
        matches!(
            self,
            ImageFormat::Jpg
                | ImageFormat::Png
                | ImageFormat::Gif
                | ImageFormat::Bmp
                | ImageFormat::Tiff
                | ImageFormat::Webp
                | ImageFormat::Ico
        )
    }
}

fn is_emf(data: &[u8]) -> bool {
    data.starts_with(EMF_PREFIX_MAGIC)
        && data
            .get(EMF_SIGNATURE_OFFSET..EMF_SIGNATURE_OFFSET + EMF_SIGNATURE.len())
            .is_some_and(|signature| signature == EMF_SIGNATURE)
}

fn is_svg(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let search_window = &data[..data.len().min(SVG_SEARCH_LIMIT)];

    (0..search_window.len()).any(|start| {
        starts_with_ignore_ascii_case(&search_window[start..], b"<svg")
            && matches!(
                search_window.get(start + 4),
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>')
            )
    })
}

fn starts_with_ignore_ascii_case(data: &[u8], prefix: &[u8]) -> bool {
    data.len() >= prefix.len()
        && data[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_format_aliases_to_canonical_formats() {
        assert_eq!(ImageFormat::from_user_format("jpg"), Some(ImageFormat::Jpg));
        assert_eq!(
            ImageFormat::from_user_format("JPEG"),
            Some(ImageFormat::Jpg)
        );
        assert_eq!(
            ImageFormat::from_user_format("tif"),
            Some(ImageFormat::Tiff)
        );
        assert_eq!(ImageFormat::from_user_format("unknown"), None);
    }

    #[test]
    fn maps_mime_types_to_canonical_formats() {
        assert_eq!(ImageFormat::from_mime("image/jpeg"), Some(ImageFormat::Jpg));
        assert_eq!(
            ImageFormat::from_mime("image/png; charset=binary"),
            Some(ImageFormat::Png)
        );
        assert_eq!(ImageFormat::from_mime("image/gif"), Some(ImageFormat::Gif));
        assert_eq!(ImageFormat::from_mime("image/unknown"), None);
    }

    #[test]
    fn knows_conversion_support() {
        assert!(ImageFormat::Jpg.can_convert());
        assert!(ImageFormat::Png.can_convert());
        assert!(ImageFormat::Gif.can_convert());
        assert!(ImageFormat::Bmp.can_convert());
        assert!(ImageFormat::Tiff.can_convert());
        assert!(ImageFormat::Webp.can_convert());
        assert!(ImageFormat::Ico.can_convert());
        assert!(!ImageFormat::Svg.can_convert());
        assert!(!ImageFormat::Wmf.can_convert());
        assert!(!ImageFormat::Emf.can_convert());
    }

    #[test]
    fn identifies_magic_before_extension() {
        let identified = ImageFormat::identify(b"\x89PNG\r\n\x1A\n", "word/media/image.bin");
        assert_eq!(
            identified,
            Some(IdentifiedFormat {
                format: ImageFormat::Png,
                confidence: FormatConfidence::Magic
            })
        );
    }

    #[test]
    fn identifies_supported_extension_fallback() {
        let identified = ImageFormat::identify(b"unknown", "word/media/image.JPEG");
        assert_eq!(
            identified,
            Some(IdentifiedFormat {
                format: ImageFormat::Jpg,
                confidence: FormatConfidence::ExtensionFallback
            })
        );
    }

    #[test]
    fn ignores_unsupported_extension_fallback() {
        assert_eq!(
            ImageFormat::identify(b"unknown", "word/media/image.bin"),
            None
        );
    }

    #[test]
    fn identifies_source_with_magic_before_extension_and_mime() {
        let identified = ImageFormat::identify_source(ImageFormatSource {
            data: b"\x89PNG\r\n\x1A\n",
            source_name: Some("OEBPS/images/cover.jpg"),
            mime: Some("image/jpeg"),
            fallback_policy: FormatFallbackPolicy::SkipUnknown,
        });

        assert_eq!(
            identified,
            Some(IdentifiedFormat {
                format: ImageFormat::Png,
                confidence: FormatConfidence::Magic
            })
        );
    }

    #[test]
    fn identifies_source_with_extension_before_mime_when_magic_fails() {
        let identified = ImageFormat::identify_source(ImageFormatSource {
            data: b"unknown image bytes",
            source_name: Some("OEBPS/images/cover.png"),
            mime: Some("image/jpeg"),
            fallback_policy: FormatFallbackPolicy::SkipUnknown,
        });

        assert_eq!(
            identified,
            Some(IdentifiedFormat {
                format: ImageFormat::Png,
                confidence: FormatConfidence::ExtensionFallback
            })
        );
    }

    #[test]
    fn identifies_source_with_mime_when_magic_and_extension_fail() {
        let identified = ImageFormat::identify_source(ImageFormatSource {
            data: b"unknown image bytes",
            source_name: Some("OEBPS/images/cover.bin"),
            mime: Some("image/webp; charset=binary"),
            fallback_policy: FormatFallbackPolicy::SkipUnknown,
        });

        assert_eq!(
            identified,
            Some(IdentifiedFormat {
                format: ImageFormat::Webp,
                confidence: FormatConfidence::MimeFallback
            })
        );
    }

    #[test]
    fn identifies_source_with_cover_default_only_after_other_sources_fail() {
        let identified = ImageFormat::identify_source(ImageFormatSource {
            data: b"unknown cover bytes",
            source_name: None,
            mime: Some("application/octet-stream"),
            fallback_policy: FormatFallbackPolicy::DefaultCoverToJpeg,
        });

        assert_eq!(
            identified,
            Some(IdentifiedFormat {
                format: ImageFormat::Jpg,
                confidence: FormatConfidence::CoverDefault
            })
        );
    }

    #[test]
    fn detects_png() {
        assert_eq!(
            ImageFormat::from_magic(b"\x89PNG\r\n\x1A\nmore"),
            Some(ImageFormat::Png)
        );
    }

    #[test]
    fn detects_jpg() {
        assert_eq!(
            ImageFormat::from_magic(b"\xFF\xD8\xFF\xE0"),
            Some(ImageFormat::Jpg)
        );
    }

    #[test]
    fn detects_gif() {
        assert_eq!(ImageFormat::from_magic(b"GIF89a"), Some(ImageFormat::Gif));
    }

    #[test]
    fn detects_bmp() {
        assert_eq!(ImageFormat::from_magic(b"BMrest"), Some(ImageFormat::Bmp));
    }

    #[test]
    fn detects_tiff_little_endian() {
        assert_eq!(
            ImageFormat::from_magic(b"II\x2A\x00rest"),
            Some(ImageFormat::Tiff)
        );
    }

    #[test]
    fn detects_tiff_big_endian() {
        assert_eq!(
            ImageFormat::from_magic(b"MM\x00\x2Arest"),
            Some(ImageFormat::Tiff)
        );
    }

    #[test]
    fn detects_webp() {
        assert_eq!(
            ImageFormat::from_magic(b"RIFF\x00\x00\x00\x00WEBP"),
            Some(ImageFormat::Webp)
        );
    }

    #[test]
    fn detects_ico() {
        assert_eq!(
            ImageFormat::from_magic(b"\x00\x00\x01\x00rest"),
            Some(ImageFormat::Ico)
        );
    }

    #[test]
    fn detects_wmf_placeable() {
        assert_eq!(
            ImageFormat::from_magic(b"\xD7\xCD\xC6\x9Arest"),
            Some(ImageFormat::Wmf)
        );
    }

    #[test]
    fn detects_wmf_standard() {
        assert_eq!(
            ImageFormat::from_magic(b"\x01\x00\x09\x00rest"),
            Some(ImageFormat::Wmf)
        );
    }

    #[test]
    fn detects_emf() {
        let mut data = vec![0x01, 0x00, 0x00, 0x00];
        data.resize(40, 0);
        data.extend_from_slice(b" EMF");
        assert_eq!(ImageFormat::from_magic(&data), Some(ImageFormat::Emf));
    }

    #[test]
    fn ignores_emf_prefix_without_signature() {
        let mut data = vec![0x01, 0x00, 0x00, 0x00];
        data.resize(44, 0);
        assert_eq!(ImageFormat::from_magic(&data), None);
    }

    #[test]
    fn detects_svg_with_svg_prefix() {
        assert_eq!(
            ImageFormat::from_magic(b"  <svg xmlns=\"x\"/>"),
            Some(ImageFormat::Svg)
        );
    }

    #[test]
    fn detects_svg_with_xml_prefix() {
        assert_eq!(
            ImageFormat::from_magic(b"\xEF\xBB\xBF\n<?xml version=\"1.0\"?><svg/>"),
            Some(ImageFormat::Svg)
        );
    }

    #[test]
    fn ignores_non_svg_xml() {
        assert_eq!(
            ImageFormat::from_magic(b"<?xml version=\"1.0\"?><root/>"),
            None
        );
    }

    #[test]
    fn returns_none_for_unknown_data() {
        assert_eq!(ImageFormat::from_magic(b"not an image"), None);
    }

    #[test]
    fn returns_none_for_short_data() {
        assert_eq!(ImageFormat::from_magic(b"RIF"), None);
    }
}
