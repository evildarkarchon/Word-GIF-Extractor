//! Tests for image format conversion.

use super::*;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat as EncodedImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

/// Creates a 2x2 RGBA PNG with transparent pixels for alpha compositing tests
fn create_test_rgba_png() -> Vec<u8> {
    let mut img = RgbaImage::new(2, 2);
    img.put_pixel(0, 0, Rgba([255, 0, 0, 255])); // red, opaque
    img.put_pixel(1, 0, Rgba([0, 255, 0, 128])); // green, 50% alpha
    img.put_pixel(0, 1, Rgba([0, 0, 255, 0])); // blue, fully transparent
    img.put_pixel(1, 1, Rgba([255, 255, 255, 255])); // white, opaque

    let dynamic = DynamicImage::ImageRgba8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic.write_to(&mut buf, EncodedImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Creates a 4x4 RGB JPEG for testing (no alpha channel)
fn create_test_rgb_jpeg() -> Vec<u8> {
    use image::RgbImage;
    let mut img = RgbImage::new(4, 4);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 60) as u8, (y * 60) as u8, 128]);
    }
    let dynamic = DynamicImage::ImageRgb8(img);
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, 95);
    dynamic.write_with_encoder(encoder).unwrap();
    buf
}

/// Creates a 4x4 BMP for testing
fn create_test_bmp() -> Vec<u8> {
    use image::RgbImage;
    let img = RgbImage::new(4, 4);
    let dynamic = DynamicImage::ImageRgb8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic.write_to(&mut buf, EncodedImageFormat::Bmp).unwrap();
    buf.into_inner()
}

/// Creates a 4x4 GIF for testing
fn create_test_gif() -> Vec<u8> {
    let mut img = RgbaImage::new(4, 4);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = Rgba([(x * 60) as u8, (y * 60) as u8, 128, 255]);
    }
    let dynamic = DynamicImage::ImageRgba8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic.write_to(&mut buf, EncodedImageFormat::Gif).unwrap();
    buf.into_inner()
}

/// Creates a 4x4 TIFF for testing
fn create_test_tiff() -> Vec<u8> {
    use image::RgbImage;
    let mut img = RgbImage::new(4, 4);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 60) as u8, (y * 60) as u8, 128]);
    }
    let dynamic = DynamicImage::ImageRgb8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut buf, EncodedImageFormat::Tiff)
        .unwrap();
    buf.into_inner()
}

/// Creates a 4x4 WebP for testing
fn create_test_webp() -> Vec<u8> {
    use image::RgbImage;
    let mut img = RgbImage::new(4, 4);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x * 60) as u8, (y * 60) as u8, 128]);
    }
    let dynamic = DynamicImage::ImageRgb8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut buf, EncodedImageFormat::WebP)
        .unwrap();
    buf.into_inner()
}

/// Creates a 256x256 photographic-style test image with smooth gradients.
/// Larger size ensures lossy WebP is smaller than lossless for photographic content.
fn create_photographic_test_image() -> Vec<u8> {
    use image::RgbImage;
    let mut img = RgbImage::new(256, 256);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([x as u8, y as u8, ((x + y) / 2) as u8]);
    }
    let dynamic = DynamicImage::ImageRgb8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic.write_to(&mut buf, EncodedImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Creates a 16x16 RGBA PNG with a large transparent region for alpha compositing tests.
/// Uses a larger image to avoid JPEG cross-pixel color bleeding artifacts.
fn create_alpha_test_png() -> Vec<u8> {
    let mut img = RgbaImage::new(16, 16);
    // Fill entire image with fully transparent pixels
    for (_, _, pixel) in img.enumerate_pixels_mut() {
        *pixel = Rgba([0, 0, 255, 0]); // blue, fully transparent
    }
    // Put a small opaque red region in the top-left corner
    img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));

    let dynamic = DynamicImage::ImageRgba8(img);
    let mut buf = Cursor::new(Vec::new());
    dynamic.write_to(&mut buf, EncodedImageFormat::Png).unwrap();
    buf.into_inner()
}

#[test]
fn matching_jpeg_is_preserved_when_quality_is_omitted() {
    let policy = ConversionPolicy::try_from(ConversionRequest {
        target: ConversionTarget::Jpg,
        quality: None,
        lossless: false,
    })
    .expect("default JPEG conversion policy should be valid");
    let input = create_test_rgb_jpeg();

    let outcome = policy
        .convert(&input, ImageFormat::Jpg)
        .expect("matching JPEG should be handled without codec failure");

    assert!(matches!(
        outcome,
        ConversionOutcome::PreservedMatchingSource
    ));
}

#[test]
fn test_jpeg_alpha_compositing_white_background() {
    let png_data = create_alpha_test_png();
    let result = convert_image(&png_data, CodecTarget::Jpg, 85, false)
        .expect("convert_image should succeed");
    let jpeg_bytes = result.expect("should return Some(bytes)");

    // Decode the JPEG result
    let decoded = image::load_from_memory(&jpeg_bytes).expect("should decode JPEG output");
    let rgb = decoded.to_rgb8();

    // Check a pixel in the center of the transparent region (8,8)
    // should be white, not black -- far enough from edges to avoid JPEG bleed
    let pixel = rgb.get_pixel(8, 8);
    assert!(
        pixel[0] >= 250 && pixel[1] >= 250 && pixel[2] >= 250,
        "Transparent pixel should be white (>= 250), got: {:?}",
        pixel
    );
}

#[test]
fn test_jpeg_opaque_no_compositing() {
    let jpeg_data = create_test_rgb_jpeg();
    let result = convert_image(&jpeg_data, CodecTarget::Jpg, 85, false)
        .expect("convert_image should succeed");
    assert!(
        result.is_some(),
        "Opaque JPEG to JPEG should return Some(bytes)"
    );
    let output = result.unwrap();
    assert!(!output.is_empty(), "Output should not be empty");

    // Decode and verify pixel values are similar (lossy compression allows deviation)
    let decoded = image::load_from_memory(&output).expect("should decode output");
    let rgb = decoded.to_rgb8();
    let pixel = rgb.get_pixel(0, 0);
    // Original pixel at (0,0) was (0, 0, 128) -- allow JPEG lossy deviation
    assert!(
        pixel[2] > 100,
        "Blue channel should be roughly preserved, got: {:?}",
        pixel
    );
}

#[test]
fn test_jpeg_quality_85() {
    let png_data = create_photographic_test_image();

    let result_85 = convert_image(&png_data, CodecTarget::Jpg, 85, false)
        .expect("quality 85 should succeed")
        .expect("should return Some");
    let result_75 = convert_image(&png_data, CodecTarget::Jpg, 75, false)
        .expect("quality 75 should succeed")
        .expect("should return Some");

    // Different quality settings must produce different byte lengths
    assert_ne!(
        result_85.len(),
        result_75.len(),
        "Quality 85 and 75 must produce different output sizes (proving quality is used)"
    );
}

#[test]
fn test_png_preserves_alpha() {
    let png_data = create_test_rgba_png();
    let result = convert_image(&png_data, CodecTarget::Png, 85, false)
        .expect("convert_image should succeed");
    let output = result.expect("should return Some(bytes)");

    // Decode and check alpha is preserved
    let decoded = image::load_from_memory(&output).expect("should decode PNG output");
    let rgba = decoded.to_rgba8();

    // The fully transparent pixel at (0,1) should still have alpha=0
    let pixel = rgba.get_pixel(0, 1);
    assert_eq!(
        pixel[3], 0,
        "Alpha channel should be preserved: transparent pixel should have alpha=0, got: {}",
        pixel[3]
    );

    // The opaque pixel at (0,0) should still have alpha=255
    let pixel_opaque = rgba.get_pixel(0, 0);
    assert_eq!(
        pixel_opaque[3], 255,
        "Alpha channel should be preserved: opaque pixel should have alpha=255, got: {}",
        pixel_opaque[3]
    );
}

#[test]
fn test_webp_lossy_smaller_than_lossless() {
    let photo_data = create_photographic_test_image();

    // Lossy via convert_image
    let lossy = convert_image(&photo_data, CodecTarget::Webp, 85, false)
        .expect("lossy should succeed")
        .expect("should return Some");

    // Lossless via image crate
    let img = image::load_from_memory(&photo_data).unwrap();
    let mut lossless_buf = Cursor::new(Vec::new());
    img.write_to(&mut lossless_buf, EncodedImageFormat::WebP)
        .unwrap();
    let lossless = lossless_buf.into_inner();

    assert!(
        lossy.len() < lossless.len(),
        "Lossy WebP ({} bytes) should be smaller than lossless ({} bytes)",
        lossy.len(),
        lossless.len()
    );
}

#[test]
fn test_webp_lossy_encoding() {
    let png_data = create_test_rgba_png();
    let result = convert_image(&png_data, CodecTarget::Webp, 85, false)
        .expect("convert_image should succeed");
    let webp_bytes = result.expect("should return Some(bytes)");
    assert!(!webp_bytes.is_empty(), "WebP output should not be empty");

    // Verify the output is valid WebP by decoding it
    let decoded = image::load_from_memory(&webp_bytes);
    assert!(decoded.is_ok(), "Output should be decodable as valid WebP");
}

#[test]
fn test_unsupported_format_returns_none() {
    // SVG-like data that image crate cannot decode (unsupported format)
    let svg_data = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    let result =
        convert_image(svg_data, CodecTarget::Png, 85, false).expect("should return Ok, not Err");
    assert!(
        result.is_none(),
        "Unsupported format should return Ok(None)"
    );
}

#[test]
fn test_corrupt_data_returns_err() {
    // Partially matches a format header but is corrupt
    // PNG magic bytes followed by garbage
    let corrupt_data = b"\x89PNG\r\n\x1a\nnot an image at all!!!";
    let result = convert_image(corrupt_data, CodecTarget::Png, 85, false);
    assert!(
        result.is_err(),
        "Corrupt data should return Err, got: {:?}",
        result
    );
}

#[test]
fn test_format_matrix() {
    // Source format generators
    let sources: Vec<(&str, Vec<u8>)> = vec![
        ("JPEG", create_test_rgb_jpeg()),
        ("PNG", create_test_rgba_png()),
        ("GIF", create_test_gif()),
        ("BMP", create_test_bmp()),
        ("TIFF", create_test_tiff()),
        ("WebP", create_test_webp()),
    ];

    let targets = [
        ("Jpg", CodecTarget::Jpg),
        ("Png", CodecTarget::Png),
        ("Webp", CodecTarget::Webp),
    ];

    for (src_name, src_data) in &sources {
        for (tgt_name, tgt_format) in &targets {
            let result = convert_image(src_data, *tgt_format, 85, false);
            let output = result.unwrap_or_else(|e| {
                panic!("{} -> {}: unexpected error: {}", src_name, tgt_name, e)
            });
            let bytes =
                output.unwrap_or_else(|| panic!("{} -> {}: unexpected None", src_name, tgt_name));
            assert!(
                !bytes.is_empty(),
                "{} -> {}: output should not be empty",
                src_name,
                tgt_name
            );
        }
    }
}

#[test]
fn test_convert_returns_bytes() {
    let png_data = create_test_rgba_png();
    let result = convert_image(&png_data, CodecTarget::Png, 85, false)
        .expect("convert_image should succeed");
    let bytes = result.expect("should return Some(Vec<u8>)");
    assert!(!bytes.is_empty(), "Converted bytes should not be empty");
}

#[test]
fn conversion_policy_converts_supported_source() {
    let policy = policy(ConversionTarget::Jpg, None, false);
    let png_data = create_test_rgba_png();

    let result = policy
        .convert(&png_data, ImageFormat::Png)
        .expect("PNG to JPEG conversion should succeed");

    assert!(matches!(
        result,
        ConversionOutcome::Converted(bytes, ImageFormat::Jpg) if !bytes.is_empty()
    ));
}

#[test]
fn unsupported_sources_are_normal_outcomes() {
    let policy = policy(ConversionTarget::Png, None, false);

    for format in [ImageFormat::Svg, ImageFormat::Wmf, ImageFormat::Emf] {
        let result = policy
            .convert(&[1, 2, 3], format)
            .expect("unsupported source should not be an error");
        assert!(matches!(
            result,
            ConversionOutcome::UnsupportedSource(actual) if actual == format
        ));
    }
}

#[test]
fn decoder_unsupported_is_a_normal_outcome() {
    let policy = policy(ConversionTarget::Jpg, None, false);
    let fake_svg_data = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";

    let result = policy
        .convert(fake_svg_data, ImageFormat::Png)
        .expect("decoder-unsupported bytes should not be an error");

    assert!(matches!(
        result,
        ConversionOutcome::UnsupportedSource(ImageFormat::Png)
    ));
}

#[test]
fn converted_outcome_uses_target_image_format() {
    let bmp_data = create_test_bmp();
    let png_policy = policy(ConversionTarget::Png, None, false);
    let png_result = png_policy
        .convert(&bmp_data, ImageFormat::Bmp)
        .expect("BMP to PNG conversion should succeed");
    assert!(matches!(
        png_result,
        ConversionOutcome::Converted(_, ImageFormat::Png)
    ));

    let png_data = create_test_rgba_png();
    let webp_policy = policy(ConversionTarget::Webp, None, false);
    let webp_result = webp_policy
        .convert(&png_data, ImageFormat::Png)
        .expect("PNG to WebP conversion should succeed");
    assert!(matches!(
        webp_result,
        ConversionOutcome::Converted(_, ImageFormat::Webp)
    ));
}

#[test]
fn corrupt_data_is_an_error_when_conversion_runs() {
    let policy = policy(ConversionTarget::Jpg, None, false);
    let corrupt_data = b"\x89PNG\r\n\x1a\nnot an image at all!!!";

    let result = policy.convert(corrupt_data, ImageFormat::Png);

    assert!(result.is_err(), "corrupt data should return an error");
}

#[test]
fn explicit_quality_reencodes_matching_jpeg() {
    let policy = policy(ConversionTarget::Jpg, Some(85), false);
    let jpeg_data = create_test_rgb_jpeg();

    let result = policy
        .convert(&jpeg_data, ImageFormat::Jpg)
        .expect("explicit JPEG quality should re-encode matching JPEG");

    assert!(matches!(
        result,
        ConversionOutcome::Converted(_, ImageFormat::Jpg)
    ));
}

#[test]
fn implicit_and_explicit_default_quality_encode_nonmatching_source_identically() {
    let png_data = create_test_rgba_png();
    let implicit = policy(ConversionTarget::Jpg, None, false)
        .convert(&png_data, ImageFormat::Png)
        .expect("implicit JPEG quality should convert PNG");
    let explicit = policy(ConversionTarget::Jpg, Some(85), false)
        .convert(&png_data, ImageFormat::Png)
        .expect("explicit default JPEG quality should convert PNG");

    let ConversionOutcome::Converted(implicit_bytes, ImageFormat::Jpg) = implicit else {
        panic!("expected implicit JPEG conversion");
    };
    let ConversionOutcome::Converted(explicit_bytes, ImageFormat::Jpg) = explicit else {
        panic!("expected explicit JPEG conversion");
    };
    assert_eq!(implicit_bytes, explicit_bytes);
}

#[test]
fn matching_png_is_always_preserved() {
    let policy = policy(ConversionTarget::Png, None, false);
    let png_data = create_test_rgba_png();

    let result = policy
        .convert(&png_data, ImageFormat::Png)
        .expect("matching PNG should be preserved");

    assert!(matches!(result, ConversionOutcome::PreservedMatchingSource));
}

#[test]
fn matching_webp_preservation_depends_on_explicit_encoding() {
    let webp_data = create_test_webp();
    let default_policy = policy(ConversionTarget::Webp, None, false);
    assert!(matches!(
        default_policy
            .convert(&webp_data, ImageFormat::Webp)
            .expect("default matching WebP should be preserved"),
        ConversionOutcome::PreservedMatchingSource
    ));

    let quality_policy = policy(ConversionTarget::Webp, Some(85), false);
    assert!(matches!(
        quality_policy
            .convert(&webp_data, ImageFormat::Webp)
            .expect("explicit WebP quality should re-encode matching WebP"),
        ConversionOutcome::Converted(_, ImageFormat::Webp)
    ));

    let lossless_policy = policy(ConversionTarget::Webp, None, true);
    assert!(matches!(
        lossless_policy
            .convert(&webp_data, ImageFormat::Webp)
            .expect("explicit lossless WebP should re-encode matching WebP"),
        ConversionOutcome::Converted(_, ImageFormat::Webp)
    ));
}

#[test]
fn conversion_policy_rejects_invalid_target_settings() {
    assert_eq!(
        ConversionPolicy::try_from(ConversionRequest {
            target: ConversionTarget::Png,
            quality: Some(85),
            lossless: false,
        }),
        Err(ConversionPolicyError::QualityUnsupportedForPng)
    );
    assert_eq!(
        ConversionPolicy::try_from(ConversionRequest {
            target: ConversionTarget::Jpg,
            quality: None,
            lossless: true,
        }),
        Err(ConversionPolicyError::LosslessUnsupportedForTarget {
            target: ConversionTarget::Jpg,
        })
    );
    assert_eq!(
        ConversionPolicy::try_from(ConversionRequest {
            target: ConversionTarget::Webp,
            quality: Some(85),
            lossless: true,
        }),
        Err(ConversionPolicyError::LosslessConflictsWithQuality)
    );
    assert_eq!(
        ConversionPolicy::try_from(ConversionRequest {
            target: ConversionTarget::Jpg,
            quality: Some(0),
            lossless: false,
        }),
        Err(ConversionPolicyError::QualityOutOfRange { quality: 0 })
    );
}

#[test]
fn lossless_webp_conversion_produces_decodable_output() {
    let policy = policy(ConversionTarget::Webp, None, true);
    let photo_data = create_photographic_test_image();

    let result = policy
        .convert(&photo_data, ImageFormat::Png)
        .expect("lossless WebP conversion should succeed");
    let ConversionOutcome::Converted(bytes, ImageFormat::Webp) = result else {
        panic!("expected converted WebP bytes");
    };

    assert!(image::load_from_memory(&bytes).is_ok());
}

/// Constructs a valid policy for tests through the public conversion seam.
fn policy(target: ConversionTarget, quality: Option<u8>, lossless: bool) -> ConversionPolicy {
    ConversionPolicy::try_from(ConversionRequest {
        target,
        quality,
        lossless,
    })
    .expect("test conversion request should be valid")
}
