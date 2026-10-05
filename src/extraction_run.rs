//! Extraction run workflow for document selection, sequencing, and observation.
//!
//! The run sequences documents and reports them; it no longer keeps counters of
//! its own. Each document's facts are folded into an
//! [`ExtractionRunOutcomeAccumulator`], which builds the terminal outcome by
//! construction — see ADR-0006.

use std::path::PathBuf;

use crate::document_extraction::{
    ApplicableOutcomeFacts, DocumentExtraction, DocumentExtractionOutcome, EpubCoverPolicy,
};
use crate::document_search_surface::{DocumentSearchSurface, FilesystemSearchSurface};
use crate::document_selection::{self, DocumentSelectionOptions, EpubFilter, SelectedDocument};
use crate::epub_declarations::{EpubDeclarationSource, EpubFileDeclarations};
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
/// Construction takes the policies that govern the run — any EPUB cover policy
/// and the Image write policy — and retains no facts derived from either. An
/// Extraction run consumes this request by value and asks Document extraction
/// for the derived facts when it needs them.
pub struct ExtractionRunRequest {
    selection_inputs: RunSelectionInputs,
    document_extraction: DocumentExtraction,
}

impl ExtractionRunRequest {
    /// Builds one valid request from normalized inputs and workflow policies.
    pub(crate) fn new(
        inputs: Vec<PathBuf>,
        recursive: bool,
        output: Option<PathBuf>,
        epub_filter: EpubFilter,
        epub_cover_policy: Option<EpubCoverPolicy>,
        image_write_policy: ImageWritePolicy,
    ) -> Self {
        let image_write_pipeline = ImageWritePipeline::new(image_write_policy);

        Self {
            selection_inputs: RunSelectionInputs {
                inputs,
                recursive,
                output,
                epub_filter,
            },
            document_extraction: DocumentExtraction::new(epub_cover_policy, image_write_pipeline),
        }
    }
}

/// What the run hands Document selection, apart from cover intent.
///
/// Cover intent is absent on purpose: [`run`] reads it from the request's own
/// Document extraction, so it can never disagree with the extraction that
/// honours it.
struct RunSelectionInputs {
    inputs: Vec<PathBuf>,
    recursive: bool,
    output: Option<PathBuf>,
    epub_filter: EpubFilter,
}

/// Executes one Extraction run and returns its semantic outcome directly.
///
/// The owned request is consumed exactly once. Selection diagnostics and
/// document-local failures are emitted through the observer and do not make the
/// run fallible or stop later documents. The final observation carries the same
/// outcome returned by this operation, and nothing is observed afterward.
///
/// This entry is the production composition: it unpacks the request and runs it
/// against the real filesystem, real EPUB files and the request's own Document
/// extraction. The sequencing itself lives in [`run_with`].
pub fn run(
    request: ExtractionRunRequest,
    observer: &mut impl ExtractionRunObserver,
) -> ExtractionRunOutcome {
    let ExtractionRunRequest {
        selection_inputs,
        document_extraction,
    } = request;
    // Both facts are read from their single owner here, before the inner
    // function receives anything that can extract, so they are fixed at the
    // moment the run begins rather than cached in the request (ADR-0006,
    // ADR-0018).
    let cover_only = document_extraction.is_epub_cover_extraction_configured();
    let applicable_outcome_facts = document_extraction.applicable_outcome_facts();
    run_with(
        selection_inputs,
        cover_only,
        applicable_outcome_facts,
        &FilesystemSearchSurface,
        &EpubFileDeclarations,
        &mut |document| document_extraction.extract(document),
        observer,
    )
}

/// Sequences one Extraction run against whichever collaborators it is given.
///
/// `cover_only` and `applicable_outcome_facts` arrive as values that [`run`]
/// read from Document extraction, their single owner, before calling this
/// function. They stay separate parameters because cover intent is
/// classification input, not an Applicable outcome fact group (ADR-0006).
///
/// `surface` and `declarations` are passed to Document selection unchanged, as
/// the two separate seams ADR-0008 chose over a bundle; selection itself stays
/// real. Every selected document is passed to `extract` exactly once. `extract`
/// is `FnMut` because Document extraction outcomes are not `Clone`: a scripted
/// adapter hands each one out by value, and `Fn` would force interior
/// mutability on it for no production benefit.
///
/// The contract on observations and the returned outcome is the one documented
/// on [`run`], which is this function's only production caller.
///
/// Document selection reports into the same observer rather than through a
/// second seam, so the run never transports selection facts it does not read.
fn run_with(
    selection_inputs: RunSelectionInputs,
    cover_only: bool,
    applicable_outcome_facts: ApplicableOutcomeFacts,
    surface: &dyn DocumentSearchSurface,
    declarations: &dyn EpubDeclarationSource,
    extract: &mut impl FnMut(SelectedDocument) -> DocumentExtractionOutcome,
    observer: &mut impl ExtractionRunObserver,
) -> ExtractionRunOutcome {
    let RunSelectionInputs {
        inputs,
        recursive,
        output,
        epub_filter,
    } = selection_inputs;
    let selected_documents = document_selection::select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive,
            output: output.as_deref(),
            epub_filter: &epub_filter,
            epub_only: cover_only,
        },
        surface,
        declarations,
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
    let mut outcome_accumulator = ExtractionRunOutcomeAccumulator::new(applicable_outcome_facts);

    for selected_document in selected_documents {
        // The run retains only observer-facing facts before transferring the
        // selection-owned handoff into Document extraction exactly once.
        let path = selected_document.get_path().to_path_buf();
        let display_name = selected_document.get_display_name().to_string();
        observer.on_observation(ExtractionRunObservation::DocumentStarted {
            path: path.clone(),
            display_name,
        });

        let (facts, error) = match extract(selected_document) {
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
            outcome_accumulator.record_failed_document();
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
