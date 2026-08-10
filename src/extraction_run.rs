//! Extraction run workflow for document selection, sequencing, and observation.
//!
//! The run sequences documents and reports them; it no longer keeps counters of
//! its own. Each document's facts are folded into an
//! [`ExtractionRunOutcomeAccumulator`], which builds the terminal outcome by
//! construction — see ADR-0006.

use std::path::PathBuf;

use crate::document_extraction::{
    DocumentExtraction, DocumentExtractionOutcome, DocumentExtractionPolicy,
};
use crate::document_search_surface::FilesystemSearchSurface;
use crate::document_selection::{self, DocumentSelectionOptions, EpubFilter};
use crate::extraction_run_observation::{
    ExtractionRunObservation, ExtractionRunObserver, ExtractionRunOutcome,
    ExtractionRunOutcomeAccumulator,
};
use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};

// The run reads its outcome only as a whole value; these two parts of it are
// named by the in-crate tests, which reach them through `use super::*`.
#[cfg(test)]
use crate::extraction_run_observation::{ConversionFacts, ExtractionOutputKind};

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
        &FilesystemSearchSurface,
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
    let mut outcome_accumulator =
        ExtractionRunOutcomeAccumulator::new(document_extraction.applicable_outcome_facts());

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
        outcome_accumulator.fold(&facts);
        if let Some(error) = error {
            observer.on_observation(ExtractionRunObservation::DocumentError {
                path: path.clone(),
                message: error.to_string(),
            });
        }

        observer.on_observation(ExtractionRunObservation::DocumentFinished { path: path.clone() });
    }

    let outcome = outcome_accumulator.finish(cover_only);
    observer.on_observation(ExtractionRunObservation::Terminal(outcome.clone()));
    outcome
}

#[cfg(test)]
mod tests;
