//! Extraction run workflow for document selection, sequencing, and aggregation.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::document_extraction::{
    ApplicableOutcomeFacts, DocumentExtraction, DocumentExtractionFacts, DocumentExtractionOutcome,
    DocumentExtractionPolicy, DocumentExtractionWarning,
};
use crate::document_selection::{
    self, DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionOptions,
    DocumentSelectionProgress, EpubFilter,
};
use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};

/// Opaque, ready-to-execute handoff produced by Extraction run intake.
///
/// Construction takes the two assembled policies that govern the run — the
/// Document extraction policy and the Image write policy — and retains no facts
/// derived from either. An Extraction run consumes this request by value and
/// asks Document extraction for the derived facts when it needs them.
pub struct ExtractionRunRequest {
    inputs: Vec<PathBuf>,
    recursive: bool,
    output: Option<PathBuf>,
    epub_filter: EpubFilter,
    document_extraction: DocumentExtraction,
}

impl ExtractionRunRequest {
    /// Builds one valid request from normalized inputs and workflow policies.
    pub(crate) fn new(
        inputs: Vec<PathBuf>,
        recursive: bool,
        output: Option<PathBuf>,
        epub_filter: EpubFilter,
        document_extraction_policy: DocumentExtractionPolicy,
        image_write_policy: ImageWritePolicy,
    ) -> Self {
        let image_write_pipeline = ImageWritePipeline::new(image_write_policy);

        Self {
            inputs,
            recursive,
            output,
            epub_filter,
            document_extraction: DocumentExtraction::new(
                document_extraction_policy,
                image_write_pipeline,
            ),
        }
    }
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
    NoOutput(ExtractionOutputKind),
    /// At least one selected document emitted at least one image file.
    ProducedOutput(ProducedOutput),
}

impl ExtractionRunOutcome {
    /// Creates a produced-output outcome when all semantic totals are consistent.
    ///
    /// Positive count types prevent terminal adapters and tests from creating a
    /// produced state with zero output. The remaining checks ensure documents
    /// and classified output facts cannot exceed the emitted-image total.
    pub fn try_produced(
        output_kind: ExtractionOutputKind,
        emitted_images: NonZeroUsize,
        documents_with_output: NonZeroUsize,
        conversion: Option<ConversionFacts>,
        gif_routing: Option<GifRoutingFacts>,
    ) -> Option<Self> {
        let conversion_total = match conversion {
            Some(facts) => facts
                .converted_images
                .checked_add(facts.skipped_conversions)?,
            None => 0,
        };
        let routed_gifs = gif_routing
            .as_ref()
            .map_or(0, |facts| facts.routed_gifs.get());
        let classified_output = conversion_total.checked_add(routed_gifs)?;
        if documents_with_output.get() > emitted_images.get()
            || classified_output > emitted_images.get()
        {
            return None;
        }

        Some(Self::ProducedOutput(ProducedOutput {
            output_kind,
            emitted_images,
            documents_with_output,
            conversion,
            gif_routing,
        }))
    }
}

#[derive(Default)]
struct ConversionAggregation {
    converted_images: usize,
    skipped_conversions: usize,
}

struct GifRoutingAggregation {
    routed_gifs: usize,
    destination: PathBuf,
}

/// Run-private aggregation that can be consumed only into a valid outcome.
struct RunAggregation {
    emitted_images: usize,
    documents_with_output: usize,
    has_normal_image_output: bool,
    conversion: Option<ConversionAggregation>,
    gif_routing: Option<GifRoutingAggregation>,
}

/// One structured, ordered fact emitted during an Extraction run.
///
/// Observations retain Document selection's structured domain values alongside
/// per-document extraction lifecycle facts and the final semantic outcome. They
/// exclude terminal wording and user-interface commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionRunObservation {
    /// One immutable Document selection phase snapshot.
    DocumentSelectionProgress(DocumentSelectionProgress),
    /// One structured, non-fatal Document selection diagnostic.
    DocumentSelectionDiagnostic(DocumentSelectionDiagnostic),
    /// Document extraction has started.
    ExtractionStarted { total: usize, cover_only: bool },
    /// A document is about to be processed.
    DocumentStarted { path: PathBuf, display_name: String },
    /// A document failed to process; the run will continue.
    DocumentError { path: PathBuf, message: String },
    /// A document produced an opaque non-fatal warning while processing.
    ///
    /// The run transports the Document extraction-owned value together with the
    /// originating document path; it never renders or reconstructs the wording.
    DocumentWarning {
        path: PathBuf,
        warning: DocumentExtractionWarning,
    },
    /// A document has finished processing, successfully or not.
    DocumentFinished { path: PathBuf },
    /// The final semantic outcome of the run; no observation follows it.
    Terminal(ExtractionRunOutcome),
}

/// Receives the complete ordered stream of Extraction run observations.
///
/// The observer does not inherit the lower Document selection interface; the
/// run privately translates those callbacks into this cohesive seam and ends
/// every stream with exactly one terminal outcome.
pub trait ExtractionRunObserver {
    /// Handles one structured observation emitted by the Extraction run.
    fn on_observation(&mut self, observation: ExtractionRunObservation);
}

/// Run-private bridge from Document selection callbacks to run observations.
struct DocumentSelectionObservationAdapter<'observer, Observer> {
    observer: &'observer mut Observer,
}

impl<'observer, Observer> DocumentSelectionObservationAdapter<'observer, Observer> {
    /// Borrows the run observer for the duration of one Document selection call.
    fn new(observer: &'observer mut Observer) -> Self {
        Self { observer }
    }
}

impl<Observer> DocumentSelectionObserver for DocumentSelectionObservationAdapter<'_, Observer>
where
    Observer: ExtractionRunObserver,
{
    /// Preserves one structured progress value in the run observation stream.
    fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
        self.observer
            .on_observation(ExtractionRunObservation::DocumentSelectionProgress(
                progress,
            ));
    }

    /// Preserves one structured diagnostic value in the run observation stream.
    fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        self.observer
            .on_observation(ExtractionRunObservation::DocumentSelectionDiagnostic(
                diagnostic,
            ));
    }
}

/// Executes one Extraction run and returns its semantic outcome directly.
///
/// The owned request is consumed exactly once. Selection diagnostics and
/// document-local failures are emitted through the observer and do not make the
/// run fallible or stop later documents. The final observation carries the same
/// outcome returned by this operation, and nothing is observed afterward.
pub fn run(
    request: ExtractionRunRequest,
    observer: &mut impl ExtractionRunObserver,
) -> ExtractionRunOutcome {
    let ExtractionRunRequest {
        inputs,
        recursive,
        output,
        epub_filter,
        document_extraction,
    } = request;
    let cover_only = document_extraction.is_epub_cover_extraction_configured();
    let selected_documents = {
        let mut selection_observer = DocumentSelectionObservationAdapter::new(observer);
        document_selection::select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive,
                output: output.as_deref(),
                epub_filter: &epub_filter,
            },
            &mut selection_observer,
        )
    };

    if selected_documents.is_empty() {
        let outcome = ExtractionRunOutcome::NoDocuments;
        observer.on_observation(ExtractionRunObservation::Terminal(outcome.clone()));
        return outcome;
    }

    observer.on_observation(ExtractionRunObservation::ExtractionStarted {
        total: selected_documents.len(),
        cover_only,
    });
    let mut aggregation = RunAggregation::new(document_extraction.applicable_outcome_facts());

    for selected_document in selected_documents {
        // The run retains only observer-facing facts before transferring the
        // selection-owned handoff into Document extraction exactly once.
        let path = selected_document.get_path().to_path_buf();
        let display_name = selected_document.get_display_name().to_string();
        observer.on_observation(ExtractionRunObservation::DocumentStarted {
            path: path.clone(),
            display_name,
        });

        let (facts, error) = match document_extraction.extract(selected_document) {
            DocumentExtractionOutcome::Completed(facts) => (facts, None),
            DocumentExtractionOutcome::Failed { facts, error } => (facts, Some(error)),
        };
        // Warnings are forwarded as opaque Document extraction values in their
        // retained order, before any document error, so the run never becomes a
        // second owner of warning wording.
        for warning in facts.get_warnings() {
            observer.on_observation(ExtractionRunObservation::DocumentWarning {
                path: path.clone(),
                warning: warning.clone(),
            });
        }
        aggregation.record_document_result(&facts);
        if let Some(error) = error {
            observer.on_observation(ExtractionRunObservation::DocumentError {
                path: path.clone(),
                message: error.to_string(),
            });
        }

        observer.on_observation(ExtractionRunObservation::DocumentFinished { path: path.clone() });
    }

    let outcome = aggregation.into_outcome(cover_only);
    observer.on_observation(ExtractionRunObservation::Terminal(outcome.clone()));
    outcome
}

impl RunAggregation {
    /// Starts aggregation with only the facts applicable to the requested workflow.
    fn new(applicable: ApplicableOutcomeFacts) -> Self {
        let conversion = applicable
            .is_conversion_applicable()
            .then(ConversionAggregation::default);

        Self {
            emitted_images: 0,
            documents_with_output: 0,
            has_normal_image_output: false,
            conversion,
            gif_routing: applicable
                .into_gif_destination()
                .map(|destination| GifRoutingAggregation {
                    routed_gifs: 0,
                    destination,
                }),
        }
    }

    /// Records Document extraction facts retained by one completed or failed outcome.
    fn record_document_result(&mut self, facts: &DocumentExtractionFacts) {
        self.emitted_images += facts.get_emitted_images();
        if let Some(conversion) = &mut self.conversion {
            conversion.converted_images += facts.get_converted_images();
            conversion.skipped_conversions += facts.get_skipped_conversions();
        }
        if let Some(gif_routing) = &mut self.gif_routing {
            gif_routing.routed_gifs += facts.get_gifs_routed();
        }

        if facts.get_emitted_images() > 0 {
            self.documents_with_output += 1;
            self.has_normal_image_output |= facts.is_normal_image_output_present();
        }
    }

    /// Consumes aggregate counters into one closed, state-valid semantic outcome.
    fn into_outcome(self, cover_only: bool) -> ExtractionRunOutcome {
        // EPUB fallback and DOCX output are normal images even during a cover-only run.
        // Classify output as covers only when every emitted file used required-cover purpose.
        let output_kind = if cover_only && !self.has_normal_image_output {
            ExtractionOutputKind::Covers
        } else {
            ExtractionOutputKind::Images
        };
        let Some(emitted_images) = NonZeroUsize::new(self.emitted_images) else {
            return ExtractionRunOutcome::NoOutput(output_kind);
        };
        let documents_with_output = NonZeroUsize::new(self.documents_with_output)
            .expect("emitted output must belong to at least one document");
        let conversion = self
            .conversion
            .map(|facts| ConversionFacts::new(facts.converted_images, facts.skipped_conversions));
        let gif_routing = self.gif_routing.and_then(|facts| {
            NonZeroUsize::new(facts.routed_gifs)
                .map(|routed_gifs| GifRoutingFacts::new(routed_gifs, facts.destination))
        });

        ExtractionRunOutcome::try_produced(
            output_kind,
            emitted_images,
            documents_with_output,
            conversion,
            gif_routing,
        )
        .expect("run aggregation must produce consistent semantic totals")
    }
}

#[cfg(test)]
mod tests;
