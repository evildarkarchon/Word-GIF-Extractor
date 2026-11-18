//! Image format conversion module
//!
//! Provides byte-in, byte-out image format conversion. Decodes source image
//! bytes using the `image` crate and re-encodes to a target format (JPEG, PNG,
//! or WebP). Handles alpha compositing for JPEG conversion and provides lossy
//! WebP encoding via the `webp` crate.
//!
//! Every target is encoded from one still image, so converting an animated GIF would
//! keep only its first frame. An animated GIF is therefore never converted: it is
//! reported as [`ConversionOutcome::AnimatedSource`] and kept in its original bytes
//! (ADR-0034).

use anyhow::{Context, Result};
use image::codecs::gif::GifDecoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageError, Limits, RgbImage};
use std::fmt;
use std::io::Cursor;

use crate::image_format::ImageFormat;

const DEFAULT_QUALITY: u8 = 85;

/// A format that the conversion module can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversionTarget {
    /// JPEG output.
    Jpg,
    /// PNG output.
    Png,
    /// WebP output.
    Webp,
}

/// Raw, CLI-independent facts used to construct a valid Conversion policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConversionRequest {
    /// Requested output target.
    pub target: ConversionTarget,
    /// Explicit JPEG or lossy WebP quality, when supplied.
    pub quality: Option<u8>,
    /// Whether WebP output should use lossless encoding.
    pub lossless: bool,
}

/// Why a raw conversion request cannot become a valid Conversion policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConversionPolicyError {
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
pub(crate) struct ConversionPolicy {
    mode: ConversionMode,
}

/// Observable result of applying a Conversion policy to one image.
#[derive(Debug)]
pub(crate) enum ConversionOutcome {
    /// New bytes were encoded in the requested target format.
    Converted(Vec<u8>, ImageFormat),
    /// Original bytes should be preserved because source and target match and no
    /// encoding setting was explicitly requested.
    PreservedMatchingSource,
    /// The source Image format is not supported by the available decoders.
    UnsupportedSource(ImageFormat),
    /// The source is animated, and converting it would keep only its first frame,
    /// so its original bytes should be kept. Only a GIF is recognized as animated.
    AnimatedSource(ImageFormat),
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
    /// Unsupported and animated sources are returned as normal outcomes. Decoder or
    /// encoder failures are returned as errors for the Image write pipeline to
    /// interpret.
    pub(crate) fn convert(
        &self,
        data: &[u8],
        source_format: ImageFormat,
    ) -> Result<ConversionOutcome> {
        if let Some(outcome) = self.outcome_without_decoding(source_format) {
            return Ok(outcome);
        }

        // Animation is a property of the bytes, not the format, so it can only be
        // asked here, after `outcome_without_decoding` has sent this image down the
        // buffered path. No target matches GIF, so a GIF always reaches this point.
        if source_format == ImageFormat::Gif && is_animated_gif(data) {
            return Ok(ConversionOutcome::AnimatedSource(source_format));
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

    /// Decides from the source Image format alone whether this policy keeps the
    /// original bytes, before any byte is decoded.
    ///
    /// Returns [`ConversionOutcome::PreservedMatchingSource`] for a source that
    /// already matches the target and should be preserved, and
    /// [`ConversionOutcome::UnsupportedSource`] for a format no decoder takes, in
    /// that order of precedence. Returns `None` when the outcome depends on the
    /// bytes, which is the only case [`Self::convert`] decodes. Never returns
    /// [`ConversionOutcome::Converted`]. The Image write pipeline asks this first
    /// so that an image whose bytes are kept can be streamed rather than buffered
    /// (ADR-0030); `convert` asks it too, so the two cannot disagree.
    pub(crate) fn outcome_without_decoding(
        &self,
        source_format: ImageFormat,
    ) -> Option<ConversionOutcome> {
        if source_format == self.target_format() && self.preserves_matching_source() {
            return Some(ConversionOutcome::PreservedMatchingSource);
        }

        if !source_format.can_convert() {
            return Some(ConversionOutcome::UnsupportedSource(source_format));
        }

        None
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

/// Returns whether `data` is a GIF that holds more than one frame.
///
/// At most two frames are decoded, because the only question is whether a second one
/// exists. A GIF that cannot be read that far counts as still: a damaged first frame
/// then fails in the normal decode with its own error, and a damaged second frame
/// leaves a first frame that converts exactly as it did before. Only a frame that
/// decodes makes the GIF animated, so a still GIF missing its trailer is not one.
fn is_animated_gif(data: &[u8]) -> bool {
    let Ok(mut decoder) = GifDecoder::new(Cursor::new(data)) else {
        return false;
    };
    // `GifDecoder::new` starts with no allocation limit, while the normal decode goes
    // through `load_from_memory`, which applies the default one. Applying it here too
    // keeps this check from allocating what the conversion itself would refuse.
    if decoder.set_limits(Limits::default()).is_err() {
        return false;
    }
    let mut frames = decoder.into_frames();
    let mut next_frame_decodes = || frames.next().is_some_and(|frame| frame.is_ok());
    next_frame_decodes() && next_frame_decodes()
}

/// Composites an RGBA image onto a white background, producing an RGB image.
///
/// For each pixel, blends the source color against white (255, 255, 255)
/// using the alpha channel value, rounding each channel to the nearest level.
/// Fully transparent pixels become white and opaque pixels keep their color.
fn composite_on_white(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = RgbImage::new(width, height);

    // Walks both raw buffers in step rather than through `put_pixel`, which
    // bounds-checks every coordinate. Both buffers hold exactly `width * height`
    // pixels, so neither has a remainder to ignore.
    let (out_pixels, _) = rgb.as_chunks_mut::<3>();
    let (source_pixels, _) = rgba.as_chunks::<4>();
    for (out, &[r, g, b, a]) in out_pixels.iter_mut().zip(source_pixels) {
        let alpha = u16::from(a);
        *out = [r, g, b].map(|channel| blend_on_white(u16::from(channel), alpha));
    }

    DynamicImage::ImageRgb8(rgb)
}

/// Blends one channel value over white at `alpha`, rounded to the nearest level.
///
/// Integer arithmetic keeps the result exact: the `+ 127` turns the division's
/// floor into rounding, and no tie can occur because the true blend is a whole
/// number divided by 255, which never ends in exactly one half. The numerator is
/// at most `255 * 255 + 127`, so it fits in a `u16`, and the quotient is at most
/// 255, so the narrowing cast cannot truncate.
fn blend_on_white(channel: u16, alpha: u16) -> u8 {
    ((channel * alpha + 255 * (255 - alpha) + 127) / 255) as u8
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
/// The source is first normalized to 8-bit RGBA when it carries alpha and to 8-bit
/// RGB otherwise. `webp::Encoder::from_image` accepts only those two layouts and
/// returns `Err("Unimplemented")` for grayscale, gray-alpha and every 16-bit or
/// float image, which left common grayscale scans unconverted. The `image` crate's
/// own encoders (JPEG, PNG, lossless WebP) convert colour types themselves, so only
/// this path needs it. The quality parameter controls lossy compression (1-100,
/// where 100 is highest quality).
fn encode_webp_lossy(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let (width, height) = (img.width(), img.height());
    let webp_data = if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        webp::Encoder::from_rgba(rgba.as_raw(), width, height).encode(quality as f32)
    } else {
        let rgb = img.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), width, height).encode(quality as f32)
    };
    Ok(webp_data.to_vec())
}

#[cfg(test)]
mod tests;
