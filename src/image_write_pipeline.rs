//! Image write pipeline for discovering and emitting archive images incrementally.

mod discovery;
mod emission;
mod purpose;

use anyhow::{Error, Result, anyhow};
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionOutcome, ConversionPolicy};
use crate::image_format::ImageFormat;

pub(crate) use self::discovery::ArchiveImageSource;
use self::discovery::{ArchiveImageDiscoveryOutcome, discover_image};
use self::emission::ImageFileEmission;
use self::purpose::{
    ConversionAction, ImageWritePurpose, NormalImages, RequiredCover, SourceEligibility,
};

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

    /// Returns whether the policy carries a Conversion policy.
    pub(crate) fn is_conversion_configured(&self) -> bool {
        self.conversion.is_some()
    }

    /// Returns the GIF destination when the policy routes GIFs separately.
    pub(crate) fn gif_destination(&self) -> Option<&Path> {
        self.gif_output.as_deref()
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
        detail: String,
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
        detail: String,
    },
    CoverConversionFailed {
        detail: String,
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
            detail: error.to_string(),
        }
    }
}

/// Complete observable outcome of one Image write pipeline invocation.
#[derive(Debug, Default)]
pub(crate) struct ImageWriteResult {
    pub(crate) counts: ImageWriteCounts,
    pub(crate) warnings: Vec<ImageWriteWarning>,
    has_normal_image_output: bool,
}

impl ImageWriteResult {
    /// Returns whether at least one normal batch image was emitted.
    pub(crate) fn has_normal_image_output(&self) -> bool {
        self.has_normal_image_output
    }

    /// Appends later Image write facts while preserving warning order.
    pub(crate) fn append(&mut self, mut later: Self) {
        self.counts.extracted += later.counts.extracted;
        self.counts.gifs_routed += later.counts.gifs_routed;
        self.counts.converted += later.counts.converted;
        self.counts.skipped += later.counts.skipped;
        self.has_normal_image_output |= later.has_normal_image_output;
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

    /// Forwards whether the bound Image write policy carries a Conversion policy.
    pub(crate) fn is_conversion_configured(&self) -> bool {
        self.policy.is_conversion_configured()
    }

    /// Forwards the bound Image write policy's GIF destination, if any.
    pub(crate) fn gif_destination(&self) -> Option<&Path> {
        self.policy.gif_destination()
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
        let mut visitor = RequiredCoverWriteVisitor::new(&self.policy, request, RequiredCover);
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
        let mut visitor = ArchiveImageVisitor::new(&self.policy, request, NormalImages);
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
    purpose: RequiredCover,
    disposition: Option<RequiredCoverWriteDisposition>,
    result: ImageWriteResult,
}

impl<'policy, 'request> RequiredCoverWriteVisitor<'policy, 'request> {
    /// Starts one required-cover attempt with no acquired source.
    fn new(
        policy: &'policy ImageWritePolicy,
        request: RequiredCoverWriteRequest<'request>,
        purpose: RequiredCover,
    ) -> Self {
        Self {
            policy,
            request,
            purpose,
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
        let discovered =
            discover_image(&source, reader, &self.policy.allowed_formats, &self.purpose);
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
            &self.purpose,
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
            &mut self.result.counts,
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
                source.diagnostic_name(),
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
    purpose: NormalImages,
    output_dir: &'request Path,
    base_name: &'request str,
    discovery_warnings: Vec<ImageWriteWarning>,
    conversion_warnings: Vec<ImageWriteWarning>,
    counts: ImageWriteCounts,
    has_normal_image_output: bool,
    pending_first: Option<PreparedImage>,
    multiple_emission: Option<ImageFileEmission<'request>>,
}

impl<'policy, 'request> ArchiveImageVisitor<'policy, 'request> {
    /// Starts one scoped Archive image discovery traversal.
    fn new(
        policy: &'policy ImageWritePolicy,
        request: ImageWriteRequest<'request>,
        purpose: NormalImages,
    ) -> Self {
        Self {
            policy,
            purpose,
            output_dir: request.output_dir,
            base_name: request.base_name,
            discovery_warnings: Vec::new(),
            conversion_warnings: Vec::new(),
            counts: ImageWriteCounts::default(),
            has_normal_image_output: false,
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
        let discovered =
            discover_image(&source, reader, &self.policy.allowed_formats, &self.purpose);
        self.discovery_warnings.extend(discovered.warnings);

        let ArchiveImageDiscoveryOutcome::Accepted(image) = discovered.outcome else {
            return Ok(());
        };
        let Some(prepared) = prepare_image_for_write(
            image,
            self.base_name,
            self.policy,
            &self.purpose,
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
        if matches!(
            self.purpose.source_eligibility(&source),
            SourceEligibility::Inspect
        ) {
            self.discovery_warnings
                .push(ImageWriteWarning::archive_image_acquisition_failed(
                    source.diagnostic_name(),
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
            &mut self.counts,
        )?;
        self.has_normal_image_output = true;
        Ok(())
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
            has_normal_image_output: self.has_normal_image_output,
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

/// Applies one statically selected purpose's conversion decision before a file write.
///
/// Conversion warning facts are appended in accepted-source order. `None` is
/// returned only when required-cover conversion completes without emission.
fn prepare_image_for_write<P: ImageWritePurpose>(
    image: AcceptedImage,
    base_name: &str,
    policy: &ImageWritePolicy,
    purpose: &P,
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

        let (original_format, decision) = match conversion.convert(&image.data, image.format) {
            Ok(ConversionOutcome::Converted(converted_bytes, format)) => {
                return Some(PreparedImage {
                    data: converted_bytes,
                    format,
                    routed_gif: false,
                    converted: true,
                    skipped_conversion: false,
                });
            }
            Ok(ConversionOutcome::PreservedMatchingSource) => {
                return Some(PreparedImage {
                    data: image.data,
                    format: image.format,
                    routed_gif: false,
                    converted: false,
                    skipped_conversion: false,
                });
            }
            Ok(ConversionOutcome::UnsupportedSource(original_format)) => (
                original_format,
                purpose.unsupported_conversion(base_name, original_format),
            ),
            Err(error) => (image.format, purpose.failed_conversion(base_name, &error)),
        };

        if let Some(warning) = decision.warning {
            warnings.push(warning);
        }
        match decision.action {
            ConversionAction::PreserveOriginal => Some(PreparedImage {
                data: image.data,
                format: original_format,
                routed_gif: false,
                converted: false,
                skipped_conversion: true,
            }),
            ConversionAction::CompleteWithoutEmission => None,
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
    counts: &mut ImageWriteCounts,
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

    Ok(())
}

#[cfg(test)]
mod tests;
