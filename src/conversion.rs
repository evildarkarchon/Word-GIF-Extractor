//! Image format conversion module
//!
//! Provides byte-in, byte-out image format conversion. Decodes source image
//! bytes using the `image` crate and re-encodes to a target format (JPEG, PNG,
//! or WebP). Handles alpha compositing for JPEG conversion and provides lossy
//! WebP encoding via the `webp` crate.
//!
//! Note: Animated GIFs are decoded as their first frame only.

use anyhow::{Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageError, Rgb, RgbImage, Rgba};
use std::fmt;

use crate::image_format::ImageFormat;

const DEFAULT_QUALITY: u8 = 85;

/// A format that the conversion module can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionTarget {
    /// JPEG output.
    Jpg,
    /// PNG output.
    Png,
    /// WebP output.
    Webp,
}

/// Raw, CLI-independent facts used to construct a valid Conversion policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionRequest {
    /// Requested output target.
    pub target: ConversionTarget,
    /// Explicit JPEG or lossy WebP quality, when supplied.
    pub quality: Option<u8>,
    /// Whether WebP output should use lossless encoding.
    pub lossless: bool,
}

/// Why a raw conversion request cannot become a valid Conversion policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionPolicyError {
    /// Quality must be within the encoder-supported range.
    QualityOutOfRange { quality: u8 },
    /// PNG has no configurable quality setting.
    QualityUnsupportedForPng,
    /// Lossless encoding is only available for WebP.
    LosslessUnsupportedForTarget { target: ConversionTarget },
    /// Lossless WebP and lossy WebP quality are mutually exclusive.
    LosslessConflictsWithQuality,
}

impl fmt::Display for ConversionPolicyError {
    /// Formats a CLI-independent explanation of the invalid request.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConversionPolicyError::QualityOutOfRange { quality } => {
                write!(
                    formatter,
                    "quality {quality} is outside the valid range 1..=100"
                )
            }
            ConversionPolicyError::QualityUnsupportedForPng => {
                formatter.write_str("quality is not supported for PNG output")
            }
            ConversionPolicyError::LosslessUnsupportedForTarget { target } => {
                write!(
                    formatter,
                    "lossless encoding is not supported for {target:?} output"
                )
            }
            ConversionPolicyError::LosslessConflictsWithQuality => {
                formatter.write_str("lossless WebP cannot also specify lossy quality")
            }
        }
    }
}

impl std::error::Error for ConversionPolicyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversionMode {
    Jpeg {
        quality: u8,
        // Explicit quality 85 must re-encode matching JPEGs, while an omitted
        // quality also uses 85 for other sources but preserves matching bytes.
        reencode_matching: bool,
    },
    Png,
    WebpLossy {
        quality: u8,
        // The same explicitness rule applies to lossy WebP quality.
        reencode_matching: bool,
    },
    WebpLossless,
}

/// A validated target encoding and its matching-source preservation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionPolicy {
    mode: ConversionMode,
}

/// Observable result of applying a Conversion policy to one image.
#[derive(Debug)]
pub enum ConversionOutcome {
    /// New bytes were encoded in the requested target format.
    Converted(Vec<u8>, ImageFormat),
    /// Original bytes should be preserved because source and target match and no
    /// encoding setting was explicitly requested.
    PreservedMatchingSource,
    /// The source Image format is not supported by the available decoders.
    UnsupportedSource(ImageFormat),
}

impl TryFrom<ConversionRequest> for ConversionPolicy {
    type Error = ConversionPolicyError;

    /// Validates a raw request and constructs a target-specific Conversion policy.
    fn try_from(request: ConversionRequest) -> std::result::Result<Self, Self::Error> {
        if let Some(quality) = request.quality
            && !(1..=100).contains(&quality)
        {
            return Err(ConversionPolicyError::QualityOutOfRange { quality });
        }

        let mode = match request.target {
            ConversionTarget::Jpg => {
                if request.lossless {
                    return Err(ConversionPolicyError::LosslessUnsupportedForTarget {
                        target: request.target,
                    });
                }
                ConversionMode::Jpeg {
                    quality: request.quality.unwrap_or(DEFAULT_QUALITY),
                    reencode_matching: request.quality.is_some(),
                }
            }
            ConversionTarget::Png => {
                if request.quality.is_some() {
                    return Err(ConversionPolicyError::QualityUnsupportedForPng);
                }
                if request.lossless {
                    return Err(ConversionPolicyError::LosslessUnsupportedForTarget {
                        target: request.target,
                    });
                }
                ConversionMode::Png
            }
            ConversionTarget::Webp => {
                if request.lossless && request.quality.is_some() {
                    return Err(ConversionPolicyError::LosslessConflictsWithQuality);
                }
                if request.lossless {
                    ConversionMode::WebpLossless
                } else {
                    ConversionMode::WebpLossy {
                        quality: request.quality.unwrap_or(DEFAULT_QUALITY),
                        reencode_matching: request.quality.is_some(),
                    }
                }
            }
        };

        Ok(Self { mode })
    }
}

impl ConversionPolicy {
    /// Applies this policy to one image without deciding write fallback behavior.
    ///
    /// Unsupported sources are returned as a normal outcome. Decoder or encoder
    /// failures are returned as errors for the Image write pipeline to interpret.
    pub fn convert(&self, data: &[u8], source_format: ImageFormat) -> Result<ConversionOutcome> {
        if source_format == self.target_format() && self.preserves_matching_source() {
            return Ok(ConversionOutcome::PreservedMatchingSource);
        }

        if !source_format.can_convert() {
            return Ok(ConversionOutcome::UnsupportedSource(source_format));
        }

        let (format, quality, lossless) = match self.mode {
            ConversionMode::Jpeg { quality, .. } => (CodecTarget::Jpg, quality, false),
            ConversionMode::Png => (CodecTarget::Png, DEFAULT_QUALITY, false),
            ConversionMode::WebpLossy { quality, .. } => (CodecTarget::Webp, quality, false),
            ConversionMode::WebpLossless => (CodecTarget::Webp, DEFAULT_QUALITY, true),
        };

        match convert_image(data, format, quality, lossless)? {
            Some(converted_bytes) => Ok(ConversionOutcome::Converted(
                converted_bytes,
                self.target_format(),
            )),
            None => Ok(ConversionOutcome::UnsupportedSource(source_format)),
        }
    }

    /// Returns the canonical Image format produced by this policy.
    fn target_format(&self) -> ImageFormat {
        match self.mode {
            ConversionMode::Jpeg { .. } => ImageFormat::Jpg,
            ConversionMode::Png => ImageFormat::Png,
            ConversionMode::WebpLossy { .. } | ConversionMode::WebpLossless => ImageFormat::Webp,
        }
    }

    /// Returns whether matching source bytes should bypass codec work.
    fn preserves_matching_source(&self) -> bool {
        match self.mode {
            ConversionMode::Jpeg {
                reencode_matching, ..
            }
            | ConversionMode::WebpLossy {
                reencode_matching, ..
            } => !reencode_matching,
            ConversionMode::Png => true,
            ConversionMode::WebpLossless => false,
        }
    }
}

/// Target format used by the private codec implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodecTarget {
    /// JPEG output (lossy, quality 1-100)
    Jpg,
    /// PNG output (lossless, preserves transparency)
    Png,
    /// WebP output (lossy by default, lossless optional)
    Webp,
}

/// Converts image bytes from one format to another.
///
/// Takes raw image bytes, decodes them, and re-encodes to the target format.
/// Returns `Ok(Some(bytes))` on success, `Ok(None)` for unsupported source
/// formats, or `Err` for corrupt/undecodable data.
///
/// The `quality` parameter controls JPEG and lossy WebP output (1-100).
/// It is ignored for PNG (lossless).
fn convert_image(
    data: &[u8],
    format: CodecTarget,
    quality: u8,
    lossless: bool,
) -> Result<Option<Vec<u8>>> {
    // Stage 1 - Decode: detect format from magic bytes and decode
    let img = match image::load_from_memory(data) {
        Ok(img) => img,
        Err(ImageError::Unsupported(_)) => return Ok(None),
        Err(e) => return Err(e).context("Failed to decode image"),
    };

    // Stage 2 & 3 - Alpha compositing (JPEG only) and Encode
    let encoded = match format {
        CodecTarget::Jpg => {
            let img_for_encode = if img.color().has_alpha() {
                composite_on_white(&img)
            } else {
                img
            };
            encode_jpeg(&img_for_encode, quality)?
        }
        CodecTarget::Png => encode_png(&img)?,
        CodecTarget::Webp => {
            if lossless {
                encode_webp_lossless(&img)?
            } else {
                encode_webp_lossy(&img, quality)?
            }
        }
    };

    Ok(Some(encoded))
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

/// Encodes a DynamicImage as lossless WebP using the image crate's built-in encoder.
fn encode_webp_lossless(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut buf);
    img.write_with_encoder(encoder)
        .context("Failed to encode lossless WebP")?;
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
mod tests;
