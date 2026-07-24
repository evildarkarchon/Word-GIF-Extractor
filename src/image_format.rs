//! Project-level canonical image format vocabulary.
//!
//! The module owns canonical identities and extensions, user-facing aliases,
//! and conversion support policy. Archive evidence interpretation belongs to
//! Archive image discovery; callers pass `ImageFormat` values across module
//! seams instead of raw extensions or MIME types.

use std::collections::HashSet;

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
    fn parses_every_accepted_extension_to_its_canonical_format() {
        let accepted = [
            ("jpg", ImageFormat::Jpg),
            ("jpeg", ImageFormat::Jpg),
            ("png", ImageFormat::Png),
            ("gif", ImageFormat::Gif),
            ("bmp", ImageFormat::Bmp),
            ("tiff", ImageFormat::Tiff),
            ("tif", ImageFormat::Tiff),
            ("svg", ImageFormat::Svg),
            ("wmf", ImageFormat::Wmf),
            ("emf", ImageFormat::Emf),
            ("webp", ImageFormat::Webp),
            ("ico", ImageFormat::Ico),
        ];

        for (extension, expected) in accepted {
            assert_eq!(ImageFormat::from_extension(extension), Some(expected));
        }
    }

    #[test]
    fn exposes_the_canonical_extension_for_every_format() {
        let expected = [
            (ImageFormat::Jpg, "jpg"),
            (ImageFormat::Png, "png"),
            (ImageFormat::Gif, "gif"),
            (ImageFormat::Bmp, "bmp"),
            (ImageFormat::Tiff, "tiff"),
            (ImageFormat::Svg, "svg"),
            (ImageFormat::Wmf, "wmf"),
            (ImageFormat::Emf, "emf"),
            (ImageFormat::Webp, "webp"),
            (ImageFormat::Ico, "ico"),
        ];

        for (format, extension) in expected {
            assert_eq!(format.extension(), extension);
        }
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
}
