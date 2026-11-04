//! Image write pipeline for discovering and emitting archive images incrementally.

mod discovery;
mod emission;

use anyhow::{Error, Result};
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionOutcome, ConversionPolicy};
use crate::emitted_image_tally::{EmittedImageTally, TallyRole};
use crate::image_format::ImageFormat;
use crate::output_placement::OutputPlacement;

pub(crate) use self::discovery::ArchiveImageSource;
use self::discovery::{ArchiveImageDiscoveryOutcome, discover_image};
use self::emission::ImageFileEmission;

/// Valid per-run choices interpreted by the Image write pipeline.
///
/// [`PartialEq`] is derived only under `cfg(test)`, so that Extraction run
/// intake's tests can compare the policy intake built; production never compares
/// policies.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
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
    /// An image's conversion was unsupported, whatever its Image write purpose.
    /// `source_name` is the source's diagnostic name, so two skipped images in one
    /// document warn apart.
    ConversionSkipped {
        source_name: String,
        format: ImageFormat,
    },
    /// An image's conversion failed, whatever its Image write purpose.
    /// `source_name` is the source's diagnostic name, as for [`Self::ConversionSkipped`].
    ConversionFailed {
        source_name: String,
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
///
/// The Emitted image tally records whether each emitted image was a normal image
/// or a cover, so no separate normal-output flag travels beside it.
#[derive(Debug, Default)]
pub(crate) struct ImageWriteResult {
    pub(crate) tally: EmittedImageTally,
    pub(crate) warnings: Vec<ImageWriteWarning>,
}

impl ImageWriteResult {
    /// Creates one complete Image write pipeline outcome from already-produced facts.
    ///
    /// There is nothing to validate: a tally can only grow by recording images, so
    /// its totals are consistent however it was built.
    fn new(tally: EmittedImageTally, warnings: Vec<ImageWriteWarning>) -> Self {
        Self { tally, warnings }
    }

    /// Appends later Image write facts, adding tallies and preserving warning order.
    ///
    /// Private to the pipeline: the required-cover scope is the only fold of facts
    /// across invocations, so no module outside the pipeline combines them (ADR-0021).
    fn append(&mut self, mut later: Self) {
        self.tally += later.tally;
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

/// One image Archive image discovery accepted, as image preparation receives it.
///
/// `source_name` is the source's diagnostic name, the same name acquisition and
/// extension-fallback warnings use. It travels with the bytes so a conversion
/// warning can name the image it is about; it never decides an output path.
#[derive(Debug, PartialEq)]
struct AcceptedImage {
    source_name: String,
    data: Vec<u8>,
    format: ImageFormat,
}

/// The role one image takes as the Image write pipeline emits it.
///
/// Exactly one role applies per emitted image, which is what keeps the
/// converted, conversion-skipped and GIF-routed counts from together exceeding
/// the emitted count. `RoutedGif` carries the destination read from the Image
/// write policy at the moment routing was decided, so a routed image and the
/// destination that justifies it cannot be decided apart. The Emitted image
/// tally records the destination-free [`TallyRole`] this maps onto.
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
/// There is always an image to emit: a conversion that cannot produce the
/// requested bytes is resolved here, once, for every Image write purpose, by
/// emitting the original bytes under their original Image format in the
/// conversion-skipped role (ADR-0023). The warning that explains the fallback
/// travels beside the image, and each visitor stages it into its own warning
/// order; no visitor has a fallback case left to resolve.
struct ImagePreparation<'policy> {
    /// The image ready to emit, in the role preparation decided.
    image: PreparedImage<'policy>,
    /// The conversion-skipped or conversion-failed warning naming the source
    /// image, present exactly when conversion fell back to the original bytes.
    conversion_warning: Option<ImageWriteWarning>,
}

impl<'policy> ImagePreparation<'policy> {
    /// Wraps an image that preparation produced without a conversion fallback.
    fn ready(image: PreparedImage<'policy>) -> Self {
        Self {
            image,
            conversion_warning: None,
        }
    }

    /// Resolves a conversion fallback: the original `data` is emitted under
    /// `format` as a conversion-skipped image, beside `warning`.
    fn fell_back(data: Vec<u8>, format: ImageFormat, warning: ImageWriteWarning) -> Self {
        Self {
            image: PreparedImage {
                data,
                format,
                role: EmittedImageRole::ConversionSkipped,
            },
            conversion_warning: Some(warning),
        }
    }
}

/// Immutable Image write pipeline configured for one Extraction run.
///
/// It holds nothing but its policy, and stays a type of its own anyway, for the
/// reason [`DocumentExtraction`] stays distinct from its Document extraction
/// policy: the policy is the run's choices, and this is the process that applies
/// them. Merging the two would leave either a policy that writes images or an
/// intake that builds the pipeline itself.
///
/// [`DocumentExtraction`]: crate::document_extraction::DocumentExtraction
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

    /// Runs one required EPUB cover attempt sequence inside a scoped cover visitor.
    ///
    /// Modelled on [`Self::write_from`]: the caller drives the sequence through the
    /// [`RequiredCoverVisitor`] it is lent, and the pipeline folds every attempt's
    /// facts, and any fallback's after them, into the one outcome returned here. EPUB
    /// cover extraction decides which candidate to attempt and when to stop; it never
    /// combines facts itself.
    ///
    /// # Errors
    ///
    /// Returns an error when `attempts` returns one, which is how an emission failure
    /// in an attempt or in fallback ends the sequence. Every fact recorded before the
    /// failure stays on it, in the order it was recorded; earlier successful writes
    /// are not rolled back.
    pub(crate) fn write_required_cover(
        &self,
        placement: &OutputPlacement,
        attempts: impl FnOnce(&mut RequiredCoverVisitor<'_, '_>) -> Result<()>,
    ) -> ImageWriteOutcome {
        let mut visitor = RequiredCoverVisitor {
            pipeline: self,
            placement,
            facts: ImageWriteResult::default(),
        };
        if let Err(error) = attempts(&mut visitor) {
            return Err(ImageWriteFailure {
                partial: visitor.facts,
                error,
            });
        }
        Ok(visitor.facts)
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
    /// The attempt stages its single prepared image with Image file emission and then
    /// finishes it, so finishing names the cover singularly after the placement's
    /// base name: a cover attempt never has a second image to number against.
    ///
    /// Returns only the attempt's facts: whether it wrote a cover is read off their
    /// Emitted image tally, never off a second channel beside them (ADR-0023). A read
    /// failure and a cover filtered out by the allowed formats write nothing, so EPUB
    /// cover extraction hands over to its next candidate. A conversion fallback is a
    /// successful emission, of the original bytes as a conversion-skipped cover.
    /// [`RequiredCoverVisitor::attempt`] folds the facts into its scope.
    ///
    /// # Errors
    ///
    /// Returns an error when the cover cannot be emitted, retaining the facts the
    /// attempt accumulated before the failure.
    fn attempt_required_cover(
        &self,
        placement: &OutputPlacement,
        manifest_path: &str,
        mime: &str,
        reader: &mut dyn Read,
    ) -> ImageWriteOutcome {
        let source = ArchiveImageSource::required_cover(manifest_path, mime);
        let discovered = discover_image(&source, reader, &self.policy.allowed_formats);
        let mut result = ImageWriteResult::default();
        result.warnings.extend(discovered.warnings);
        let image = match discovered.outcome {
            ArchiveImageDiscoveryOutcome::Accepted(image) => image,
            // Neither writes a cover, and the tally says so: the warnings discovery
            // recorded are the attempt's only facts.
            ArchiveImageDiscoveryOutcome::Completed
            | ArchiveImageDiscoveryOutcome::AcquisitionFailed => return Ok(result),
        };

        let prepared = prepare_image_for_write(image, &self.policy);
        // A cover attempt has one source, so its conversion warning simply follows
        // the discovery warnings already recorded; the cover is still emitted in
        // its original bytes as a conversion-skipped cover (ADR-0023).
        result.warnings.extend(prepared.conversion_warning);
        let mut emission = ImageFileEmission::new(placement);
        // This path is bound to the required-cover purpose, so it records covers
        // whatever role the image took: a routed GIF cover is a routed cover, and a
        // conversion-skipped cover is a conversion-skipped cover.
        let tally = &mut result.tally;
        let mut record_cover = |role| tally.record_cover(tally_role(role));
        let emitted = emission
            .stage(prepared.image, &mut record_cover)
            .and_then(|()| emission.finish(record_cover));
        if let Err(error) = emitted {
            return Err(ImageWriteFailure {
                partial: result,
                error,
            });
        }
        Ok(result)
    }

    /// Discovers, prepares, and writes sources supplied through one scoped traversal.
    ///
    /// Sources are numbered in the order the traversal visits them, which is the
    /// document's own order, under the placement's directory and base name.
    /// The traversal must finish each reader before opening the next archive entry.
    /// Per-resource acquisition failures belong to the visitor and remain non-fatal;
    /// an error returned by the traversal aborts the document.
    ///
    /// Returns phase-ordered warning facts and a tally of files actually written.
    /// Filesystem setup, collision exhaustion, create, write, and flush failures
    /// retain those facts with the error; earlier successful writes are not rolled back.
    pub(crate) fn write_from(
        &self,
        placement: &OutputPlacement,
        traverse: impl FnOnce(&mut ArchiveImageVisitor<'_, '_>) -> Result<()>,
    ) -> ImageWriteOutcome {
        let mut visitor = ArchiveImageVisitor::new(&self.policy, placement);
        if let Err(error) = traverse(&mut visitor) {
            return Err(visitor.into_failure(error));
        }
        visitor.finish()
    }
}

/// Scoped authority for one required EPUB cover attempt sequence.
///
/// It folds the facts of every attempt, unreadable cover and fallback into one
/// Image write outcome in the order they happen, and answers from those same facts
/// whether the sequence has written a cover. Nothing stops an attempt after fallback
/// or after a written cover: ADR-0021 rejected typestate here, because EPUB cover
/// extraction's loop is the only caller and its tests pin the order.
pub(crate) struct RequiredCoverVisitor<'pipeline, 'placement> {
    pipeline: &'pipeline ImageWritePipeline,
    placement: &'placement OutputPlacement,
    facts: ImageWriteResult,
}

impl RequiredCoverVisitor<'_, '_> {
    /// Discovers and writes one required cover from its raw source facts and payload.
    ///
    /// The attempt's facts are folded into this scope; whether it wrote a cover is
    /// answered afterwards by [`Self::has_written_cover`]. See
    /// [`ImageWritePipeline::write_required_cover`] for how the sequence ends.
    ///
    /// # Errors
    ///
    /// Returns an error when the cover cannot be emitted. The facts the attempt
    /// recorded before the failure are already folded into the scope, so the caller
    /// only has to propagate the error.
    pub(crate) fn attempt(
        &mut self,
        manifest_path: &str,
        mime: &str,
        reader: &mut dyn Read,
    ) -> Result<()> {
        let outcome =
            self.pipeline
                .attempt_required_cover(self.placement, manifest_path, mime, reader);
        self.fold(outcome)
    }

    /// Records a required cover whose payload the EPUB adapter could not acquire.
    ///
    /// An unavailable candidate writes nothing, so it leaves [`Self::has_written_cover`]
    /// as it was and EPUB cover extraction moves on exactly as after an attempt that
    /// wrote nothing.
    pub(crate) fn unreadable(&mut self, manifest_path: &str, error: impl fmt::Display) {
        self.facts
            .warnings
            .push(ImageWriteWarning::archive_image_acquisition_failed(
                manifest_path,
                error,
            ));
    }

    /// Reports whether any step of this scope has written a cover.
    ///
    /// Answered from the scope's own folded Emitted image tally (covers recorded > 0),
    /// so it can never disagree with the facts the scope returns. Every written cover
    /// counts, whether converted, conversion-skipped or routed to the GIF destination;
    /// a cover candidate that wrote nothing does not (ADR-0023).
    pub(crate) fn has_written_cover(&self) -> bool {
        self.facts.tally.covers() > 0
    }

    /// Runs normal-image fallback as a traversal against a fresh normal-image visitor.
    ///
    /// The fresh visitor names its images singularly or numbered on its own, exactly
    /// as a separate [`ImageWritePipeline::write_from`] would, and its facts are
    /// appended after every cover-attempt fact already in this scope.
    ///
    /// # Errors
    ///
    /// Returns the traversal's error, which aborts the sequence. The fallback's facts
    /// recorded before it are already folded into the scope.
    pub(crate) fn fallback(
        &mut self,
        traverse: impl FnOnce(&mut ArchiveImageVisitor<'_, '_>) -> Result<()>,
    ) -> Result<()> {
        let outcome = self.pipeline.write_from(self.placement, traverse);
        self.fold(outcome)
    }

    /// Appends one step's facts to this scope and hands back the step's error, if any.
    ///
    /// The single fold every step of the sequence goes through: a successful step's
    /// facts and a failed step's partial facts are both appended in the order they
    /// happen, so the caller only ever sees success or the bare error.
    ///
    /// # Errors
    ///
    /// Returns the failed step's error once its partial facts are in this scope.
    fn fold(&mut self, outcome: ImageWriteOutcome) -> Result<()> {
        match outcome {
            Ok(result) => {
                self.facts.append(result);
                Ok(())
            }
            Err(failure) => {
                self.facts.append(failure.partial);
                Err(failure.error)
            }
        }
    }
}

/// Scoped authority for per-resource discovery, preparation, and ordered emission.
///
/// It holds one Image file emission for its whole traversal and stages every
/// prepared image with it; naming and placement are emission's, not the visitor's.
/// The visitor keeps no Output placement of its own: only Image file emission holds
/// it (ADR-0022), and warning facts name source images rather than the placement.
pub(crate) struct ArchiveImageVisitor<'policy, 'placement> {
    policy: &'policy ImageWritePolicy,
    facts: NormalImageFacts,
    emission: ImageFileEmission<'policy, 'placement>,
}

impl<'policy, 'placement> ArchiveImageVisitor<'policy, 'placement> {
    /// Starts one scoped Archive image discovery traversal.
    fn new(policy: &'policy ImageWritePolicy, placement: &'placement OutputPlacement) -> Self {
        Self {
            policy,
            facts: NormalImageFacts::default(),
            emission: ImageFileEmission::new(placement),
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
        self.facts.discovery_warnings.extend(discovered.warnings);

        let ArchiveImageDiscoveryOutcome::Accepted(image) = discovered.outcome else {
            return Ok(());
        };
        let prepared = prepare_image_for_write(image, self.policy);
        // A conversion fallback's warning goes to the conversion phase, so every
        // discovery warning in the traversal still precedes it. It names the
        // source image, not the Output placement, so two fallbacks in one document
        // stay distinguishable.
        self.facts
            .conversion_warnings
            .extend(prepared.conversion_warning);

        // This visitor is bound to the normal-images purpose, so it records
        // normal images; the role-mapping function supplies only the role. The
        // binding holds because only this pipeline can build a required-cover
        // source, and it never hands one to a visitor.
        self.emission
            .stage(prepared.image, normal_image_recorder(&mut self.facts.tally))
    }

    /// Records a source that the document adapter could not open.
    ///
    /// Every such source warns, whatever its name: a source name never decides
    /// where a file is written, so no name is grounds for a silent skip.
    pub(crate) fn unreadable(&mut self, source: ArchiveImageSource, error: impl fmt::Display) {
        self.facts
            .discovery_warnings
            .push(ImageWriteWarning::archive_image_acquisition_failed(
                source.diagnostic_name(),
                error,
            ));
    }

    /// Finishes Image file emission and returns phase-ordered warning facts.
    ///
    /// Finishing writes a still-held-back lone image under the singular name.
    ///
    /// # Errors
    ///
    /// Returns a failure, retaining every fact recorded so far, if that lone image
    /// cannot be emitted.
    fn finish(mut self) -> ImageWriteOutcome {
        // Finishing consumes the emission, moving only that field out of `self`; the
        // facts stay reachable, so the lone image is recorded before they are collected.
        let finished = self
            .emission
            .finish(normal_image_recorder(&mut self.facts.tally));
        let result = self.facts.into_result();
        match finished {
            Ok(()) => Ok(result),
            Err(error) => Err(ImageWriteFailure {
                partial: result,
                error,
            }),
        }
    }

    /// Retains facts accumulated before a traversal or emission failure.
    fn into_failure(self, error: Error) -> ImageWriteFailure {
        ImageWriteFailure {
            partial: self.facts.into_result(),
            error,
        }
    }
}

/// Facts one normal-image visitor accumulates over its traversal.
///
/// Discovery and conversion warnings are kept apart while the traversal runs, so
/// collecting them can report every discovery warning before any conversion warning
/// whichever order the sources produced them in.
#[derive(Default)]
struct NormalImageFacts {
    discovery_warnings: Vec<ImageWriteWarning>,
    conversion_warnings: Vec<ImageWriteWarning>,
    tally: EmittedImageTally,
}

impl NormalImageFacts {
    /// Collects these facts with discovery warnings before conversion warnings, the
    /// phase order every Image write result reports.
    fn into_result(self) -> ImageWriteResult {
        ImageWriteResult::new(
            self.tally,
            self.discovery_warnings
                .into_iter()
                .chain(self.conversion_warnings)
                .collect(),
        )
    }
}

/// Applies Image write policy's GIF routing and conversion to one accepted image.
///
/// Purpose-free: a conversion that cannot produce requested bytes falls back to
/// the original bytes under the original Image format, in the conversion-skipped
/// role, with one warning naming the source image. The same happens for a normal
/// image and a required cover alike (ADR-0023), so a caller never resolves a
/// fallback itself; it stages the image and the warning it is handed.
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
            return ImagePreparation::ready(PreparedImage {
                data: image.data,
                format: image.format,
                role: EmittedImageRole::RoutedGif(destination),
            });
        }

        match conversion.convert(&image.data, image.format) {
            Ok(ConversionOutcome::Converted(converted_bytes, format)) => {
                ImagePreparation::ready(PreparedImage {
                    data: converted_bytes,
                    format,
                    role: EmittedImageRole::Converted,
                })
            }
            Ok(ConversionOutcome::PreservedMatchingSource) => {
                ImagePreparation::ready(PreparedImage {
                    data: image.data,
                    format: image.format,
                    role: EmittedImageRole::Preserved,
                })
            }
            Ok(ConversionOutcome::UnsupportedSource(original_format)) => {
                ImagePreparation::fell_back(
                    image.data,
                    original_format,
                    ImageWriteWarning::ConversionSkipped {
                        source_name: image.source_name,
                        format: original_format,
                    },
                )
            }
            Err(error) => ImagePreparation::fell_back(
                image.data,
                image.format,
                ImageWriteWarning::ConversionFailed {
                    source_name: image.source_name,
                    detail: error.to_string(),
                },
            ),
        }
    } else {
        ImagePreparation::ready(PreparedImage {
            data: image.data,
            format: image.format,
            role: match routed_destination {
                Some(destination) => EmittedImageRole::RoutedGif(destination),
                None => EmittedImageRole::Preserved,
            },
        })
    }
}

/// Maps an Emitted image role onto the Emitted image tally's destination-free role.
///
/// This is the one place production maps roles for counting (ADR-0017). Both
/// visitors' recording closures call it with each role Image file emission reports
/// once a file is complete, and record the result under their own Image write
/// purpose: each caller is statically bound to one purpose, so no runtime purpose
/// value is passed here.
fn tally_role(role: EmittedImageRole<'_>) -> TallyRole {
    // The match stays exhaustive so a new Emitted image role fails to compile
    // here rather than silently defaulting to no count at all.
    match role {
        EmittedImageRole::RoutedGif(_) => TallyRole::RoutedGif,
        EmittedImageRole::Converted => TallyRole::Converted,
        EmittedImageRole::ConversionSkipped => TallyRole::ConversionSkipped,
        EmittedImageRole::Preserved => TallyRole::Preserved,
    }
}

/// Returns the recording callback that counts each completed file as a normal image.
///
/// The one home of the normal-image recording rule: the normal-image visitor stages
/// and finishes through it, and it maps roles only through [`tally_role`] (ADR-0017).
/// The callback borrows `tally` mutably until it is dropped.
fn normal_image_recorder(tally: &mut EmittedImageTally) -> impl FnMut(EmittedImageRole<'_>) + '_ {
    move |role: EmittedImageRole<'_>| tally.record_normal_image(tally_role(role))
}

#[cfg(test)]
mod tests;
