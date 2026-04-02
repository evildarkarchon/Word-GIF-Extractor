//! Image format conversion module
//!
//! Provides byte-in, byte-out image format conversion. Decodes source image
//! bytes using the `image` crate and re-encodes to a target format (JPEG, PNG,
//! or WebP). Handles alpha compositing for JPEG conversion and provides lossy
//! WebP encoding via the `webp` crate.
//!
//! Note: Animated GIFs are decoded as their first frame only.

use anyhow::{Context, Result};
use clap::ValueEnum;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageError, Rgb, RgbImage, Rgba};

/// Target format for image conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// JPEG output (lossy, quality 1-100)
    Jpg,
    /// PNG output (lossless, preserves transparency)
    Png,
    /// WebP output (lossy by default, lossless optional)
    Webp,
}

/// Result of attempting to convert an image to a target format.
///
/// Used by callers to determine whether to write converted bytes or
/// extract the original image as-is. The extension string in each
/// variant is ready to pass directly to `get_unique_output_path()`.
#[derive(Debug)]
pub enum ConversionResult {
    /// Image was successfully converted. Contains the converted bytes
    /// and the target format extension (e.g., "png", "jpg", "webp").
    Converted(Vec<u8>, String),
    /// Image was skipped (unsupported source format). Contains the
    /// original file extension to preserve in the output filename.
    Skipped(String),
}

impl OutputFormat {
    /// Returns the file extension for this output format (without leading dot).
    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Jpg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }
}

/// Checks whether a source image extension can be decoded for conversion.
///
/// Returns `true` for formats the `image` crate can decode: jpg, jpeg, png,
/// gif, bmp, tiff, tif, webp, ico. Returns `false` for SVG, WMF, EMF, and
/// other unsupported formats.
pub fn can_convert(extension: &str) -> bool {
    matches!(
        extension.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "tif" | "webp" | "ico"
    )
}

/// Converts image bytes from one format to another.
///
/// Takes raw image bytes, decodes them, and re-encodes to the target format.
/// Returns `Ok(Some(bytes))` on success, `Ok(None)` for unsupported source
/// formats, or `Err` for corrupt/undecodable data.
///
/// The `quality` parameter controls JPEG and lossy WebP output (1-100).
/// It is ignored for PNG (lossless).
pub fn convert_image(data: &[u8], format: OutputFormat, quality: u8) -> Result<Option<Vec<u8>>> {
    // Stage 1 - Decode: detect format from magic bytes and decode
    let img = match image::load_from_memory(data) {
        Ok(img) => img,
        Err(ImageError::Unsupported(_)) => return Ok(None),
        Err(e) => return Err(e).context("Failed to decode image"),
    };

    // Stage 2 & 3 - Alpha compositing (JPEG only) and Encode
    let encoded = match format {
        OutputFormat::Jpg => {
            let img_for_encode = if img.color().has_alpha() {
                composite_on_white(&img)
            } else {
                img
            };
            encode_jpeg(&img_for_encode, quality)?
        }
        OutputFormat::Png => encode_png(&img)?,
        OutputFormat::Webp => encode_webp_lossy(&img, quality)?,
    };

    Ok(Some(encoded))
}

/// Attempts to convert image data to the target format.
///
/// This is the primary conversion API for callers. It checks whether the
/// source format is convertible, attempts conversion, and returns a
/// `ConversionResult` indicating whether the image was converted or skipped.
///
/// Returns `Skipped` when:
/// - `can_convert()` returns false for the source extension (pre-check)
/// - `convert_image()` returns `Ok(None)` (unsupported at decode time)
///
/// Returns `Err` when:
/// - The image data is corrupt or undecodable
pub fn try_convert(
    data: &[u8],
    source_ext: &str,
    format: OutputFormat,
    quality: u8,
) -> Result<ConversionResult> {
    // Fast path: extension not in decodable set
    if !can_convert(source_ext) {
        return Ok(ConversionResult::Skipped(source_ext.to_lowercase()));
    }

    // Attempt conversion
    match convert_image(data, format, quality)? {
        Some(converted_bytes) => Ok(ConversionResult::Converted(
            converted_bytes,
            format.extension().to_string(),
        )),
        None => {
            // Unsupported format detected at decode time (magic bytes don't match)
            Ok(ConversionResult::Skipped(source_ext.to_lowercase()))
        }
    }
}

/// Composites an RGBA image onto a white background, producing an RGB image.
///
/// For each pixel, blends the source color against white (255, 255, 255)
/// using the alpha channel value. Fully transparent pixels become white.
fn composite_on_white(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = RgbImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let Rgba([r, g, b, a]) = *pixel;
        let alpha = a as f32 / 255.0;
        let inv_alpha = 1.0 - alpha;

        // Blend against white (255, 255, 255)
        let out_r = (r as f32 * alpha + 255.0 * inv_alpha) as u8;
        let out_g = (g as f32 * alpha + 255.0 * inv_alpha) as u8;
        let out_b = (b as f32 * alpha + 255.0 * inv_alpha) as u8;

        rgb.put_pixel(x, y, Rgb([out_r, out_g, out_b]));
    }

    DynamicImage::ImageRgb8(rgb)
}

/// Encodes a DynamicImage as JPEG with the specified quality.
///
/// Uses `JpegEncoder::new_with_quality` to control output quality (1-100).
fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    img.write_with_encoder(encoder)
        .context("Failed to encode JPEG")?;
    Ok(buf)
}

/// Encodes a DynamicImage as PNG (lossless, preserves alpha channel).
fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    img.write_with_encoder(encoder)
        .context("Failed to encode PNG")?;
    Ok(buf)
}

/// Encodes a DynamicImage as lossy WebP using the `webp` crate.
///
/// Uses `webp::Encoder::from_image` for direct `DynamicImage` integration.
/// The quality parameter controls lossy compression (1-100, where 100 is highest quality).
fn encode_webp_lossy(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let encoder = webp::Encoder::from_image(img)
        .map_err(|e| anyhow::anyhow!("WebP encoder creation failed: {}", e))?;
    let webp_data = encoder.encode(quality as f32);
    Ok(webp_data.to_vec())
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
        dynamic.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn test_jpeg_alpha_compositing_white_background() {
        let png_data = create_alpha_test_png();
        let result =
            convert_image(&png_data, OutputFormat::Jpg, 85).expect("convert_image should succeed");
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
        let result =
            convert_image(&jpeg_data, OutputFormat::Jpg, 85).expect("convert_image should succeed");
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
        let result =
            convert_image(&png_data, OutputFormat::Png, 85).expect("convert_image should succeed");
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
        let result =
            convert_image(&png_data, OutputFormat::Webp, 85).expect("convert_image should succeed");
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
            convert_image(svg_data, OutputFormat::Png, 85).expect("should return Ok, not Err");
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
                let output = result.unwrap_or_else(|e| {
                    panic!("{} -> {}: unexpected error: {}", src_name, tgt_name, e)
                });
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
        let result =
            convert_image(&png_data, OutputFormat::Png, 85).expect("convert_image should succeed");
        let bytes = result.expect("should return Some(Vec<u8>)");
        assert!(!bytes.is_empty(), "Converted bytes should not be empty");
    }

    #[test]
    fn test_output_format_extension() {
        assert_eq!(OutputFormat::Jpg.extension(), "jpg");
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Webp.extension(), "webp");
    }

    #[test]
    fn test_try_convert_supported() {
        let png_data = create_test_rgba_png();
        let result = try_convert(&png_data, "png", OutputFormat::Jpg, 85)
            .expect("try_convert should succeed");
        match result {
            ConversionResult::Converted(bytes, ext) => {
                assert!(!bytes.is_empty(), "Converted bytes should not be empty");
                assert_eq!(ext, "jpg", "Extension should be target format");
            }
            ConversionResult::Skipped(_) => {
                panic!("Expected Converted, got Skipped");
            }
        }
    }

    #[test]
    fn test_try_convert_unsupported_extension() {
        // SVG
        let result = try_convert(&[1, 2, 3], "svg", OutputFormat::Png, 85)
            .expect("try_convert should succeed for unsupported extension");
        match result {
            ConversionResult::Skipped(ext) => {
                assert_eq!(ext, "svg", "Skipped extension should be 'svg'");
            }
            ConversionResult::Converted(_, _) => {
                panic!("Expected Skipped for SVG, got Converted");
            }
        }

        // WMF
        let result = try_convert(&[1, 2, 3], "wmf", OutputFormat::Png, 85)
            .expect("try_convert should succeed for unsupported extension");
        match result {
            ConversionResult::Skipped(ext) => {
                assert_eq!(ext, "wmf", "Skipped extension should be 'wmf'");
            }
            ConversionResult::Converted(_, _) => {
                panic!("Expected Skipped for WMF, got Converted");
            }
        }

        // EMF
        let result = try_convert(&[1, 2, 3], "emf", OutputFormat::Png, 85)
            .expect("try_convert should succeed for unsupported extension");
        match result {
            ConversionResult::Skipped(ext) => {
                assert_eq!(ext, "emf", "Skipped extension should be 'emf'");
            }
            ConversionResult::Converted(_, _) => {
                panic!("Expected Skipped for EMF, got Converted");
            }
        }
    }

    #[test]
    fn test_try_convert_unsupported_at_decode() {
        // Extension says "png" but data is actually SVG -- convert_image returns Ok(None)
        let fake_svg_data =
            b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        let result = try_convert(fake_svg_data, "png", OutputFormat::Jpg, 85)
            .expect("try_convert should succeed (Skipped, not Err)");
        match result {
            ConversionResult::Skipped(ext) => {
                assert_eq!(ext, "png", "Skipped extension should preserve source ext");
            }
            ConversionResult::Converted(_, _) => {
                panic!("Expected Skipped for undecodable data, got Converted");
            }
        }
    }

    #[test]
    fn test_try_convert_correct_extension() {
        // BMP -> PNG: extension should be "png", not "bmp" or "bmp.png"
        let bmp_data = create_test_bmp();
        let result = try_convert(&bmp_data, "bmp", OutputFormat::Png, 85)
            .expect("try_convert should succeed");
        match result {
            ConversionResult::Converted(_, ext) => {
                assert_eq!(ext, "png", "Extension should be target format 'png'");
            }
            ConversionResult::Skipped(_) => {
                panic!("Expected Converted for BMP->PNG, got Skipped");
            }
        }

        // PNG -> WebP: extension should be "webp"
        let png_data = create_test_rgba_png();
        let result = try_convert(&png_data, "png", OutputFormat::Webp, 85)
            .expect("try_convert should succeed");
        match result {
            ConversionResult::Converted(_, ext) => {
                assert_eq!(ext, "webp", "Extension should be target format 'webp'");
            }
            ConversionResult::Skipped(_) => {
                panic!("Expected Converted for PNG->WebP, got Skipped");
            }
        }
    }

    #[test]
    fn test_try_convert_corrupt_data() {
        // Corrupt PNG: has PNG magic bytes but is invalid
        let corrupt_data = b"\x89PNG\r\n\x1a\nnot an image at all!!!";
        let result = try_convert(corrupt_data, "png", OutputFormat::Png, 85);
        assert!(
            result.is_err(),
            "Corrupt data should return Err, got: {:?}",
            result
        );
    }

    #[test]
    fn test_try_convert_case_normalization() {
        // Uppercase "SVG" should become lowercase "svg" in Skipped
        let result = try_convert(&[1, 2, 3], "SVG", OutputFormat::Png, 85)
            .expect("try_convert should succeed for unsupported extension");
        match result {
            ConversionResult::Skipped(ext) => {
                assert_eq!(ext, "svg", "Extension should be lowercase 'svg', not 'SVG'");
            }
            ConversionResult::Converted(_, _) => {
                panic!("Expected Skipped for SVG, got Converted");
            }
        }

        // Uppercase "WMF" should become lowercase "wmf" in Skipped
        let result = try_convert(&[1, 2, 3], "WMF", OutputFormat::Png, 85)
            .expect("try_convert should succeed for unsupported extension");
        match result {
            ConversionResult::Skipped(ext) => {
                assert_eq!(ext, "wmf", "Extension should be lowercase 'wmf', not 'WMF'");
            }
            ConversionResult::Converted(_, _) => {
                panic!("Expected Skipped for WMF, got Converted");
            }
        }
    }
}
