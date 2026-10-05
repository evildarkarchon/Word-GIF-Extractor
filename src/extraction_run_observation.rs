//! Extraction run observation: the single ordered fact stream an Extraction run emits.
//!
//! One vocabulary covers the whole run — Document selection progress and
//! diagnostics, per-document extraction status, and the terminal Extraction run
//! outcome. Document selection emits into this stream directly rather than
//! through a second observer seam, so no module transports facts it never reads.
//!
//! # Why the outcome types live here
//!
//! [`ExtractionRunObservation::Terminal`] carries an [`ExtractionRunOutcome`]. If
//! the outcome types stayed in the Extraction run, this module would depend on
//! that module while Document selection depended on this one, and the Extraction
//! run already depends on Document selection — the cycle would only have moved.
//! Owning the outcome types here leaves every edge one-way: Document selection,
//! the Extraction run and Extraction run presentation all depend on this module,
//! and it depends on none of them.
//!
//! The outward dependencies are Document extraction and the Emitted image tally.
//! The wording of a [`DocumentExtractionWarning`] is owned by Document extraction
//! and transported here opaquely, and [`ExtractionRunOutcomeAccumulator`] folds
//! the facts that module retains. Document extraction never observes a run, so
//! that edge does not come back. The fold adds Emitted image tallies, whose leaf
//! module imports nothing, so that edge cannot come back either (ADR-0017).
//!
//! # What is deliberately absent
//!
//! Observations carry structured facts, never terminal wording and never run
//! policy. [`ExtractionRunObservation::FilteringEpubs`] therefore carries the
//! title and author it is filtering by rather than the Document selection
//! `EpubFilter` that holds them, which keeps a selection policy type out of the
//! stream and out of this module's dependencies.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::document_extraction::{
    ApplicableOutcomeFacts, DocumentExtractionFacts, DocumentExtractionWarning,
};
use crate::emitted_image_tally::EmittedImageTally;

/// Scope of the Document discovery phase reported in the observation stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentDiscoveryScope {
    /// Requested paths are inspected without recursive directory traversal.
    RequestedInputs,
    /// At least one requested directory is traversed recursively.
    RecursiveDirectories,
}

/// Document selection use that could not read EPUB declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubDeclarationPurpose {
    /// Declarations were needed to apply a requested EPUB filter.
    Filtering,
    /// Declarations were needed to deduplicate EPUBs before filename fallback.
    Deduplication,
}

/// Semantic classification of output produced or sought by an Extraction run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionOutputKind {
    /// Normal document images, including EPUB normal-image fallback output.
    Images,
    /// Required EPUB covers when no normal-image output was emitted.
    Covers,
}

/// Conversion totals retained only when conversion was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionFacts {
    converted_images: usize,
    skipped_conversions: usize,
}

impl ConversionFacts {
    /// Creates applicable conversion facts, including valid zero totals.
    pub fn new(converted_images: usize, skipped_conversions: usize) -> Self {
        Self {
            converted_images,
            skipped_conversions,
        }
    }

    /// Returns the number of images converted before emission.
    pub fn converted_images(&self) -> usize {
        self.converted_images
    }

    /// Returns the number of requested conversions that preserved source bytes.
    pub fn skipped_conversions(&self) -> usize {
        self.skipped_conversions
    }
}

/// Routed-GIF facts retained together with their required destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GifRoutingFacts {
    routed_gifs: NonZeroUsize,
    destination: PathBuf,
}

impl GifRoutingFacts {
    /// Creates applicable GIF-routing facts with a positive routed count.
    pub fn new(routed_gifs: NonZeroUsize, destination: PathBuf) -> Self {
        Self {
            routed_gifs,
            destination,
        }
    }

    /// Returns the positive number of GIFs routed by the run.
    pub fn routed_gifs(&self) -> usize {
        self.routed_gifs.get()
    }

    /// Returns the destination that received the routed GIFs.
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

/// State-valid facts for an Extraction run that emitted output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducedOutput {
    output_kind: ExtractionOutputKind,
    emitted_images: NonZeroUsize,
    documents_with_output: NonZeroUsize,
    conversion: Option<ConversionFacts>,
    gif_routing: Option<GifRoutingFacts>,
    failed_documents: Option<NonZeroUsize>,
}

impl ProducedOutput {
    /// Returns whether the emitted files are normal images or required covers.
    pub fn output_kind(&self) -> ExtractionOutputKind {
        self.output_kind
    }

    /// Returns the positive number of emitted image files.
    pub fn emitted_images(&self) -> usize {
        self.emitted_images.get()
    }

    /// Returns the positive number of documents that emitted output.
    pub fn documents_with_output(&self) -> usize {
        self.documents_with_output.get()
    }

    /// Returns conversion facts exactly when conversion was requested.
    pub fn conversion(&self) -> Option<&ConversionFacts> {
        self.conversion.as_ref()
    }

    /// Returns routing facts exactly when at least one GIF was routed.
    pub fn gif_routing(&self) -> Option<&GifRoutingFacts> {
        self.gif_routing.as_ref()
    }
}

/// Closed terminal result of one Extraction run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionRunOutcome {
    /// Document selection found no eligible documents.
    NoDocuments,
    /// Documents were selected, but no image file was emitted.
    NoOutput {
        /// Whether the run sought normal images or required covers.
        output_kind: ExtractionOutputKind,
        /// How many selected documents failed, when any did.
        failed_documents: Option<NonZeroUsize>,
    },
    /// At least one selected document emitted at least one image file.
    ProducedOutput(ProducedOutput),
}

impl ExtractionRunOutcome {
    /// Returns how many selected documents failed to extract, when any did.
    ///
    /// Always `None` for [`Self::NoDocuments`]: a run that selected nothing had
    /// nothing to fail, which the variant states by carrying no count at all.
    pub fn failed_documents(&self) -> Option<NonZeroUsize> {
        match self {
            Self::NoDocuments => None,
            Self::NoOutput {
                failed_documents, ..
            } => *failed_documents,
            Self::ProducedOutput(output) => output.failed_documents,
        }
    }
}

/// Builds one [`ExtractionRunOutcome`] from the documents an Extraction run processed.
///
/// The value is seeded from [`ApplicableOutcomeFacts`], folded once per
/// document with that document's [`DocumentExtractionFacts`], and finished into
/// an outcome. Finish takes the run's cover intent and cannot fail.
///
/// This is the one definition of a valid produced outcome. An Extraction run
/// assembles its outcome here, and so does any caller holding no run, such as a
/// test building one outcome to render: a few recorded tallies are enough to
/// fold, so no second, validating constructor is needed (ADR-0017).
///
/// # Why finishing needs no check
///
/// A produced outcome whose documents or whose classified output exceeded its
/// emitted total would be inconsistent. Neither can happen here, and
/// nothing is re-tested after the fold, per ADR-0006. Every total this
/// value reads comes from one Emitted image tally, the sum of the documents'
/// tallies, and a tally grows only by recording images, each under one purpose
/// and at most one counted role, so its converted, conversion-skipped and
/// GIF-routed totals never together exceed its emitted total (ADR-0017). A
/// document only raises the document count by emitting at least one image.
/// Restating either check here would put back the assumption ADR-0006 removed.
pub(crate) struct ExtractionRunOutcomeAccumulator {
    /// Every folded document's tally added together, not any one document's.
    tally: EmittedImageTally,
    documents_with_output: usize,
    /// Whether the outcome carries conversion facts, which are valid at zero.
    conversion_applicable: bool,
    /// Where routed GIFs go, held whether or not any GIF is routed.
    gif_destination: Option<PathBuf>,
    /// Documents whose extraction failed, counted where the run reports each one.
    failed_documents: usize,
}

impl ExtractionRunOutcomeAccumulator {
    /// Seeds accumulation with only the fact groups the run's workflow permits.
    ///
    /// The seed comes from Document extraction's report rather than from the
    /// Image write policy that owns those facts: naming that policy here would
    /// give this module a second outward edge, to the Image write pipeline,
    /// which is the coupling ADR-0004 exists to keep out.
    pub(crate) fn new(applicable: ApplicableOutcomeFacts) -> Self {
        Self {
            tally: EmittedImageTally::default(),
            documents_with_output: 0,
            conversion_applicable: applicable.is_conversion_applicable(),
            gif_destination: applicable.into_gif_destination(),
            failed_documents: 0,
        }
    }

    /// Folds in the facts retained by one completed or failed Document extraction.
    ///
    /// The cross-document fold lives here rather than in Document extraction, so
    /// that module stays stateless across documents.
    pub(crate) fn fold(&mut self, facts: &DocumentExtractionFacts) {
        // One addition carries every total, so no counter is re-spelled here, and
        // what a document's output was for arrives as its normal-image and cover
        // totals rather than as a classification to merge.
        let tally = facts.get_tally();
        self.tally += tally;

        if tally.emitted() > 0 {
            self.documents_with_output += 1;
        }
    }

    /// Counts one document whose extraction failed.
    ///
    /// Separate from [`Self::fold`] because a failed document's facts fold
    /// exactly like a completed one's -- partial output is still output -- while
    /// the failure itself is a second fact. The run records it where it reports
    /// the document's error, so the outcome counts a failure exactly when the
    /// terminal shows one.
    pub(crate) fn record_failed_document(&mut self) {
        self.failed_documents += 1;
    }

    /// Consumes the accumulated facts into the run's terminal outcome.
    ///
    /// `cover_only` is the run's cover intent, which arrives here rather than
    /// inside [`ApplicableOutcomeFacts`] because it classifies the outcome
    /// instead of enabling a fact group — and a run that folded no output has
    /// nothing else that could say it sought covers.
    pub(crate) fn finish(self, cover_only: bool) -> ExtractionRunOutcome {
        // EPUB fallback and DOCX output are normal images even during a cover-only run.
        // Classify output as covers only when no document included normal images.
        let output_kind = if cover_only && self.tally.normal_images() == 0 {
            ExtractionOutputKind::Covers
        } else {
            ExtractionOutputKind::Images
        };
        let failed_documents = NonZeroUsize::new(self.failed_documents);
        // The two counts are zero together or positive together, because a
        // document joins the document count exactly by emitting an image. The
        // pattern reads both rather than deriving one from the other, which is
        // what keeps this a total match instead of a discharged assumption.
        let (Some(emitted_images), Some(documents_with_output)) = (
            NonZeroUsize::new(self.tally.emitted()),
            NonZeroUsize::new(self.documents_with_output),
        ) else {
            return ExtractionRunOutcome::NoOutput {
                output_kind,
                failed_documents,
            };
        };
        let conversion = self
            .conversion_applicable
            .then(|| ConversionFacts::new(self.tally.converted(), self.tally.conversion_skipped()));
        // `GifRoutingFacts` requires a positive routed count, so the routed total
        // is lifted into it only here, and only if any GIF was in fact routed.
        let gif_routing = self.gif_destination.and_then(|destination| {
            NonZeroUsize::new(self.tally.gifs_routed())
                .map(|routed_gifs| GifRoutingFacts::new(routed_gifs, destination))
        });

        ExtractionRunOutcome::ProducedOutput(ProducedOutput {
            output_kind,
            emitted_images,
            documents_with_output,
            conversion,
            gif_routing,
            failed_documents,
        })
    }
}

/// One structured, ordered fact emitted during an Extraction run.
///
/// Phases are reported in Document discovery, optional EPUB filtering, then
/// optional EPUB deduplication order. A phase with no work is silent; a phase
/// that runs emits an initial running variant, monotonically advancing running
/// variants, and exactly one finished variant — a rule the split between the
/// running and finished variants makes visible rather than merely documented.
///
/// Observations exclude terminal wording and user-interface commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionRunObservation {
    /// Current Document discovery state while inputs are still being inspected.
    DiscoveringDocuments {
        scope: DocumentDiscoveryScope,
        discovered: usize,
    },
    /// Final Document discovery state; no further discovery variant follows.
    DocumentDiscoveryFinished {
        scope: DocumentDiscoveryScope,
        discovered: usize,
    },
    /// Current EPUB filtering state, including what is being filtered by.
    ///
    /// The title and author travel as facts rather than as the Document
    /// selection filter that holds them, so run policy stays off the stream.
    FilteringEpubs {
        title: Option<String>,
        author: Option<String>,
        checked: usize,
        total: usize,
        matching: usize,
    },
    /// Final EPUB filtering state; no further filtering variant follows.
    EpubFilteringFinished {
        checked: usize,
        total: usize,
        matching: usize,
    },
    /// Current EPUB deduplication state.
    DeduplicatingEpubs {
        checked: usize,
        total: usize,
        duplicates_found: usize,
        unique_remaining: usize,
    },
    /// Final EPUB deduplication state; no further deduplication variant follows.
    EpubDeduplicationFinished {
        checked: usize,
        total: usize,
        duplicates_found: usize,
        unique_remaining: usize,
    },
    /// A requested input path does not exist and was skipped.
    MissingInput { path: PathBuf },
    /// A requested input was skipped because only EPUB documents are eligible.
    ///
    /// The fact names what was skipped, never why eligibility was restricted:
    /// Document selection is not told the reason. Only inputs the user named
    /// reach this variant — documents dropped during directory traversal are
    /// accounted for by the discovery counters, as filtered-out EPUBs are.
    SkippedNonEpubInput { path: PathBuf },
    /// A Document discovery path could not be inspected; detail stays presentation-neutral.
    DocumentDiscoveryFailed { path: PathBuf, detail: String },
    /// EPUB declarations could not be read for the stated selection purpose.
    UnreadableEpubDeclarations {
        path: PathBuf,
        purpose: EpubDeclarationPurpose,
        detail: String,
    },
    /// Document extraction has started.
    ExtractionStarted { total: usize, cover_only: bool },
    /// A document is about to be processed.
    DocumentStarted { path: PathBuf, display_name: String },
    /// A document produced an opaque non-fatal warning while processing.
    ///
    /// The run transports the Document extraction-owned value together with the
    /// originating document path; it never renders or reconstructs the wording.
    DocumentWarning {
        path: PathBuf,
        warning: DocumentExtractionWarning,
    },
    /// A document failed to process; the run will continue.
    DocumentError { path: PathBuf, message: String },
    /// A document has finished processing, successfully or not.
    DocumentFinished { path: PathBuf },
    /// The final semantic outcome of the run; no observation follows it.
    Terminal(ExtractionRunOutcome),
}

/// Receives the complete ordered stream of Extraction run observations.
///
/// Every emitter in the run — Document selection included — reports through this
/// one seam, and the stream ends with exactly one terminal outcome.
pub trait ExtractionRunObserver {
    /// Handles one structured observation emitted by the Extraction run.
    fn on_observation(&mut self, observation: ExtractionRunObservation);
}

#[cfg(test)]
mod tests;
