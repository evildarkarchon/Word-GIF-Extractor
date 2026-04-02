//! Image format conversion module
//!
//! Provides byte-in, byte-out image format conversion. Decodes source image
//! bytes using the `image` crate and re-encodes to a target format (JPEG, PNG,
//! or WebP). Handles alpha compositing for JPEG conversion and provides lossy
//! WebP encoding via the `webp` crate.
//!
//! Note: Animated GIFs are decoded as their first frame only.

use anyhow::Result;

/// Target format for image conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// JPEG output (lossy, quality 1-100)
    Jpg,
    /// PNG output (lossless, preserves transparency)
    Png,
    /// WebP output (lossy by default, lossless optional)
    Webp,
}

/// Checks whether a source image extension can be decoded for conversion.
///
/// Returns `true` for formats the `image` crate can decode: jpg, jpeg, png,
/// gif, bmp, tiff, tif, webp, ico. Returns `false` for SVG, WMF, EMF, and
/// other unsupported formats.
pub fn can_convert(_extension: &str) -> bool {
    todo!()
}

/// Converts image bytes from one format to another.
///
/// Takes raw image bytes, decodes them, and re-encodes to the target format.
/// Returns `Ok(Some(bytes))` on success, `Ok(None)` for unsupported source
/// formats, or `Err` for corrupt/undecodable data.
///
/// The `quality` parameter controls JPEG and lossy WebP output (1-100).
/// It is ignored for PNG (lossless).
pub fn convert_image(_data: &[u8], _format: OutputFormat, _quality: u8) -> Result<Option<Vec<u8>>> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
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
        dynamic.write_to(&mut buf, ImageFormat::Png).unwrap();
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
        dynamic.write_to(&mut buf, ImageFormat::Bmp).unwrap();
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
        dynamic.write_to(&mut buf, ImageFormat::Gif).unwrap();
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
        dynamic.write_to(&mut buf, ImageFormat::Tiff).unwrap();
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
        dynamic.write_to(&mut buf, ImageFormat::WebP).unwrap();
        buf.into_inner()
    }

    /// Creates a 64x64 photographic-style test image with smooth gradients
    fn create_photographic_test_image() -> Vec<u8> {
        use image::RgbImage;
        let mut img = RgbImage::new(64, 64);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgb([
                (x * 4) as u8,
                (y * 4) as u8,
                ((x + y) * 2) as u8,
            ]);
        }
        let dynamic = DynamicImage::ImageRgb8(img);
        let mut buf = Cursor::new(Vec::new());
        dynamic.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn test_can_convert() {
        // Supported formats
        assert!(can_convert("jpg"));
        assert!(can_convert("jpeg"));
        assert!(can_convert("png"));
        assert!(can_convert("gif"));
        assert!(can_convert("bmp"));
        assert!(can_convert("tiff"));
        assert!(can_convert("tif"));
        assert!(can_convert("webp"));
        assert!(can_convert("ico"));

        // Unsupported formats
        assert!(!can_convert("svg"));
        assert!(!can_convert("wmf"));
        assert!(!can_convert("emf"));
        assert!(!can_convert("pdf"));
    }

    #[test]
    fn test_can_convert_case_insensitive() {
        assert!(can_convert("JPG"));
        assert!(can_convert("PNG"));
        assert!(can_convert("Jpeg"));
        assert!(can_convert("TIFF"));
    }

    #[test]
    fn test_jpeg_alpha_compositing_white_background() {
        let png_data = create_test_rgba_png();
        let result = convert_image(&png_data, OutputFormat::Jpg, 85)
            .expect("convert_image should succeed");
        let jpeg_bytes = result.expect("should return Some(bytes)");

        // Decode the JPEG result
        let decoded = image::load_from_memory(&jpeg_bytes)
            .expect("should decode JPEG output");
        let rgb = decoded.to_rgb8();

        // Check the fully transparent pixel (0,1) -- should be white, not black
        let pixel = rgb.get_pixel(0, 1);
        assert!(
            pixel[0] >= 250 && pixel[1] >= 250 && pixel[2] >= 250,
            "Transparent pixel should be white (>= 250), got: {:?}",
            pixel
        );
    }

    #[test]
    fn test_jpeg_opaque_no_compositing() {
        let jpeg_data = create_test_rgb_jpeg();
        let result = convert_image(&jpeg_data, OutputFormat::Jpg, 85)
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

        let result_85 = convert_image(&png_data, OutputFormat::Jpg, 85)
            .expect("quality 85 should succeed")
            .expect("should return Some");
        let result_75 = convert_image(&png_data, OutputFormat::Jpg, 75)
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
        let result = convert_image(&png_data, OutputFormat::Png, 85)
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
        let lossy = convert_image(&photo_data, OutputFormat::Webp, 85)
            .expect("lossy should succeed")
            .expect("should return Some");

        // Lossless via image crate
        let img = image::load_from_memory(&photo_data).unwrap();
        let mut lossless_buf = Cursor::new(Vec::new());
        img.write_to(&mut lossless_buf, ImageFormat::WebP).unwrap();
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
        let result = convert_image(&png_data, OutputFormat::Webp, 85)
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
        let result = convert_image(svg_data, OutputFormat::Png, 85)
            .expect("should return Ok, not Err");
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
        let result = convert_image(corrupt_data, OutputFormat::Png, 85);
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
            ("Jpg", OutputFormat::Jpg),
            ("Png", OutputFormat::Png),
            ("Webp", OutputFormat::Webp),
        ];

        for (src_name, src_data) in &sources {
            for (tgt_name, tgt_format) in &targets {
                let result = convert_image(src_data, *tgt_format, 85);
                let output = result
                    .unwrap_or_else(|e| panic!("{} -> {}: unexpected error: {}", src_name, tgt_name, e));
                let bytes = output
                    .unwrap_or_else(|| panic!("{} -> {}: unexpected None", src_name, tgt_name));
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
        let result = convert_image(&png_data, OutputFormat::Png, 85)
            .expect("convert_image should succeed");
        let bytes = result.expect("should return Some(Vec<u8>)");
        assert!(!bytes.is_empty(), "Converted bytes should not be empty");
    }
}
