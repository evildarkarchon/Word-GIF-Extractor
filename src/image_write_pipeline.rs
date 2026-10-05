//! Image write pipeline for discovering and emitting archive images incrementally.

mod discovery;
mod emission;
mod purpose;

use anyhow::{Error, Result};
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionOutcome, ConversionPolicy};
use crate::image_format::ImageFormat;
use crate::output_placement::OutputPlacement;

pub(crate) use self::discovery::ArchiveImageSource;
use self::discovery::{ArchiveImageDiscoveryOutcome, discover_image};
use self::emission::ImageFileEmission;
use self::purpose::{ImageWritePurpose, NormalImages, RequiredCover, SourceEligibility};

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

/// Whether an Image write pipeline invocation emitted any normal batch image.
///
/// Required-cover output is not normal image output, so a completed cover attempt
/// reports `Absent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalImageOutput {
    Present,
    Absent,
}

/// Complete observable outcome of one Image write pipeline invocation.
#[derive(Debug, Default)]
pub(crate) struct ImageWriteResult {
    pub(crate) counts: ImageWriteCounts,
    pub(crate) warnings: Vec<ImageWriteWarning>,
    has_normal_image_output: bool,
}

impl ImageWriteResult {
    /// Creates one complete Image write pipeline outcome from already-produced facts.
    ///
    /// `normal_image_output` supplies the normal-batch emission flag, which is
    /// otherwise readable only through [`Self::has_normal_image_output`]. Taking it
    /// as a parameter is what keeps that field private: a complete outcome can be
    /// built anywhere in the crate without the field becoming crate-visible.
    pub(crate) fn new(
        counts: ImageWriteCounts,
        warnings: Vec<ImageWriteWarning>,
        normal_image_output: NormalImageOutput,
    ) -> Self {
        Self {
            counts,
            warnings,
            has_normal_image_output: normal_image_output == NormalImageOutput::Present,
        }
    }

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

#[derive(Debug, PartialEq)]
struct AcceptedImage {
    data: Vec<u8>,
    format: ImageFormat,
}

/// The role one image takes as the Image write pipeline emits it.
///
/// Exactly one role applies per emitted image, which is what keeps the
/// converted, conversion-skipped and GIF-routed counts from together exceeding
/// the emitted count. `RoutedGif` carries the destination read from the Image
/// write policy at the moment routing was decided, so a routed image and the
/// destination that justifies it cannot be decided apart.
#[derive(Debug, Clone, Copy)]
enum EmittedImageRole<'policy> {
    /// A GIF the Image write policy sends to its own destination, unconverted.
    RoutedGif(&'policy Path),
    /// An image the Conversion policy re-encoded to its target.
    Converted,
    /// An image conversion could not take, emitted in its original bytes.
    ConversionSkipped,
    /// An image emitted as extracted, because no Conversion policy was
    /// requested or because conversion preserved an already-matching source.
    /// Neither is counted, and the applicable warning fact carries which.
    Preserved,
}

struct PreparedImage<'policy> {
    data: Vec<u8>,
    format: ImageFormat,
    role: EmittedImageRole<'policy>,
}

/// Purpose-free result of applying Image write policy to one accepted image.
///
/// Preparation reports a conversion fallback instead of resolving it, because
/// what a fallback means depends on the Image write purpose: a normal image is
/// still emitted in its original bytes, while a required cover is not emitted at
/// all. Each visitor already knows its purpose, so each resolves the fallback
/// itself and preparation never has to return a case its caller cannot reach.
enum ImagePreparation<'policy> {
    /// The image is ready to emit in the role preparation decided.
    Prepared(PreparedImage<'policy>),
    /// The Conversion policy could not produce requested bytes; the original
    /// bytes and the format to emit them under are returned unchanged.
    ConversionFellBack {
        data: Vec<u8>,
        format: ImageFormat,
        reason: ConversionFallbackReason,
    },
}

/// Why the Conversion policy fell back to an image's original bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConversionFallbackReason {
    /// The source format cannot be decoded for conversion.
    Unsupported,
    /// Conversion was attempted and failed, with the error's detail.
    Failed(String),
}

/// Completion disposition for one required-cover candidate.
#[derive(Debug)]
pub(crate) enum RequiredCoverWriteOutcome {
    /// Resource acquisition failed, so EPUB cover extraction may try another candidate.
    Retry(ImageWriteResult),
    /// Image write policy reached a final emitting or non-emitting cover outcome.
    Completed(ImageWriteResult),
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

    /// Discovers and writes one required EPUB cover from its scoped payload reader.
    ///
    /// Single-shot, unlike [`Self::write_from`]: a cover attempt has exactly one
    /// source, and normal images need a traversal only because their singular versus
    /// multiple naming waits on the next source. Taking the reader directly lets the
    /// signature say "one" where a traversal had to check it at runtime. The source
    /// facts are raw so the pipeline builds the cover's evidence itself, and a caller
    /// cannot hand it a normal source that would enable path-extension fallback.
    ///
    /// An emitted cover is named singularly after the placement's base name, since a
    /// cover attempt never has a second image to number against.
    ///
    /// Read failures return `Retry`, so EPUB cover extraction may try its next
    /// candidate. Filtering, conversion fallback and successful emission return
    /// `Completed`. Emission failures retain the facts accumulated before them.
    pub(crate) fn write_required_cover(
        &self,
        placement: &OutputPlacement,
        manifest_path: &str,
        mime: &str,
        reader: &mut dyn Read,
    ) -> ImageWriteOutcome<RequiredCoverWriteOutcome> {
        let source = ArchiveImageSource::required_cover(manifest_path, mime);
        let discovered = discover_image(
            &source,
            reader,
            &self.policy.allowed_formats,
            &RequiredCover,
        );
        let mut result = ImageWriteResult::default();
        result.warnings.extend(discovered.warnings);
        let image = match discovered.outcome {
            ArchiveImageDiscoveryOutcome::Accepted(image) => image,
            ArchiveImageDiscoveryOutcome::Completed => {
                return Ok(RequiredCoverWriteOutcome::Completed(result));
            }
            ArchiveImageDiscoveryOutcome::AcquisitionFailed => {
                return Ok(RequiredCoverWriteOutcome::Retry(result));
            }
        };

        let prepared = match prepare_image_for_write(image, &self.policy) {
            ImagePreparation::Prepared(prepared) => prepared,
            // A required cover is never emitted in bytes the Conversion policy could
            // not produce; the attempt completes with the cover-specific warning.
            ImagePreparation::ConversionFellBack { format, reason, .. } => {
                result.warnings.push(match reason {
                    ConversionFallbackReason::Unsupported => {
                        ImageWriteWarning::CoverConversionSkipped { format }
                    }
                    ConversionFallbackReason::Failed(detail) => {
                        ImageWriteWarning::CoverConversionFailed { detail }
                    }
                });
                return Ok(RequiredCoverWriteOutcome::Completed(result));
            }
        };
        let mut emission = ImageFileEmission::new(placement.base_name(), false);
        if let Err(error) = emit_prepared_image(
            placement.output_dir(),
            &mut emission,
            prepared,
            &mut result.counts,
        ) {
            return Err(ImageWriteFailure {
                partial: result,
                error,
            });
        }
        Ok(RequiredCoverWriteOutcome::Completed(result))
    }

    /// Records a required cover whose payload the EPUB adapter could not acquire.
    ///
    /// Always `Retry`: an unavailable candidate settles nothing about the cover, so
    /// EPUB cover extraction may try its next candidate.
    pub(crate) fn required_cover_unavailable(
        &self,
        manifest_path: &str,
        error: impl fmt::Display,
    ) -> RequiredCoverWriteOutcome {
        let mut result = ImageWriteResult::default();
        result
            .warnings
            .push(ImageWriteWarning::archive_image_acquisition_failed(
                manifest_path,
                error,
            ));
        RequiredCoverWriteOutcome::Retry(result)
    }

    /// Discovers, prepares, and writes sources supplied through one scoped traversal.
    ///
    /// Sources are numbered in the order the traversal visits them, which is the
    /// document's own order, under the placement's directory and base name.
    /// The traversal must finish each reader before opening the next archive entry.
    /// Per-resource acquisition failures belong to the visitor and remain non-fatal;
    /// an error returned by the traversal aborts the document.
    ///
    /// Returns phase-ordered warning facts and counts for files actually written.
    /// Filesystem setup, collision exhaustion, create, write, and flush failures
    /// retain those facts with the error; earlier successful writes are not rolled back.
    pub(crate) fn write_from(
        &self,
        placement: &OutputPlacement,
        traverse: impl FnOnce(&mut ArchiveImageVisitor<'_, '_>) -> Result<()>,
    ) -> ImageWriteOutcome {
        let mut visitor = ArchiveImageVisitor::new(&self.policy, placement, NormalImages);
        if let Err(error) = traverse(&mut visitor) {
            return Err(visitor.into_failure(error));
        }
        visitor.finish()
    }
}

/// Scoped authority for per-resource discovery, preparation, and ordered emission.
pub(crate) struct ArchiveImageVisitor<'policy, 'placement> {
    policy: &'policy ImageWritePolicy,
    purpose: NormalImages,
    output_dir: &'placement Path,
    base_name: &'placement str,
    discovery_warnings: Vec<ImageWriteWarning>,
    conversion_warnings: Vec<ImageWriteWarning>,
    counts: ImageWriteCounts,
    normal_image_output: NormalImageOutput,
    pending_first: Option<PreparedImage<'policy>>,
    multiple_emission: Option<ImageFileEmission<'placement>>,
}

impl<'policy, 'placement> ArchiveImageVisitor<'policy, 'placement> {
    /// Starts one scoped Archive image discovery traversal.
    fn new(
        policy: &'policy ImageWritePolicy,
        placement: &'placement OutputPlacement,
        purpose: NormalImages,
    ) -> Self {
        Self {
            policy,
            purpose,
            output_dir: placement.output_dir(),
            base_name: placement.base_name(),
            discovery_warnings: Vec::new(),
            conversion_warnings: Vec::new(),
            counts: ImageWriteCounts::default(),
            normal_image_output: NormalImageOutput::Absent,
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
        let prepared = match prepare_image_for_write(image, self.policy) {
            ImagePreparation::Prepared(prepared) => prepared,
            // A normal image is still emitted when conversion falls back: its
            // original bytes go out as a conversion-skipped image with a warning.
            ImagePreparation::ConversionFellBack {
                data,
                format,
                reason,
            } => {
                let base_name = self.base_name.to_string();
                self.conversion_warnings.push(match reason {
                    ConversionFallbackReason::Unsupported => {
                        ImageWriteWarning::ConversionSkipped { base_name, format }
                    }
                    ConversionFallbackReason::Failed(detail) => {
                        ImageWriteWarning::ConversionFailed { base_name, detail }
                    }
                });
                PreparedImage {
                    data,
                    format,
                    role: EmittedImageRole::ConversionSkipped,
                }
            }
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
    fn stage_prepared(&mut self, prepared: PreparedImage<'policy>) -> Result<()> {
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
        prepared: PreparedImage<'policy>,
    ) -> Result<()> {
        emit_prepared_image(self.output_dir, emission, prepared, &mut self.counts)?;
        self.normal_image_output = NormalImageOutput::Present;
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
        ImageWriteResult::new(
            self.counts,
            self.discovery_warnings
                .into_iter()
                .chain(self.conversion_warnings)
                .collect(),
            self.normal_image_output,
        )
    }

    /// Retains facts accumulated before a traversal or emission failure.
    fn into_failure(self, error: Error) -> ImageWriteFailure {
        ImageWriteFailure {
            partial: self.into_result(),
            error,
        }
    }
}

/// Applies Image write policy's GIF routing and conversion to one accepted image.
///
/// Purpose-free: a conversion that cannot produce requested bytes is reported as
/// [`ImagePreparation::ConversionFellBack`] for the calling visitor to resolve,
/// and no warning fact is produced here.
fn prepare_image_for_write(
    image: AcceptedImage,
    policy: &ImageWritePolicy,
) -> ImagePreparation<'_> {
    // Routing is decided together with the destination it needs, so emission
    // never has to ask the policy a second question it could answer differently.
    let routed_destination = if image.format == ImageFormat::Gif {
        policy.gif_destination()
    } else {
        None
    };

    if let Some(conversion) = &policy.conversion {
        if let Some(destination) = routed_destination {
            return ImagePreparation::Prepared(PreparedImage {
                data: image.data,
                format: image.format,
                role: EmittedImageRole::RoutedGif(destination),
            });
        }

        match conversion.convert(&image.data, image.format) {
            Ok(ConversionOutcome::Converted(converted_bytes, format)) => {
                ImagePreparation::Prepared(PreparedImage {
                    data: converted_bytes,
                    format,
                    role: EmittedImageRole::Converted,
                })
            }
            Ok(ConversionOutcome::PreservedMatchingSource) => {
                ImagePreparation::Prepared(PreparedImage {
                    data: image.data,
                    format: image.format,
                    role: EmittedImageRole::Preserved,
                })
            }
            Ok(ConversionOutcome::UnsupportedSource(original_format)) => {
                ImagePreparation::ConversionFellBack {
                    data: image.data,
                    format: original_format,
                    reason: ConversionFallbackReason::Unsupported,
                }
            }
            Err(error) => ImagePreparation::ConversionFellBack {
                data: image.data,
                format: image.format,
                reason: ConversionFallbackReason::Failed(error.to_string()),
            },
        }
    } else {
        ImagePreparation::Prepared(PreparedImage {
            data: image.data,
            format: image.format,
            role: match routed_destination {
                Some(destination) => EmittedImageRole::RoutedGif(destination),
                None => EmittedImageRole::Preserved,
            },
        })
    }
}

/// Emits one prepared image using shared destination routing and count semantics.
fn emit_prepared_image(
    output_dir: &Path,
    emission: &mut ImageFileEmission<'_>,
    prepared: PreparedImage<'_>,
    counts: &mut ImageWriteCounts,
) -> Result<()> {
    // Both matches stay exhaustive so a new Emitted image role fails to compile
    // here rather than silently defaulting to the document's output directory
    // or to no count at all.
    let destination = match prepared.role {
        EmittedImageRole::RoutedGif(destination) => destination,
        EmittedImageRole::Converted
        | EmittedImageRole::ConversionSkipped
        | EmittedImageRole::Preserved => output_dir,
    };
    emission.emit(destination, prepared.format, &prepared.data)?;

    counts.extracted += 1;
    match prepared.role {
        EmittedImageRole::RoutedGif(_) => counts.gifs_routed += 1,
        EmittedImageRole::Converted => counts.converted += 1,
        EmittedImageRole::ConversionSkipped => counts.skipped += 1,
        EmittedImageRole::Preserved => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests;
