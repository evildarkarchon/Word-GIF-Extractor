//! Image write pipeline for turning buffered archive resources into image files.

mod discovery;

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionOutcome, ConversionPolicy};
use crate::image_format::ImageFormat;

use self::discovery::{discover_normal_images, discover_required_cover};

/// Buffered raw facts for one named archive resource.
#[derive(Debug)]
pub(crate) struct ArchiveImageSource {
    data: Vec<u8>,
    source_name: String,
    mime: Option<String>,
}

impl ArchiveImageSource {
    /// Creates a named archive image source without a declared MIME type.
    pub(crate) fn named(data: Vec<u8>, source_name: impl Into<String>) -> Self {
        Self {
            data,
            source_name: source_name.into(),
            mime: None,
        }
    }

    /// Adds the document-declared MIME type used by Image format identification.
    #[must_use]
    pub(crate) fn with_mime(mut self, mime: impl Into<String>) -> Self {
        self.mime = Some(mime.into());
        self
    }
}

/// Valid per-run choices interpreted by the Image write pipeline.
#[derive(Debug)]
pub(crate) struct ImageWritePolicy {
    allowed_formats: HashSet<ImageFormat>,
    conversion: Option<ConversionPolicy>,
    gif_output: Option<PathBuf>,
}

impl ImageWritePolicy {
    /// Creates the immutable Image write policy for one Extraction run.
    pub(crate) fn new(
        allowed_formats: HashSet<ImageFormat>,
        conversion: Option<ConversionPolicy>,
        gif_output: Option<PathBuf>,
    ) -> Self {
        Self {
            allowed_formats,
            conversion,
            gif_output,
        }
    }
}

/// Observable Image write pipeline counts for one invocation.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ImageWriteCounts {
    pub(crate) extracted: usize,
    pub(crate) gifs_routed: usize,
    pub(crate) converted: usize,
    pub(crate) skipped: usize,
}

/// Structured warning facts produced by the Image write pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageWriteWarning {
    ExtensionFallback {
        source_name: String,
        format: ImageFormat,
    },
    CoverDefaultToJpeg {
        mime: String,
    },
    UnsupportedCoverFormat {
        format: ImageFormat,
    },
    ConversionSkipped {
        base_name: String,
        format: ImageFormat,
    },
    CoverConversionSkipped {
        format: ImageFormat,
    },
    ConversionFailed {
        base_name: String,
        message: String,
    },
    CoverConversionFailed {
        message: String,
    },
}

impl ImageWriteWarning {
    /// Formats this warning using the existing terminal wording.
    pub(crate) fn message(&self) -> String {
        match self {
            ImageWriteWarning::ExtensionFallback {
                source_name,
                format,
            } => format!(
                "Magic detection failed for {}; falling back to .{} extension",
                source_name,
                format.extension()
            ),
            ImageWriteWarning::CoverDefaultToJpeg { mime } => format!(
                "Cover image MIME '{}' could not be identified; defaulting to .jpg extension.",
                mime
            ),
            ImageWriteWarning::UnsupportedCoverFormat { format } => format!(
                "Cover image format '{}' not in allowed formats, skipping.",
                format.extension()
            ),
            ImageWriteWarning::ConversionSkipped { base_name, format } => format!(
                "Skipping conversion for {} ({} format not supported for conversion)",
                base_name,
                format.extension()
            ),
            ImageWriteWarning::CoverConversionSkipped { format } => format!(
                "Cover image format '{}' not supported for conversion, skipping cover.",
                format.extension()
            ),
            ImageWriteWarning::ConversionFailed { base_name, message } => {
                format!("Conversion failed for image in {}: {}", base_name, message)
            }
            ImageWriteWarning::CoverConversionFailed { message } => {
                format!("Cover conversion failed: {}", message)
            }
        }
    }
}

/// Complete observable outcome of one Image write pipeline invocation.
#[derive(Debug, Default)]
pub(crate) struct ImageWriteResult {
    pub(crate) counts: ImageWriteCounts,
    pub(crate) warnings: Vec<ImageWriteWarning>,
}

enum ImageWritePurpose {
    NormalImages(Vec<ArchiveImageSource>),
    RequiredEpubCover { data: Vec<u8>, mime: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageWritePurposeKind {
    NormalImages,
    RequiredEpubCover,
}

impl ImageWritePurposeKind {
    /// Returns whether conversion failure preserves and writes original bytes.
    fn preserves_original_on_conversion_failure(self) -> bool {
        self == ImageWritePurposeKind::NormalImages
    }
}

#[derive(Debug)]
struct AcceptedImage {
    data: Vec<u8>,
    format: ImageFormat,
}

struct PreparedImage {
    data: Vec<u8>,
    format: ImageFormat,
    converted: bool,
    skipped_conversion: bool,
}

/// Document-specific facts for one Image write pipeline invocation.
pub(crate) struct ImageWriteRequest<'a> {
    output_dir: &'a Path,
    base_name: &'a str,
    purpose: ImageWritePurpose,
}

impl<'a> ImageWriteRequest<'a> {
    /// Creates a normal-images request from ordered, named archive sources.
    pub(crate) fn normal_images(
        output_dir: &'a Path,
        base_name: &'a str,
        sources: Vec<ArchiveImageSource>,
    ) -> Self {
        Self {
            output_dir,
            base_name,
            purpose: ImageWritePurpose::NormalImages(sources),
        }
    }

    /// Creates a required EPUB cover request from exactly one payload and MIME type.
    pub(crate) fn required_epub_cover(
        output_dir: &'a Path,
        base_name: &'a str,
        data: Vec<u8>,
        mime: impl Into<String>,
    ) -> Self {
        Self {
            output_dir,
            base_name,
            purpose: ImageWritePurpose::RequiredEpubCover {
                data,
                mime: mime.into(),
            },
        }
    }
}

/// Immutable Image write pipeline configured for one Extraction run.
pub(crate) struct ImageWritePipeline {
    policy: ImageWritePolicy,
}

impl ImageWritePipeline {
    /// Binds one Image write policy for every document in an Extraction run.
    pub(crate) fn new(policy: ImageWritePolicy) -> Self {
        Self { policy }
    }

    /// Discovers, prepares, and writes one requested image set.
    ///
    /// Returns phase-ordered warning facts and counts for files actually written.
    /// Filesystem setup, collision exhaustion, create, write, and flush failures
    /// abort the document with an error; earlier successful writes are not rolled back.
    pub(crate) fn write(&self, request: ImageWriteRequest<'_>) -> Result<ImageWriteResult> {
        let (discovered, purpose) = match request.purpose {
            ImageWritePurpose::NormalImages(sources) => (
                discover_normal_images(sources, &self.policy.allowed_formats),
                ImageWritePurposeKind::NormalImages,
            ),
            ImageWritePurpose::RequiredEpubCover { data, mime } => (
                discover_required_cover(data, mime, &self.policy.allowed_formats),
                ImageWritePurposeKind::RequiredEpubCover,
            ),
        };

        let mut result = write_discovered_images(
            request.output_dir,
            request.base_name,
            discovered.images,
            &self.policy,
            purpose,
        )?;
        result.warnings = discovered
            .warnings
            .into_iter()
            .chain(result.warnings)
            .collect();

        Ok(result)
    }
}

/// Writes images already accepted by Archive image discovery.
fn write_discovered_images(
    output_base_dir: &Path,
    base_name: &str,
    images: Vec<AcceptedImage>,
    policy: &ImageWritePolicy,
    purpose: ImageWritePurposeKind,
) -> Result<ImageWriteResult> {
    if images.is_empty() {
        return Ok(ImageWriteResult::default());
    }

    if purpose == ImageWritePurposeKind::NormalImages {
        // Normal extraction always writes something for each accepted image,
        // even when conversion falls back to the original bytes.
        fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;
    }

    let total_images = images.len();
    let mut result = ImageWriteResult::default();
    let mut gif_dir_created = false;

    for (seq_index, image) in images.into_iter().enumerate() {
        let is_routed_gif = image.format == ImageFormat::Gif && policy.gif_output.is_some();

        let Some(prepared) =
            prepare_image_for_write(image, base_name, policy, purpose, &mut result.warnings)
        else {
            continue;
        };

        let effective_output_dir =
            if let (true, Some(gif_dir)) = (is_routed_gif, policy.gif_output.as_deref()) {
                if !gif_dir_created {
                    fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                    gif_dir_created = true;
                }
                gif_dir
            } else {
                output_base_dir
            };

        fs::create_dir_all(effective_output_dir).context("Failed to create output directory")?;

        let output_path = unique_output_path(
            effective_output_dir,
            base_name,
            seq_index,
            total_images,
            prepared.format.extension(),
        )?;

        write_image_file(&output_path, &prepared.data)?;

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

/// Applies conversion and Image write purpose semantics before one file write.
///
/// Returns `None` when a required cover cannot be converted and must therefore
/// be omitted. Conversion warning facts are appended in accepted-source order.
fn prepare_image_for_write(
    image: AcceptedImage,
    base_name: &str,
    policy: &ImageWritePolicy,
    purpose: ImageWritePurposeKind,
    warnings: &mut Vec<ImageWriteWarning>,
) -> Option<PreparedImage> {
    let is_routed_gif = image.format == ImageFormat::Gif && policy.gif_output.is_some();

    if let Some(conversion) = &policy.conversion {
        if is_routed_gif {
            return Some(PreparedImage {
                data: image.data,
                format: image.format,
                converted: false,
                skipped_conversion: false,
            });
        }

        match conversion.convert(&image.data, image.format) {
            Ok(ConversionOutcome::Converted(converted_bytes, format)) => Some(PreparedImage {
                data: converted_bytes,
                format,
                converted: true,
                skipped_conversion: false,
            }),
            Ok(ConversionOutcome::PreservedMatchingSource) => Some(PreparedImage {
                data: image.data,
                format: image.format,
                converted: false,
                skipped_conversion: false,
            }),
            Ok(ConversionOutcome::UnsupportedSource(original_format)) => {
                if purpose.preserves_original_on_conversion_failure() {
                    warnings.push(ImageWriteWarning::ConversionSkipped {
                        base_name: base_name.to_string(),
                        format: original_format,
                    });
                    Some(PreparedImage {
                        data: image.data,
                        format: original_format,
                        converted: false,
                        skipped_conversion: true,
                    })
                } else {
                    warnings.push(ImageWriteWarning::CoverConversionSkipped {
                        format: original_format,
                    });
                    None
                }
            }
            Err(error) => {
                if purpose.preserves_original_on_conversion_failure() {
                    warnings.push(ImageWriteWarning::ConversionFailed {
                        base_name: base_name.to_string(),
                        message: error.to_string(),
                    });
                    Some(PreparedImage {
                        data: image.data,
                        format: image.format,
                        converted: false,
                        skipped_conversion: true,
                    })
                } else {
                    warnings.push(ImageWriteWarning::CoverConversionFailed {
                        message: error.to_string(),
                    });
                    None
                }
            }
        }
    } else {
        Some(PreparedImage {
            data: image.data,
            format: image.format,
            converted: false,
            skipped_conversion: false,
        })
    }
}

/// Builds a collision-free output path using the existing numbering policy.
fn unique_output_path(
    output_base_dir: &Path,
    base_name: &str,
    seq_index: usize,
    total_images: usize,
    extension: &str,
) -> Result<PathBuf> {
    let output_filename = if total_images > 1 {
        format!("{}_{}.{}", base_name, seq_index + 1, extension)
    } else {
        format!("{}.{}", base_name, extension)
    };

    let mut output_path = output_base_dir.join(output_filename);

    // Bound collision attempts so a hostile or unusual filesystem cannot loop forever.
    if output_path.exists() {
        let base_stem = output_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let base_ext = output_path
            .extension()
            .map(|extension| extension.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut counter = 0u32;
        const MAX_ATTEMPTS: u32 = 1000;

        while output_path.exists() {
            counter += 1;
            if counter > MAX_ATTEMPTS {
                anyhow::bail!(
                    "Could not find unique filename after {} attempts for {}",
                    MAX_ATTEMPTS,
                    base_stem
                );
            }
            let new_filename = if base_ext.is_empty() {
                format!("{}_{}", base_stem, counter)
            } else {
                format!("{}_{}.{}", base_stem, counter, base_ext)
            };
            output_path.set_file_name(new_filename);
        }
    }

    Ok(output_path)
}

/// Writes and flushes one image file with path-specific error context.
fn write_image_file(output_path: &Path, data: &[u8]) -> Result<()> {
    let outfile = fs::File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    let mut outfile = io::BufWriter::new(outfile);

    outfile
        .write_all(data)
        .with_context(|| format!("Failed to write image data to {}", output_path.display()))?;

    outfile
        .flush()
        .with_context(|| format!("Failed to flush data to {}", output_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::{ConversionRequest, ConversionTarget};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR";

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-pipeline-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn normal_images_are_discovered_and_written_through_pipeline_interface() {
        let temp_dir = temp_test_dir("normal-images");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![ArchiveImageSource::named(
                    MINIMAL_PNG.to_vec(),
                    "word/media/image.bin",
                )],
            ))
            .expect("normal image write should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.warnings.is_empty());
        assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), MINIMAL_PNG);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn normal_images_return_extension_fallback_warning_before_writing() {
        let temp_dir = temp_test_dir("extension-fallback");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![ArchiveImageSource::named(
                    b"not actually a png".to_vec(),
                    "word/media/image.png",
                )],
            ))
            .expect("extension fallback image should be written");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::ExtensionFallback {
                source_name: "word/media/image.png".to_string(),
                format: ImageFormat::Png,
            }]
        );
        assert!(temp_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn unsafe_normal_sources_are_skipped_without_creating_output() {
        let temp_dir = temp_test_dir("unsafe-source");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![
                    ArchiveImageSource::named(MINIMAL_PNG.to_vec(), "../media/image.png"),
                    ArchiveImageSource::named(MINIMAL_PNG.to_vec(), "/media/image.png"),
                    ArchiveImageSource::named(MINIMAL_PNG.to_vec(), "C:\\media\\image.png"),
                    ArchiveImageSource::named(MINIMAL_PNG.to_vec(), "media/image.png::$DATA"),
                    ArchiveImageSource::named(MINIMAL_PNG.to_vec(), "media/image\0.png"),
                ],
            ))
            .expect("unsafe source should be skipped normally");

        assert_eq!(result.counts.extracted, 0);
        assert!(result.warnings.is_empty());
        assert!(!temp_dir.exists());
    }

    #[test]
    fn required_cover_defaults_to_jpeg_through_pipeline_interface() {
        let temp_dir = temp_test_dir("cover-default");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
            None,
            None,
        ));

        let result = pipeline
            .write(ImageWriteRequest::required_epub_cover(
                &temp_dir,
                "cover",
                b"unknown cover bytes".to_vec(),
                "application/octet-stream",
            ))
            .expect("cover fallback should be written");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::CoverDefaultToJpeg {
                mime: "application/octet-stream".to_string(),
            }]
        );
        assert!(temp_dir.join("cover.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn required_cover_filtered_out_returns_warnings_without_creating_output() {
        let temp_dir = temp_test_dir("cover-filtered");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = pipeline
            .write(ImageWriteRequest::required_epub_cover(
                &temp_dir,
                "cover",
                b"unknown cover bytes".to_vec(),
                "application/octet-stream",
            ))
            .expect("filtered cover should be a normal pipeline outcome");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(
            result.warnings,
            vec![
                ImageWriteWarning::CoverDefaultToJpeg {
                    mime: "application/octet-stream".to_string(),
                },
                ImageWriteWarning::UnsupportedCoverFormat {
                    format: ImageFormat::Jpg,
                },
            ]
        );
        assert!(!temp_dir.exists());
    }

    #[test]
    fn required_cover_conversion_skip_writes_nothing() {
        let temp_dir = temp_test_dir("cover-conversion-skip");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Svg]),
            ConversionTarget::Png,
            None,
        );

        let result = pipeline
            .write(ImageWriteRequest::required_epub_cover(
                &temp_dir,
                "cover",
                b"<svg/>".to_vec(),
                "image/svg+xml",
            ))
            .expect("unsupported cover conversion should be a normal outcome");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::CoverConversionSkipped {
                format: ImageFormat::Svg,
            }]
        );
        assert!(!temp_dir.exists());
    }

    #[test]
    fn normal_conversion_skip_writes_original_and_preserves_warning_order() {
        let temp_dir = temp_test_dir("normal-conversion-skip");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Svg]),
            ConversionTarget::Png,
            None,
        );

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![ArchiveImageSource::named(
                    b"not really svg".to_vec(),
                    "media/image.svg",
                )],
            ))
            .expect("normal conversion skip should preserve original bytes");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.skipped, 1);
        assert_eq!(
            result.warnings,
            vec![
                ImageWriteWarning::ExtensionFallback {
                    source_name: "media/image.svg".to_string(),
                    format: ImageFormat::Svg,
                },
                ImageWriteWarning::ConversionSkipped {
                    base_name: "sample".to_string(),
                    format: ImageFormat::Svg,
                },
            ]
        );
        assert!(temp_dir.join("sample.svg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn normal_conversion_failure_writes_original_and_counts_skip() {
        let temp_dir = temp_test_dir("normal-conversion-failure");
        let original = b"\x89PNG\r\n\x1A\nnot a valid png".to_vec();
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Png]),
            ConversionTarget::Jpg,
            None,
        );

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![ArchiveImageSource::named(original.clone(), "image.png")],
            ))
            .expect("normal conversion failure should preserve original bytes");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.converted, 0);
        assert_eq!(result.counts.skipped, 1);
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::ConversionFailed { base_name, message }]
                if base_name == "sample" && message.contains("Failed to decode image")
        ));
        assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), original);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn matching_conversion_target_preserves_original_without_conversion_count() {
        let temp_dir = temp_test_dir("matching-conversion-target");
        let original = b"accepted through extension fallback".to_vec();
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Png]),
            ConversionTarget::Png,
            None,
        );

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![ArchiveImageSource::named(original.clone(), "image.png")],
            ))
            .expect("matching target should preserve source bytes");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.converted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::ExtensionFallback {
                source_name: "image.png".to_string(),
                format: ImageFormat::Png,
            }]
        );
        assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), original);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn routed_gif_bypasses_conversion() {
        let temp_dir = temp_test_dir("gif-routing");
        let gif_dir = temp_dir.join("gifs");
        let output_dir = temp_dir.join("images");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Gif]),
            ConversionTarget::Png,
            Some(gif_dir.clone()),
        );

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &output_dir,
                "sample",
                vec![ArchiveImageSource::named(
                    b"GIF89a".to_vec(),
                    "media/image.gif",
                )],
            ))
            .expect("routed GIF should be written without conversion");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.gifs_routed, 1);
        assert_eq!(result.counts.converted, 0);
        assert!(result.warnings.is_empty());
        assert!(gif_dir.join("sample.gif").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn normal_source_can_be_identified_from_mime() {
        let temp_dir = temp_test_dir("mime-source");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = pipeline
            .write(ImageWriteRequest::normal_images(
                &temp_dir,
                "sample",
                vec![
                    ArchiveImageSource::named(b"unknown bytes".to_vec(), "media/image.bin")
                        .with_mime("image/png"),
                ],
            ))
            .expect("MIME-identified image should be written");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.warnings.is_empty());
        assert!(temp_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Builds a pipeline with a default Conversion policy for interface tests.
    fn pipeline_with_conversion(
        allowed_formats: HashSet<ImageFormat>,
        target: ConversionTarget,
        gif_output: Option<PathBuf>,
    ) -> ImageWritePipeline {
        let conversion = ConversionPolicy::try_from(ConversionRequest {
            target,
            quality: None,
            lossless: false,
        })
        .expect("test conversion request should be valid");

        ImageWritePipeline::new(ImageWritePolicy::new(
            allowed_formats,
            Some(conversion),
            gif_output,
        ))
    }
}
