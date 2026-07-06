//! Image write pipeline for extracted document images.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::common::{
    ExtractionConfig, ExtractionCounts, get_unique_output_path, write_image_to_file,
};
use crate::convert::{ConversionResult, try_convert};
use crate::image_format::ImageFormat;

/// Image data ready to be written by the image write pipeline.
///
/// Callers are responsible for discovering images and deciding whether the
/// source extension is allowed. The write pipeline owns conversion, GIF
/// routing, output naming, directory creation, file writes, and counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageToWrite {
    /// Raw bytes for the extracted image.
    pub data: Vec<u8>,
    /// Canonical source image format.
    pub format: ImageFormat,
}

/// Controls how conversion failure affects an image write batch.
#[derive(Clone, Copy)]
pub enum WriteMode {
    /// Write every image, falling back to original bytes when conversion fails.
    BatchImages,
    /// Skip the image entirely when cover conversion fails.
    RequiredCover,
}

struct PreparedImage {
    data: Vec<u8>,
    format: ImageFormat,
    converted: bool,
    skipped_conversion: bool,
}

/// Writes extracted images using the project's image write pipeline.
///
/// The interface accepts already-discovered images and returns extraction
/// counts for the batch. `BatchImages` preserves original bytes on conversion
/// skip/failure, while `RequiredCover` preserves the existing cover-only
/// behavior of skipping the cover when conversion cannot complete.
pub fn write_images(
    output_base_dir: &Path,
    base_name: &str,
    images: Vec<ImageToWrite>,
    config: &ExtractionConfig,
    mode: WriteMode,
) -> Result<ExtractionCounts> {
    if images.is_empty() {
        return Ok(ExtractionCounts::default());
    }

    if matches!(mode, WriteMode::BatchImages) {
        // Batch extraction always writes something for each accepted image, even
        // when conversion falls back to original bytes.
        fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;
    }

    let total_images = images.len();
    let mut counts = ExtractionCounts::default();
    let mut gif_dir_created = false;

    for (seq_index, image) in images.into_iter().enumerate() {
        let is_gif = image.format == ImageFormat::Gif;
        let is_routed_gif = is_gif && config.gif_output.is_some();

        let Some(prepared) = prepare_image_for_write(image, base_name, config, mode)? else {
            continue;
        };

        let effective_output_dir =
            if let (true, Some(gif_dir)) = (is_routed_gif, config.gif_output.as_deref()) {
                if !gif_dir_created {
                    fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                    gif_dir_created = true;
                }
                gif_dir
            } else {
                output_base_dir
            };

        fs::create_dir_all(effective_output_dir).context("Failed to create output directory")?;

        let output_path = get_unique_output_path(
            effective_output_dir,
            base_name,
            seq_index,
            total_images,
            prepared.format.extension(),
        )?;

        write_image_to_file(&output_path, &prepared.data)?;

        counts.extracted += 1;
        if is_routed_gif {
            counts.gifs_routed += 1;
        }
        if prepared.converted {
            counts.converted += 1;
        }
        if prepared.skipped_conversion {
            counts.skipped += 1;
        }
    }

    Ok(counts)
}

/// Applies conversion policy before an image is written.
fn prepare_image_for_write(
    image: ImageToWrite,
    base_name: &str,
    config: &ExtractionConfig,
    mode: WriteMode,
) -> Result<Option<PreparedImage>> {
    let is_routed_gif = image.format == ImageFormat::Gif && config.gif_output.is_some();

    if let Some(format) = config.convert {
        if is_routed_gif {
            return Ok(Some(PreparedImage {
                data: image.data,
                format: image.format,
                converted: false,
                skipped_conversion: false,
            }));
        }

        match try_convert(
            &image.data,
            image.format,
            format,
            config.quality,
            config.lossless,
        ) {
            Ok(ConversionResult::Converted(converted_bytes, format)) => Ok(Some(PreparedImage {
                data: converted_bytes,
                format,
                converted: true,
                skipped_conversion: false,
            })),
            Ok(ConversionResult::Skipped(original_format)) => match mode {
                WriteMode::BatchImages => {
                    eprintln!(
                        "Warning: Skipping conversion for {} ({} format not supported for conversion)",
                        base_name,
                        original_format.extension()
                    );
                    Ok(Some(PreparedImage {
                        data: image.data,
                        format: original_format,
                        converted: false,
                        skipped_conversion: true,
                    }))
                }
                WriteMode::RequiredCover => {
                    eprintln!(
                        "Warning: Cover image format '{}' not supported for conversion, skipping cover.",
                        original_format.extension()
                    );
                    Ok(None)
                }
            },
            Err(e) => match mode {
                WriteMode::BatchImages => {
                    eprintln!(
                        "Warning: Conversion failed for image in {}: {}",
                        base_name, e
                    );
                    Ok(Some(PreparedImage {
                        data: image.data,
                        format: image.format,
                        converted: false,
                        skipped_conversion: true,
                    }))
                }
                WriteMode::RequiredCover => {
                    eprintln!("Warning: Cover conversion failed: {}", e);
                    Ok(None)
                }
            },
        }
    } else {
        Ok(Some(PreparedImage {
            data: image.data,
            format: image.format,
            converted: false,
            skipped_conversion: false,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::OutputFormat;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-image-writer-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn batch_images_write_original_when_conversion_is_skipped() {
        let temp_dir = temp_test_dir("batch-skip");
        let config = ExtractionConfig {
            convert: Some(OutputFormat::Png),
            quality: 85,
            lossless: false,
            gif_output: None,
        };
        let images = vec![ImageToWrite {
            data: b"<svg/>".to_vec(),
            format: ImageFormat::Svg,
        }];

        let counts = write_images(&temp_dir, "sample", images, &config, WriteMode::BatchImages)
            .expect("batch image write should succeed");

        assert_eq!(counts.extracted, 1);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.converted, 0);
        assert!(temp_dir.join("sample.svg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn required_cover_skips_image_when_conversion_is_skipped() {
        let temp_dir = temp_test_dir("cover-skip");
        let config = ExtractionConfig {
            convert: Some(OutputFormat::Png),
            quality: 85,
            lossless: false,
            gif_output: None,
        };
        let images = vec![ImageToWrite {
            data: b"<svg/>".to_vec(),
            format: ImageFormat::Svg,
        }];

        let counts = write_images(
            &temp_dir,
            "cover",
            images,
            &config,
            WriteMode::RequiredCover,
        )
        .expect("cover image write should succeed");

        assert_eq!(counts.extracted, 0);
        assert_eq!(counts.gifs_routed, 0);
        assert_eq!(counts.converted, 0);
        assert_eq!(counts.skipped, 0);
        assert!(!temp_dir.exists());
    }

    #[test]
    fn gif_routing_takes_priority_over_conversion() {
        let temp_dir = temp_test_dir("gif-route");
        let gif_dir = temp_dir.join("gifs");
        let output_dir = temp_dir.join("images");
        let config = ExtractionConfig {
            convert: Some(OutputFormat::Png),
            quality: 85,
            lossless: false,
            gif_output: Some(gif_dir.clone()),
        };
        let images = vec![ImageToWrite {
            data: b"not a real gif but routed as-is".to_vec(),
            format: ImageFormat::Gif,
        }];

        let counts = write_images(
            &output_dir,
            "sample",
            images,
            &config,
            WriteMode::BatchImages,
        )
        .expect("GIF routing should succeed");

        assert_eq!(counts.extracted, 1);
        assert_eq!(counts.gifs_routed, 1);
        assert_eq!(counts.converted, 0);
        assert_eq!(counts.skipped, 0);
        assert!(gif_dir.join("sample.gif").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
