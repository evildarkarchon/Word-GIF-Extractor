//! Tests for the project-level canonical image format vocabulary.

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
