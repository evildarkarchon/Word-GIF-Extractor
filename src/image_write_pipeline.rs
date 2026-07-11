//! Image write pipeline for discovering and emitting archive images incrementally.

mod discovery;
mod emission;

use anyhow::{Error, Result, anyhow};
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionOutcome, ConversionPolicy};
use crate::image_format::ImageFormat;

use self::discovery::{
    ArchiveImageDiscoveryOutcome, discover_image, discover_required_cover, is_source_safe,
};
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

    /// Creates a required-cover source whose path is diagnostic identity, not format evidence.
    ///
    /// EPUB covers intentionally use byte evidence before MIME and never fall back
    /// to a manifest path extension.
    pub(crate) fn required_cover(source_name: impl Into<String>, mime: impl Into<String>) -> Self {
        Self {
            diagnostic_name: source_name.into(),
            format_source_name: None,
            mime: Some(mime.into()),
        }
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

/// Counts successfully emitted files by their Image write purpose.
#[derive(Debug, Default, Clone, Copy)]
struct ImageWritePurposeCounts {
    normal_images: usize,
    required_covers: usize,
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
    purpose_counts: ImageWritePurposeCounts,
}

impl ImageWriteResult {
    /// Returns whether at least one normal batch image was emitted.
    pub(crate) fn has_normal_image_output(&self) -> bool {
        self.purpose_counts.normal_images > 0
    }

    /// Appends later Image write facts while preserving warning order.
    pub(crate) fn append(&mut self, mut later: Self) {
        self.counts.extracted += later.counts.extracted;
        self.counts.gifs_routed += later.counts.gifs_routed;
        self.counts.converted += later.counts.converted;
        self.counts.skipped += later.counts.skipped;
        self.purpose_counts.normal_images += later.purpose_counts.normal_images;
        self.purpose_counts.required_covers += later.purpose_counts.required_covers;
        self.warnings.append(&mut later.warnings);
    }
}

/// Document-local Image write failure with facts retained before the error.
#[derive(Debug)]
pub(crate) struct ImageWriteFailure {
    pub(crate) partial: ImageWriteResult,
    pub(crate) error: Error,
}

impl ImageWriteFailure {
    /// Creates a failure before any Image write facts have been produced.
    pub(crate) fn empty(error: impl Into<Error>) -> Self {
        Self {
            partial: ImageWriteResult::default(),
            error: error.into(),
        }
    }

    /// Prepends facts from earlier attempts to this failure.
    pub(crate) fn prepend(&mut self, mut earlier: ImageWriteResult) {
        earlier.append(std::mem::take(&mut self.partial));
        self.partial = earlier;
    }
}

impl From<Error> for ImageWriteFailure {
    fn from(error: Error) -> Self {
        Self::empty(error)
    }
}

impl fmt::Display for ImageWriteFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for ImageWriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

/// Result of an Image write operation that retains partial facts on failure.
pub(crate) type ImageWriteOutcome<T = ImageWriteResult> = std::result::Result<T, ImageWriteFailure>;

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

/// Private policy branch shared by the two Image write pipeline operations.
#[derive(Clone, Copy)]
pub(super) enum ImageWritePurpose {
    NormalImages,
    RequiredCover,
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

/// Output facts for one required EPUB cover attempt.
pub(crate) struct RequiredCoverWriteRequest<'a> {
    output_dir: &'a Path,
    base_name: &'a str,
}

impl<'a> RequiredCoverWriteRequest<'a> {
    /// Creates a required-cover request with singular output naming.
    pub(crate) fn new(output_dir: &'a Path, base_name: &'a str) -> Self {
        Self {
            output_dir,
            base_name,
        }
    }
}

/// Completion disposition for one required-cover candidate.
#[derive(Debug)]
pub(crate) enum RequiredCoverWriteOutcome {
    /// Resource acquisition failed, so EPUB cover extraction may try another candidate.
    Retry(ImageWriteResult),
    /// Image write policy reached a final emitting or non-emitting cover outcome.
    Completed(ImageWriteResult),
}

#[derive(Debug, Clone, Copy)]
enum RequiredCoverWriteDisposition {
    Retry,
    Completed,
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

    /// Discovers and writes one required EPUB cover through a scoped source reader.
    ///
    /// Acquisition failures return a retry disposition. Filtering and other Image
    /// write policy decisions complete the attempt, while emission failures retain
    /// facts accumulated before returning the error.
    pub(crate) fn write_required_cover(
        &self,
        request: RequiredCoverWriteRequest<'_>,
        traverse: impl FnOnce(&mut RequiredCoverWriteVisitor<'_, '_>) -> Result<()>,
    ) -> ImageWriteOutcome<RequiredCoverWriteOutcome> {
        let mut visitor = RequiredCoverWriteVisitor::new(&self.policy, request);
        if let Err(error) = traverse(&mut visitor) {
            return Err(visitor.into_failure(error));
        }
        visitor.finish()
    }

    /// Discovers, prepares, and writes sources supplied through one scoped traversal.
    ///
    /// The traversal must finish each reader before opening the next archive entry.
    /// Per-resource acquisition failures belong to the visitor and remain non-fatal;
    /// an error returned by the traversal aborts the document.
    ///
    /// Returns phase-ordered warning facts and counts for files actually written.
    /// Filesystem setup, collision exhaustion, create, write, and flush failures
    /// retain those facts with the error; earlier successful writes are not rolled back.
    pub(crate) fn write_from(
        &self,
        request: ImageWriteRequest<'_>,
        traverse: impl FnOnce(&mut ArchiveImageVisitor<'_, '_>) -> Result<()>,
    ) -> ImageWriteOutcome {
        let mut visitor = ArchiveImageVisitor::new(&self.policy, request);
        if let Err(error) = traverse(&mut visitor) {
            return Err(visitor.into_failure(error));
        }
        visitor.finish()
    }
}

/// Scoped authority for one required-cover acquisition and Image write decision.
pub(crate) struct RequiredCoverWriteVisitor<'policy, 'request> {
    policy: &'policy ImageWritePolicy,
    request: RequiredCoverWriteRequest<'request>,
    disposition: Option<RequiredCoverWriteDisposition>,
    result: ImageWriteResult,
}

impl<'policy, 'request> RequiredCoverWriteVisitor<'policy, 'request> {
    /// Starts one required-cover attempt with no acquired source.
    fn new(
        policy: &'policy ImageWritePolicy,
        request: RequiredCoverWriteRequest<'request>,
    ) -> Self {
        Self {
            policy,
            request,
            disposition: None,
            result: ImageWriteResult::default(),
        }
    }

    /// Consumes one scoped cover reader and applies required-cover Image write policy.
    ///
    /// Bounded evidence is read before the remaining payload. Read failures are
    /// retryable; emission failures are returned to abort the document.
    pub(crate) fn visit(
        &mut self,
        source: ArchiveImageSource,
        reader: &mut dyn Read,
    ) -> Result<()> {
        self.ensure_empty()?;
        let discovered = discover_required_cover(&source, reader, &self.policy.allowed_formats);
        self.result.warnings.extend(discovered.warnings);
        let image = match discovered.outcome {
            ArchiveImageDiscoveryOutcome::Accepted(image) => image,
            ArchiveImageDiscoveryOutcome::Completed => {
                self.disposition = Some(RequiredCoverWriteDisposition::Completed);
                return Ok(());
            }
            ArchiveImageDiscoveryOutcome::AcquisitionFailed => {
                self.disposition = Some(RequiredCoverWriteDisposition::Retry);
                return Ok(());
            }
        };

        let Some(prepared) = prepare_image_for_write(
            image,
            self.request.base_name,
            self.policy,
            ImageWritePurpose::RequiredCover,
            &mut self.result.warnings,
        ) else {
            self.disposition = Some(RequiredCoverWriteDisposition::Completed);
            return Ok(());
        };
        let mut emission = ImageFileEmission::new(self.request.base_name, false);
        emit_prepared_image(
            self.policy,
            self.request.output_dir,
            &mut emission,
            prepared,
            ImageWritePurpose::RequiredCover,
            &mut self.result.counts,
            &mut self.result.purpose_counts,
        )?;
        self.disposition = Some(RequiredCoverWriteDisposition::Completed);
        Ok(())
    }

    /// Records a candidate that the EPUB adapter could not open.
    pub(crate) fn unreadable(
        &mut self,
        source: ArchiveImageSource,
        error: impl fmt::Display,
    ) -> Result<()> {
        self.ensure_empty()?;
        self.result
            .warnings
            .push(ImageWriteWarning::archive_image_acquisition_failed(
                source.diagnostic_name,
                error,
            ));
        self.disposition = Some(RequiredCoverWriteDisposition::Retry);
        Ok(())
    }

    /// Returns an error when a traversal attempts to supply more than one cover source.
    fn ensure_empty(&self) -> Result<()> {
        if self.disposition.is_some() {
            return Err(anyhow!(
                "required-cover traversal supplied more than one source"
            ));
        }
        Ok(())
    }

    /// Completes the required-cover traversal after exactly one source attempt.
    fn finish(self) -> ImageWriteOutcome<RequiredCoverWriteOutcome> {
        match self.disposition {
            Some(RequiredCoverWriteDisposition::Retry) => {
                Ok(RequiredCoverWriteOutcome::Retry(self.result))
            }
            Some(RequiredCoverWriteDisposition::Completed) => {
                Ok(RequiredCoverWriteOutcome::Completed(self.result))
            }
            None => Err(self.into_failure(anyhow!("required-cover traversal supplied no source"))),
        }
    }

    /// Retains facts accumulated before a required-cover failure.
    fn into_failure(self, error: Error) -> ImageWriteFailure {
        ImageWriteFailure {
            partial: self.result,
            error,
        }
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
    purpose_counts: ImageWritePurposeCounts,
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
            purpose_counts: ImageWritePurposeCounts::default(),
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

        let ArchiveImageDiscoveryOutcome::Accepted(image) = discovered.outcome else {
            return Ok(());
        };
        let Some(prepared) = prepare_image_for_write(
            image,
            self.base_name,
            self.policy,
            ImageWritePurpose::NormalImages,
            &mut self.conversion_warnings,
        ) else {
            unreachable!("normal-image preparation always preserves accepted bytes");
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
        emit_prepared_image(
            self.policy,
            self.output_dir,
            emission,
            prepared,
            ImageWritePurpose::NormalImages,
            &mut self.counts,
            &mut self.purpose_counts,
        )
    }

    /// Completes singular lookahead and returns phase-ordered warning facts.
    ///
    /// Returns an error if the lone pending image cannot be emitted.
    fn finish(mut self) -> ImageWriteOutcome {
        if let Some(prepared) = self.pending_first.take() {
            let mut emission = ImageFileEmission::new(self.base_name, false);
            if let Err(error) = self.emit_prepared(&mut emission, prepared) {
                return Err(self.into_failure(error));
            }
        }

        Ok(self.into_result())
    }

    /// Collects phase-ordered facts after traversal succeeds.
    fn into_result(self) -> ImageWriteResult {
        ImageWriteResult {
            counts: self.counts,
            warnings: self
                .discovery_warnings
                .into_iter()
                .chain(self.conversion_warnings)
                .collect(),
            purpose_counts: self.purpose_counts,
        }
    }

    /// Retains facts accumulated before a traversal or emission failure.
    fn into_failure(self, error: Error) -> ImageWriteFailure {
        ImageWriteFailure {
            partial: self.into_result(),
            error,
        }
    }
}

/// Applies purpose-specific conversion semantics before one file write.
///
/// `purpose` selects normal preservation versus final non-emitting cover outcomes.
/// Conversion warning facts are appended in accepted-source order. `None` is
/// returned only when required-cover conversion completes without emission.
fn prepare_image_for_write(
    image: AcceptedImage,
    base_name: &str,
    policy: &ImageWritePolicy,
    purpose: ImageWritePurpose,
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
                match purpose {
                    ImageWritePurpose::NormalImages => {
                        warnings.push(ImageWriteWarning::ConversionSkipped {
                            base_name: base_name.to_string(),
                            format: original_format,
                        });
                    }
                    ImageWritePurpose::RequiredCover => {
                        warnings.push(ImageWriteWarning::CoverConversionSkipped {
                            format: original_format,
                        });
                        return None;
                    }
                }
                Some(PreparedImage {
                    data: image.data,
                    format: original_format,
                    routed_gif: false,
                    converted: false,
                    skipped_conversion: true,
                })
            }
            Err(error) => {
                match purpose {
                    ImageWritePurpose::NormalImages => {
                        warnings.push(ImageWriteWarning::ConversionFailed {
                            base_name: base_name.to_string(),
                            message: error.to_string(),
                        });
                    }
                    ImageWritePurpose::RequiredCover => {
                        warnings.push(ImageWriteWarning::CoverConversionFailed {
                            message: error.to_string(),
                        });
                        return None;
                    }
                }
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

/// Emits one prepared image using shared destination routing and count semantics.
fn emit_prepared_image(
    policy: &ImageWritePolicy,
    output_dir: &Path,
    emission: &mut ImageFileEmission<'_>,
    prepared: PreparedImage,
    purpose: ImageWritePurpose,
    counts: &mut ImageWriteCounts,
    purpose_counts: &mut ImageWritePurposeCounts,
) -> Result<()> {
    let destination = if prepared.routed_gif {
        // Preparation only marks routing when the immutable policy has a destination.
        policy
            .gif_output
            .as_deref()
            .expect("routed GIF should have a configured destination")
    } else {
        output_dir
    };
    emission.emit(destination, prepared.format, &prepared.data)?;

    counts.extracted += 1;
    if prepared.routed_gif {
        counts.gifs_routed += 1;
    }
    if prepared.converted {
        counts.converted += 1;
    }
    if prepared.skipped_conversion {
        counts.skipped += 1;
    }
    match purpose {
        ImageWritePurpose::NormalImages => purpose_counts.normal_images += 1,
        ImageWritePurpose::RequiredCover => purpose_counts.required_covers += 1,
    }

    Ok(())
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
    ) -> ImageWriteOutcome {
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
    fn required_cover_defaults_unidentified_evidence_to_jpeg_and_emits_it() {
        let temp_dir = temp_test_dir("required-cover-default-jpeg");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
            None,
            None,
        ));
        let original = b"unidentified cover payload".to_vec();
        let mut reader = Cursor::new(original.clone());

        let outcome = pipeline
            .write_required_cover(
                RequiredCoverWriteRequest::new(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::required_cover(
                            "OPS/cover.bin",
                            "application/octet-stream",
                        ),
                        &mut reader,
                    )
                },
            )
            .expect("required cover write should succeed");

        let RequiredCoverWriteOutcome::Completed(result) = outcome else {
            panic!("a readable required cover should complete the cover decision");
        };
        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::CoverDefaultToJpeg {
                mime: "application/octet-stream".to_string(),
            }]
        );
        assert_eq!(fs::read(temp_dir.join("sample.jpg")).unwrap(), original);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn required_cover_filter_is_final_and_reads_only_bounded_evidence() {
        let temp_dir = temp_test_dir("required-cover-filter");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let mut reader = Cursor::new(vec![0xff; 4096]);
        reader.get_mut()[..3].copy_from_slice(b"\xFF\xD8\xFF");

        let outcome = pipeline
            .write_required_cover(
                RequiredCoverWriteRequest::new(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::required_cover("OPS/cover.jpg", "image/jpeg"),
                        &mut reader,
                    )
                },
            )
            .expect("format filtering should be a normal cover outcome");

        let RequiredCoverWriteOutcome::Completed(result) = outcome else {
            panic!("a filtered cover should complete rather than retry");
        };
        assert_eq!(reader.position(), 1027);
        assert_eq!(result.counts.extracted, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::UnsupportedCoverFormat {
                format: ImageFormat::Jpg,
            }]
        );
        assert!(!temp_dir.exists());
    }

    #[test]
    fn required_cover_tail_read_failure_is_retryable_and_preserves_warning_order() {
        let temp_dir = temp_test_dir("required-cover-tail-failure");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
            None,
            None,
        ));
        let mut reader = FailAfterReader::new(vec![0; 2048], 1100);

        let outcome = pipeline
            .write_required_cover(
                RequiredCoverWriteRequest::new(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::required_cover(
                            "OPS/cover.bin",
                            "application/octet-stream",
                        ),
                        &mut reader,
                    )
                },
            )
            .expect("a cover acquisition failure should be a typed outcome");

        let RequiredCoverWriteOutcome::Retry(result) = outcome else {
            panic!("a tail read failure should permit another cover candidate");
        };
        assert_eq!(result.counts.extracted, 0);
        assert!(matches!(
            &result.warnings[..],
            [
                ImageWriteWarning::CoverDefaultToJpeg { mime },
                ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, message }
            ] if mime == "application/octet-stream"
                && source_name == "OPS/cover.bin"
                && message.contains("injected archive resource failure")
        ));
        assert!(!temp_dir.exists());
    }

    #[test]
    fn required_cover_conversion_skip_is_final_and_writes_nothing() {
        let temp_dir = temp_test_dir("required-cover-conversion-skip");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Svg]),
            ConversionTarget::Png,
            None,
        );
        let mut reader = Cursor::new(b"<svg/>".to_vec());

        let outcome = pipeline
            .write_required_cover(
                RequiredCoverWriteRequest::new(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::required_cover("OPS/cover.svg", "image/svg+xml"),
                        &mut reader,
                    )
                },
            )
            .expect("unsupported cover conversion should be a normal outcome");

        let RequiredCoverWriteOutcome::Completed(result) = outcome else {
            panic!("conversion skip should complete rather than retry");
        };
        assert_eq!(result.counts.extracted, 0);
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
    fn required_cover_conversion_failure_is_final_and_writes_nothing() {
        let temp_dir = temp_test_dir("required-cover-conversion-failure");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Png]),
            ConversionTarget::Jpg,
            None,
        );
        let mut reader = Cursor::new(b"\x89PNG\r\n\x1A\ninvalid".to_vec());

        let outcome = pipeline
            .write_required_cover(
                RequiredCoverWriteRequest::new(&temp_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::required_cover("OPS/cover.png", "image/png"),
                        &mut reader,
                    )
                },
            )
            .expect("cover conversion failure should be a normal outcome");

        let RequiredCoverWriteOutcome::Completed(result) = outcome else {
            panic!("conversion failure should complete rather than retry");
        };
        assert_eq!(result.counts.extracted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::CoverConversionFailed { message }]
                if message.contains("Failed to decode image")
        ));
        assert!(!temp_dir.exists());
    }

    #[test]
    fn required_gif_cover_routes_without_conversion() {
        let temp_dir = temp_test_dir("required-cover-gif-routing");
        let gif_dir = temp_dir.join("gifs");
        let output_dir = temp_dir.join("images");
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Gif]),
            ConversionTarget::Png,
            Some(gif_dir.clone()),
        );
        let mut reader = Cursor::new(b"GIF89a".to_vec());

        let outcome = pipeline
            .write_required_cover(
                RequiredCoverWriteRequest::new(&output_dir, "sample"),
                |visitor| {
                    visitor.visit(
                        ArchiveImageSource::required_cover("OPS/cover.gif", "image/gif"),
                        &mut reader,
                    )
                },
            )
            .expect("routed required GIF should be emitted");

        let RequiredCoverWriteOutcome::Completed(result) = outcome else {
            panic!("a routed cover should complete");
        };
        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.gifs_routed, 1);
        assert_eq!(result.counts.converted, 0);
        assert!(result.warnings.is_empty());
        assert_eq!(fs::read(gif_dir.join("sample.gif")).unwrap(), b"GIF89a");
        assert!(!output_dir.exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
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
