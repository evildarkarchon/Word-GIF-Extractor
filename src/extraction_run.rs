//! Extraction run workflow for document selection, sequencing, and aggregation.

use std::num::NonZeroUsize;
use std::path::PathBuf;

use crate::document_extraction::{
    ApplicableOutcomeFacts, DocumentExtraction, DocumentExtractionFacts, DocumentExtractionOutcome,
    DocumentExtractionPolicy,
};
use crate::document_selection::{self, DocumentSelectionOptions, EpubFilter};
use crate::extraction_run_observation::{
    ConversionFacts, ExtractionOutputKind, ExtractionRunObservation, ExtractionRunObserver,
    ExtractionRunOutcome, GifRoutingFacts,
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

/// Executes one Extraction run and returns its semantic outcome directly.
///
/// The owned request is consumed exactly once. Selection diagnostics and
/// document-local failures are emitted through the observer and do not make the
/// run fallible or stop later documents. The final observation carries the same
/// outcome returned by this operation, and nothing is observed afterward.
///
/// Document selection reports into the same observer rather than through a
/// second seam, so the run never transports selection facts it does not read.
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
    let selected_documents = document_selection::select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive,
            output: output.as_deref(),
            epub_filter: &epub_filter,
        },
        &mut *observer,
    );

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
            gif_routing: applicable.into_gif_destination().map(|destination| {
                GifRoutingAggregation {
                    routed_gifs: 0,
                    destination,
                }
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
