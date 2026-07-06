//! Image write pipeline for extracted document images.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::common::{
    ExtractionConfig, ExtractionCounts, get_unique_output_path, write_image_to_file,
};
use crate::convert::{ConversionResult, try_convert};
use crate::extraction_warning::ImageWriteWarning;
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

/// Counts and warnings produced by one Image write pipeline batch.
#[derive(Debug, Default)]
pub struct ImageWriteResult {
    /// Write/conversion counts for the batch.
    pub counts: ExtractionCounts,
    /// Structured warning facts produced while preparing images for writing.
    pub warnings: Vec<ImageWriteWarning>,
}

/// Writes extracted images using the project's image write pipeline.
///
/// The interface accepts already-discovered images and returns extraction counts
/// plus warning facts for the batch. `BatchImages` preserves original bytes on
/// conversion skip/failure, while `RequiredCover` preserves the existing
/// cover-only behavior of skipping the cover when conversion cannot complete.
pub fn write_images(
    output_base_dir: &Path,
    base_name: &str,
    images: Vec<ImageToWrite>,
    config: &ExtractionConfig,
    mode: WriteMode,
) -> Result<ImageWriteResult> {
    if images.is_empty() {
        return Ok(ImageWriteResult::default());
    }

    if matches!(mode, WriteMode::BatchImages) {
        // Batch extraction always writes something for each accepted image, even
        // when conversion falls back to original bytes.
        fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;
    }

    let total_images = images.len();
    let mut result = ImageWriteResult::default();
    let mut gif_dir_created = false;

    for (seq_index, image) in images.into_iter().enumerate() {
        let is_gif = image.format == ImageFormat::Gif;
        let is_routed_gif = is_gif && config.gif_output.is_some();

        let Some(prepared) =
            prepare_image_for_write(image, base_name, config, mode, &mut result.warnings)?
        else {
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

        result.counts.extracted += 1;
        if is_routed_gif {
            result.counts.gifs_routed += 1;
        }
        if prepared.converted {
            result.counts.converted += 1;
        }
        if prepared.skipped_conversion {
            result.counts.skipped += 1;
        }
    }

    Ok(result)
}

/// Applies conversion policy before an image is written.
///
/// Conversion warnings are recorded as facts so the CLI adapter remains the only
/// module that renders terminal output.
fn prepare_image_for_write(
    image: ImageToWrite,
    base_name: &str,
    config: &ExtractionConfig,
    mode: WriteMode,
    warnings: &mut Vec<ImageWriteWarning>,
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
                    warnings.push(ImageWriteWarning::ConversionSkipped {
                        base_name: base_name.to_string(),
                        format: original_format,
                    });
                    Ok(Some(PreparedImage {
                        data: image.data,
                        format: original_format,
                        converted: false,
                        skipped_conversion: true,
                    }))
                }
                WriteMode::RequiredCover => {
                    warnings.push(ImageWriteWarning::CoverConversionSkipped {
                        format: original_format,
                    });
                    Ok(None)
                }
            },
            Err(e) => match mode {
                WriteMode::BatchImages => {
                    warnings.push(ImageWriteWarning::ConversionFailed {
                        base_name: base_name.to_string(),
                        message: e.to_string(),
                    });
                    Ok(Some(PreparedImage {
                        data: image.data,
                        format: image.format,
                        converted: false,
                        skipped_conversion: true,
                    }))
                }
                WriteMode::RequiredCover => {
                    warnings.push(ImageWriteWarning::CoverConversionFailed {
                        message: e.to_string(),
                    });
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

        let result = write_images(&temp_dir, "sample", images, &config, WriteMode::BatchImages)
            .expect("batch image write should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.skipped, 1);
        assert_eq!(result.counts.converted, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::ConversionSkipped {
                base_name: "sample".to_string(),
                format: ImageFormat::Svg,
            }]
        );
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

        let result = write_images(
            &temp_dir,
            "cover",
            images,
            &config,
            WriteMode::RequiredCover,
        )
        .expect("cover image write should succeed");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(result.counts.gifs_routed, 0);
        assert_eq!(result.counts.converted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::CoverConversionSkipped {
                format: ImageFormat::Svg,
            }]
        );
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

        let result = write_images(
            &output_dir,
            "sample",
            images,
            &config,
            WriteMode::BatchImages,
        )
        .expect("GIF routing should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.gifs_routed, 1);
        assert_eq!(result.counts.converted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert!(result.warnings.is_empty());
        assert!(gif_dir.join("sample.gif").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn batch_images_records_conversion_failure_warning() {
        let temp_dir = temp_test_dir("batch-conversion-failure");
        let config = ExtractionConfig {
            convert: Some(OutputFormat::Jpg),
            quality: 85,
            lossless: false,
            gif_output: None,
        };
        let images = vec![ImageToWrite {
            data: b"\x89PNG\r\n\x1A\nnot a valid png".to_vec(),
            format: ImageFormat::Png,
        }];

        let result = write_images(&temp_dir, "sample", images, &config, WriteMode::BatchImages)
            .expect("batch image write should preserve original bytes after conversion failure");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.skipped, 1);
        assert_eq!(result.counts.converted, 0);
        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            &result.warnings[0],
            ImageWriteWarning::ConversionFailed { base_name, message }
                if base_name == "sample" && message.contains("Failed to decode image")
        ));
        assert!(temp_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
