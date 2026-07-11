//! Image write pipeline for discovering and emitting archive images incrementally.

mod discovery;
mod emission;

use anyhow::Result;
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionOutcome, ConversionPolicy};
use crate::image_format::ImageFormat;

use self::discovery::{discover_image, is_source_safe};
use self::emission::ImageFileEmission;

/// Metadata supplied before Archive image discovery reads one archive resource.
#[derive(Debug, Clone)]
pub(crate) struct ArchiveImageSource {
    diagnostic_name: String,
    format_source_name: Option<String>,
    mime: Option<String>,
}

impl ArchiveImageSource {
    /// Creates a normal archive source whose name is also Image format evidence.
    pub(crate) fn named(source_name: impl Into<String>) -> Self {
        let source_name = source_name.into();
        Self {
            diagnostic_name: source_name.clone(),
            format_source_name: Some(source_name),
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
    ArchiveImageAcquisitionFailed {
        source_name: String,
        message: String,
    },
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
    /// Creates one non-fatal archive resource acquisition warning fact.
    pub(crate) fn archive_image_acquisition_failed(
        source_name: impl Into<String>,
        error: impl fmt::Display,
    ) -> Self {
        Self::ArchiveImageAcquisitionFailed {
            source_name: source_name.into(),
            message: error.to_string(),
        }
    }

    /// Formats this warning using the existing terminal wording.
    pub(crate) fn message(&self) -> String {
        match self {
            ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name,
                message,
            } => format!(
                "Could not read archive resource '{}': {}",
                source_name, message
            ),
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

impl ImageWriteResult {
    /// Creates an outcome containing one typed warning fact and no emitted files.
    pub(crate) fn from_warning(warning: ImageWriteWarning) -> Self {
        Self::from_warnings(vec![warning])
    }

    /// Creates an outcome containing typed warning facts and no emitted files.
    pub(crate) fn from_warnings(warnings: Vec<ImageWriteWarning>) -> Self {
        Self {
            counts: ImageWriteCounts::default(),
            warnings,
        }
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
    routed_gif: bool,
    converted: bool,
    skipped_conversion: bool,
}

/// Document-specific facts for one Image write pipeline invocation.
pub(crate) struct ImageWriteRequest<'a> {
    output_dir: &'a Path,
    base_name: &'a str,
}

impl<'a> ImageWriteRequest<'a> {
    /// Creates a normal-images request whose sources will be visited in document order.
    pub(crate) fn normal_images(output_dir: &'a Path, base_name: &'a str) -> Self {
        Self {
            output_dir,
            base_name,
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

    /// Returns whether one identified Image format is accepted by this run.
    pub(crate) fn accepts_format(&self, format: ImageFormat) -> bool {
        self.policy.allowed_formats.contains(&format)
    }

    /// Lends the optional Conversion policy without interpreting document behavior.
    pub(crate) fn conversion_policy(&self) -> Option<&ConversionPolicy> {
        self.policy.conversion.as_ref()
    }

    /// Returns whether an image will be routed to the configured GIF destination.
    pub(crate) fn routes_gif(&self, format: ImageFormat) -> bool {
        format == ImageFormat::Gif && self.policy.gif_output.is_some()
    }

    /// Emits one fully decided image and returns counts for the completed file.
    ///
    /// GIF destination routing remains a generic Image write policy. Create,
    /// write, flush, and collision failures are returned and abort the document.
    pub(crate) fn emit_single_image(
        &self,
        output_dir: &Path,
        base_name: &str,
        data: Vec<u8>,
        format: ImageFormat,
        converted: bool,
    ) -> Result<ImageWriteResult> {
        let routed_gif = self.routes_gif(format);
        let destination = if routed_gif {
            self.policy
                .gif_output
                .as_deref()
                .expect("routed GIF should have a configured destination")
        } else {
            output_dir
        };
        let mut emission = ImageFileEmission::new(base_name, false);
        emission.emit(destination, format, &data)?;

        Ok(ImageWriteResult {
            counts: ImageWriteCounts {
                extracted: 1,
                gifs_routed: usize::from(routed_gif),
                converted: usize::from(converted),
                skipped: 0,
            },
            warnings: Vec::new(),
        })
    }

    /// Discovers, prepares, and writes sources supplied through one scoped traversal.
    ///
    /// The traversal must finish each reader before opening the next archive entry.
    /// Per-resource acquisition failures belong to the visitor and remain non-fatal;
    /// an error returned by the traversal aborts the document.
    ///
    /// Returns phase-ordered warning facts and counts for files actually written.
    /// Filesystem setup, collision exhaustion, create, write, and flush failures
    /// abort the document with an error; earlier successful writes are not rolled back.
    pub(crate) fn write_from(
        &self,
        request: ImageWriteRequest<'_>,
        traverse: impl FnOnce(&mut ArchiveImageVisitor<'_, '_>) -> Result<()>,
    ) -> Result<ImageWriteResult> {
        let mut visitor = ArchiveImageVisitor::new(&self.policy, request);
        traverse(&mut visitor)?;
        visitor.finish()
    }
}

/// Scoped authority for per-resource discovery, preparation, and ordered emission.
pub(crate) struct ArchiveImageVisitor<'policy, 'request> {
    policy: &'policy ImageWritePolicy,
    output_dir: &'request Path,
    base_name: &'request str,
    discovery_warnings: Vec<ImageWriteWarning>,
    conversion_warnings: Vec<ImageWriteWarning>,
    counts: ImageWriteCounts,
    pending_first: Option<PreparedImage>,
    multiple_emission: Option<ImageFileEmission<'request>>,
}

impl<'policy, 'request> ArchiveImageVisitor<'policy, 'request> {
    /// Starts one scoped Archive image discovery traversal.
    fn new(policy: &'policy ImageWritePolicy, request: ImageWriteRequest<'request>) -> Self {
        Self {
            policy,
            output_dir: request.output_dir,
            base_name: request.base_name,
            discovery_warnings: Vec::new(),
            conversion_warnings: Vec::new(),
            counts: ImageWriteCounts::default(),
            pending_first: None,
            multiple_emission: None,
        }
    }

    /// Discovers and prepares one source before releasing its borrowed reader.
    ///
    /// Source read failures become warning facts and return `Ok(())`. Output
    /// emission failures remain fatal and are returned to the traversal.
    pub(crate) fn visit(
        &mut self,
        source: ArchiveImageSource,
        reader: &mut dyn Read,
    ) -> Result<()> {
        let discovered = discover_image(&source, reader, &self.policy.allowed_formats);
        self.discovery_warnings.extend(discovered.warnings);

        let Some(image) = discovered.image else {
            return Ok(());
        };
        let Some(prepared) = prepare_image_for_write(
            image,
            self.base_name,
            self.policy,
            &mut self.conversion_warnings,
        ) else {
            return Ok(());
        };

        self.stage_prepared(prepared)
    }

    /// Records a source that the document adapter could not open.
    ///
    /// Unsafe normal-image names remain silent skips, matching discovery behavior.
    pub(crate) fn unreadable(&mut self, source: ArchiveImageSource, error: impl fmt::Display) {
        if is_source_safe(&source) {
            self.discovery_warnings
                .push(ImageWriteWarning::archive_image_acquisition_failed(
                    source.diagnostic_name,
                    error,
                ));
        }
    }

    /// Holds the first prepared image until singular versus multiple naming is known.
    ///
    /// Returns an error if switching to multiple naming cannot emit either prepared image.
    fn stage_prepared(&mut self, prepared: PreparedImage) -> Result<()> {
        if let Some(mut emission) = self.multiple_emission.take() {
            self.emit_prepared(&mut emission, prepared)?;
            self.multiple_emission = Some(emission);
            return Ok(());
        }

        if let Some(first) = self.pending_first.take() {
            let mut emission = ImageFileEmission::new(self.base_name, true);
            self.emit_prepared(&mut emission, first)?;
            self.emit_prepared(&mut emission, prepared)?;
            self.multiple_emission = Some(emission);
        } else {
            self.pending_first = Some(prepared);
        }

        Ok(())
    }

    /// Emits one prepared image and records only successfully completed output.
    ///
    /// Returns an error when Image file emission cannot create or complete the output.
    fn emit_prepared(
        &mut self,
        emission: &mut ImageFileEmission<'_>,
        prepared: PreparedImage,
    ) -> Result<()> {
        let output_dir = if prepared.routed_gif {
            // Preparation only marks routing when the immutable policy has a destination.
            self.policy
                .gif_output
                .as_deref()
                .expect("routed GIF should have a configured destination")
        } else {
            self.output_dir
        };
        emission.emit(output_dir, prepared.format, &prepared.data)?;

        self.counts.extracted += 1;
        if prepared.routed_gif {
            self.counts.gifs_routed += 1;
        }
        if prepared.converted {
            self.counts.converted += 1;
        }
        if prepared.skipped_conversion {
            self.counts.skipped += 1;
        }

        Ok(())
    }

    /// Completes singular lookahead and returns phase-ordered warning facts.
    ///
    /// Returns an error if the lone pending image cannot be emitted.
    fn finish(mut self) -> Result<ImageWriteResult> {
        if let Some(prepared) = self.pending_first.take() {
            let mut emission = ImageFileEmission::new(self.base_name, false);
            self.emit_prepared(&mut emission, prepared)?;
        }

        Ok(ImageWriteResult {
            counts: self.counts,
            warnings: self
                .discovery_warnings
                .into_iter()
                .chain(self.conversion_warnings)
                .collect(),
        })
    }
}

/// Applies normal-image conversion semantics before one file write.
///
/// Conversion warning facts are appended in accepted-source order.
fn prepare_image_for_write(
    image: AcceptedImage,
    base_name: &str,
    policy: &ImageWritePolicy,
    warnings: &mut Vec<ImageWriteWarning>,
) -> Option<PreparedImage> {
    let is_routed_gif = image.format == ImageFormat::Gif && policy.gif_output.is_some();

    if let Some(conversion) = &policy.conversion {
        if is_routed_gif {
            return Some(PreparedImage {
                data: image.data,
                format: image.format,
                routed_gif: true,
                converted: false,
                skipped_conversion: false,
            });
        }

        match conversion.convert(&image.data, image.format) {
            Ok(ConversionOutcome::Converted(converted_bytes, format)) => Some(PreparedImage {
                data: converted_bytes,
                format,
                routed_gif: false,
                converted: true,
                skipped_conversion: false,
            }),
            Ok(ConversionOutcome::PreservedMatchingSource) => Some(PreparedImage {
                data: image.data,
                format: image.format,
                routed_gif: false,
                converted: false,
                skipped_conversion: false,
            }),
            Ok(ConversionOutcome::UnsupportedSource(original_format)) => {
                warnings.push(ImageWriteWarning::ConversionSkipped {
                    base_name: base_name.to_string(),
                    format: original_format,
                });
                Some(PreparedImage {
                    data: image.data,
                    format: original_format,
                    routed_gif: false,
                    converted: false,
                    skipped_conversion: true,
                })
            }
            Err(error) => {
                warnings.push(ImageWriteWarning::ConversionFailed {
                    base_name: base_name.to_string(),
                    message: error.to_string(),
                });
                Some(PreparedImage {
                    data: image.data,
                    format: image.format,
                    routed_gif: false,
                    converted: false,
                    skipped_conversion: true,
                })
            }
        }
    } else {
        Some(PreparedImage {
            data: image.data,
            format: image.format,
            routed_gif: is_routed_gif,
            converted: false,
            skipped_conversion: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::{ConversionRequest, ConversionTarget};
    use std::fs;
    use std::io::{self, Cursor, Read};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR";

    struct FailAfterReader {
        cursor: Cursor<Vec<u8>>,
        fail_at: u64,
    }

    impl FailAfterReader {
        /// Creates a reader that reports an error after the requested byte offset.
        fn new(data: Vec<u8>, fail_at: u64) -> Self {
            Self {
                cursor: Cursor::new(data),
                fail_at,
            }
        }
    }

    impl Read for FailAfterReader {
        /// Reads through `fail_at`, then returns the injected acquisition failure.
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let position = self.cursor.position();
            if position >= self.fail_at {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "injected archive resource failure",
                ));
            }

            let remaining = (self.fail_at - position) as usize;
            let read_len = buffer.len().min(remaining);
            self.cursor.read(&mut buffer[..read_len])
        }
    }

    struct AssertOutputBeforeTailReader {
        cursor: Cursor<Vec<u8>>,
        expected_output: PathBuf,
        checked: bool,
    }

    impl AssertOutputBeforeTailReader {
        /// Creates a reader that verifies earlier emission after its evidence prefix.
        fn new(data: Vec<u8>, expected_output: PathBuf) -> Self {
            Self {
                cursor: Cursor::new(data),
                expected_output,
                checked: false,
            }
        }
    }

    impl Read for AssertOutputBeforeTailReader {
        /// Verifies earlier outputs before reading beyond this source's evidence prefix.
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.cursor.position() >= 1027 && !self.checked {
                assert!(
                    self.expected_output.exists(),
                    "earlier prepared images should be emitted before the third payload tail"
                );
                self.checked = true;
            }
            self.cursor.read(buffer)
        }
    }

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

    /// Writes buffered test fixtures through the scoped production reader seam.
    fn write_sources(
        pipeline: &ImageWritePipeline,
        request: ImageWriteRequest<'_>,
        sources: Vec<(ArchiveImageSource, Vec<u8>)>,
    ) -> Result<ImageWriteResult> {
        pipeline.write_from(request, |visitor| {
            for (source, data) in sources {
                visitor.visit(source, &mut Cursor::new(data))?;
            }
            Ok(())
        })
    }

    /// Creates one named buffered source for a pipeline interface test.
    fn named_source(data: impl Into<Vec<u8>>, source_name: &str) -> (ArchiveImageSource, Vec<u8>) {
        (ArchiveImageSource::named(source_name), data.into())
    }

    /// Creates one MIME-labelled buffered source for a pipeline interface test.
    fn mime_source(
        data: impl Into<Vec<u8>>,
        source_name: &str,
        mime: &str,
    ) -> (ArchiveImageSource, Vec<u8>) {
        (
            ArchiveImageSource::named(source_name).with_mime(mime),
            data.into(),
        )
    }

    #[test]
    fn normal_images_are_discovered_and_written_through_pipeline_interface() {
        let temp_dir = temp_test_dir("normal-images");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![named_source(MINIMAL_PNG, "word/media/image.bin")],
        )
        .expect("normal image write should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.warnings.is_empty());
        assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), MINIMAL_PNG);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn rejected_source_reads_only_format_evidence_through_pipeline_interface() {
        let temp_dir = temp_test_dir("bounded-discovery");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let mut rejected_source = Cursor::new(vec![0; 4096]);

        let result = pipeline
            .write_from(
                ImageWriteRequest::normal_images(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::named("word/document.xml"),
                        &mut rejected_source,
                    )?;
                    Ok(())
                },
            )
            .expect("rejected source should be a normal pipeline outcome");

        assert_eq!(rejected_source.position(), 1027);
        assert_eq!(result.counts.extracted, 0);
        assert!(result.warnings.is_empty());
        assert!(!temp_dir.exists());
    }

    #[test]
    fn filtered_source_reads_only_format_evidence_through_pipeline_interface() {
        let temp_dir = temp_test_dir("bounded-format-filter");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let mut filtered_payload = vec![0; 4096];
        filtered_payload[..6].copy_from_slice(b"GIF89a");
        let mut filtered_source = Cursor::new(filtered_payload);

        let result = pipeline
            .write_from(
                ImageWriteRequest::normal_images(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::named("word/media/animation.gif"),
                        &mut filtered_source,
                    )?;
                    Ok(())
                },
            )
            .expect("filtered source should be a normal pipeline outcome");

        assert_eq!(filtered_source.position(), 1027);
        assert_eq!(result.counts.extracted, 0);
        assert!(result.warnings.is_empty());
        assert!(!temp_dir.exists());
    }

    #[test]
    fn tail_read_failure_warns_and_later_resource_keeps_singular_name() {
        let temp_dir = temp_test_dir("tail-read-failure");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let mut failing_payload = vec![0; 2048];
        failing_payload[..8].copy_from_slice(b"\x89PNG\r\n\x1A\n");
        let mut failing_source = FailAfterReader::new(failing_payload, 1100);
        let mut valid_source = Cursor::new(MINIMAL_PNG);

        let result = pipeline
            .write_from(
                ImageWriteRequest::normal_images(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::named("word/media/broken.png"),
                        &mut failing_source,
                    )?;
                    visitor.visit(
                        ArchiveImageSource::named("word/media/valid.png"),
                        &mut valid_source,
                    )?;
                    Ok(())
                },
            )
            .expect("a later readable source should still be emitted");

        assert_eq!(result.counts.extracted, 1);
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name,
                message,
            }] if source_name == "word/media/broken.png"
                && message.contains("injected archive resource failure")
        ));
        assert_eq!(fs::read(temp_dir.join("sample.png")).unwrap(), MINIMAL_PNG);
        assert!(!temp_dir.join("sample_1.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn bom_prefixed_svg_at_end_of_evidence_window_is_discovered() {
        let temp_dir = temp_test_dir("svg-evidence-window");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Svg]),
            None,
            None,
        ));
        let mut svg = b"\xEF\xBB\xBF".to_vec();
        svg.extend(std::iter::repeat_n(b' ', 1019));
        svg.extend_from_slice(b"<svg>");
        assert_eq!(svg.len(), 1027);

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![named_source(svg.clone(), "word/media/vector.bin")],
        )
        .expect("the full SVG evidence window should be inspected");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.warnings.is_empty());
        assert_eq!(fs::read(temp_dir.join("sample.svg")).unwrap(), svg);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn multiple_sources_keep_discovery_warnings_before_conversion_warnings() {
        let temp_dir = temp_test_dir("phase-ordered-warnings");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Svg, ImageFormat::Png]),
            ConversionTarget::Png,
            None,
        );

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![
                named_source(b"not really svg".as_slice(), "media/first.svg"),
                named_source(b"not really png".as_slice(), "media/second.png"),
            ],
        )
        .expect("both accepted sources should be emitted");

        assert_eq!(result.counts.extracted, 2);
        assert_eq!(result.counts.skipped, 1);
        assert_eq!(
            result.warnings,
            vec![
                ImageWriteWarning::ExtensionFallback {
                    source_name: "media/first.svg".to_string(),
                    format: ImageFormat::Svg,
                },
                ImageWriteWarning::ExtensionFallback {
                    source_name: "media/second.png".to_string(),
                    format: ImageFormat::Png,
                },
                ImageWriteWarning::ConversionSkipped {
                    base_name: "sample".to_string(),
                    format: ImageFormat::Svg,
                },
            ]
        );
        assert_eq!(
            fs::read(temp_dir.join("sample_1.svg")).unwrap(),
            b"not really svg"
        );
        assert_eq!(
            fs::read(temp_dir.join("sample_2.png")).unwrap(),
            b"not really png"
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn earlier_images_are_emitted_before_third_payload_is_fully_read() {
        let temp_dir = temp_test_dir("two-image-lookahead");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let mut first = Cursor::new(MINIMAL_PNG);
        let mut second = Cursor::new(MINIMAL_PNG);
        let mut third_payload = vec![0; 2048];
        third_payload[..8].copy_from_slice(b"\x89PNG\r\n\x1A\n");
        let mut third =
            AssertOutputBeforeTailReader::new(third_payload, temp_dir.join("sample_1.png"));

        let result = pipeline
            .write_from(
                ImageWriteRequest::normal_images(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(ArchiveImageSource::named("first.png"), &mut first)?;
                    visitor.visit(ArchiveImageSource::named("second.png"), &mut second)?;
                    visitor.visit(ArchiveImageSource::named("third.png"), &mut third)?;
                    Ok(())
                },
            )
            .expect("three images should be emitted incrementally");

        assert!(third.checked);
        assert_eq!(result.counts.extracted, 3);
        assert!(temp_dir.join("sample_1.png").exists());
        assert!(temp_dir.join("sample_2.png").exists());
        assert!(temp_dir.join("sample_3.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn concurrent_image_emissions_preserve_every_payload() {
        const WRITER_COUNT: usize = 32;

        let temp_dir = temp_test_dir("concurrent-emissions");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITER_COUNT));
        let mut expected_payloads = Vec::new();
        let mut writers = Vec::new();

        for writer_index in 0..WRITER_COUNT {
            let mut payload = MINIMAL_PNG.to_vec();
            payload.push(writer_index as u8);
            expected_payloads.push(payload.clone());

            let output_dir = temp_dir.clone();
            let barrier = barrier.clone();
            writers.push(std::thread::spawn(move || {
                let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
                    HashSet::from([ImageFormat::Png]),
                    None,
                    None,
                ));

                barrier.wait();
                write_sources(
                    &pipeline,
                    ImageWriteRequest::normal_images(&output_dir, "shared"),
                    vec![named_source(payload, "word/media/image.bin")],
                )
                .expect("concurrent image emission should succeed");
            }));
        }

        for writer in writers {
            writer.join().expect("concurrent writer should not panic");
        }

        let emitted_files: Vec<(String, Vec<u8>)> = fs::read_dir(&temp_dir)
            .expect("concurrent output directory should exist")
            .map(|entry| {
                let path = entry.expect("output entry should be readable").path();
                let name = path
                    .file_name()
                    .expect("emitted image should have a filename")
                    .to_string_lossy()
                    .to_string();
                let payload = fs::read(path).expect("emitted image should be readable");
                (name, payload)
            })
            .collect();
        let (mut emitted_names, mut emitted_payloads): (Vec<_>, Vec<_>) =
            emitted_files.into_iter().unzip();
        let mut expected_names = vec!["shared.png".to_string()];
        expected_names.extend((1..WRITER_COUNT).map(|index| format!("shared_{index}.png")));

        expected_names.sort();
        emitted_names.sort();
        expected_payloads.sort();
        emitted_payloads.sort();

        assert_eq!(emitted_names, expected_names);
        assert_eq!(emitted_payloads, expected_payloads);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn existing_output_is_preserved_and_uses_compatible_collision_suffix() {
        let temp_dir = temp_test_dir("existing-output");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        fs::write(temp_dir.join("shared.png"), b"existing")
            .expect("existing output should be writable");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "shared"),
            vec![named_source(MINIMAL_PNG, "word/media/image.bin")],
        )
        .expect("colliding image emission should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            fs::read(temp_dir.join("shared.png")).expect("existing output should remain readable"),
            b"existing"
        );
        assert_eq!(
            fs::read(temp_dir.join("shared_1.png"))
                .expect("collision-suffixed image should be readable"),
            MINIMAL_PNG
        );

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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![named_source(
                b"not actually a png".as_slice(),
                "word/media/image.png",
            )],
        )
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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![
                named_source(MINIMAL_PNG, "../media/image.png"),
                named_source(MINIMAL_PNG, "/media/image.png"),
                named_source(MINIMAL_PNG, "C:\\media\\image.png"),
                named_source(MINIMAL_PNG, "media/image.png::$DATA"),
                named_source(MINIMAL_PNG, "media/image\0.png"),
            ],
        )
        .expect("unsafe source should be skipped normally");

        assert_eq!(result.counts.extracted, 0);
        assert!(result.warnings.is_empty());
        assert!(!temp_dir.exists());
    }

    #[test]
    fn unsafe_source_is_rejected_before_its_reader_is_touched() {
        let temp_dir = temp_test_dir("unsafe-source-zero-read");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let mut source = Cursor::new(MINIMAL_PNG);

        let result = pipeline
            .write_from(
                ImageWriteRequest::normal_images(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(ArchiveImageSource::named("../image.png"), &mut source)?;
                    Ok(())
                },
            )
            .expect("unsafe source should be a silent normal outcome");

        assert_eq!(source.position(), 0);
        assert_eq!(result.counts.extracted, 0);
        assert!(result.warnings.is_empty());
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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![named_source(
                b"not really svg".as_slice(),
                "media/image.svg",
            )],
        )
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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![named_source(original.clone(), "image.png")],
        )
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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![named_source(original.clone(), "image.png")],
        )
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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&output_dir, "sample"),
            vec![named_source(b"GIF89a".as_slice(), "media/image.gif")],
        )
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

        let result = write_sources(
            &pipeline,
            ImageWriteRequest::normal_images(&temp_dir, "sample"),
            vec![mime_source(
                b"unknown bytes".as_slice(),
                "media/image.bin",
                "image/png",
            )],
        )
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
