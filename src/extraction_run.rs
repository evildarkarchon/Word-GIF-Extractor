//! Extraction run workflow for document discovery, dispatch, and aggregation.

use anyhow::Result;
use std::path::PathBuf;

use crate::document_selection::{
    self, DocumentSelectionObserver, DocumentSelectionOptions, EpubFilter, SelectedDocument,
};
use crate::docx;
use crate::epub;
use crate::image_write_pipeline::{ImageWriteCounts, ImageWritePipeline, ImageWriteResult};

/// Options for one extraction run after CLI arguments have been normalized.
///
/// The CLI adapter owns parsing and validation. This module receives only the
/// workflow facts it needs to discover documents, dispatch them, and aggregate
/// the final outcome.
pub struct RunOptions {
    /// Input files or directories to scan.
    pub inputs: Vec<PathBuf>,
    /// Whether directory inputs should be scanned recursively.
    pub recursive: bool,
    /// Optional output directory shared by every input file.
    pub output: Option<PathBuf>,
    /// Extract only cover images from EPUB files.
    pub cover_only: bool,
    /// Extract all EPUB images if a cover cannot be found.
    pub cover_fallback: bool,
    /// EPUB metadata filter criteria.
    pub epub_filter: EpubFilter,
    /// Ready Image write pipeline configured for the run.
    pub image_write_pipeline: ImageWritePipeline,
}

/// Aggregated outcome from an extraction run.
///
/// The report keeps workflow facts behind the extraction run seam so callers do
/// not have to infer summary state from raw counters.
#[derive(Debug, Default, Clone, Copy)]
pub struct RunReport {
    /// Total write/conversion counts across every processed document.
    pub total_counts: ImageWriteCounts,
    /// Number of documents that produced at least one output image.
    pub documents_with_output: usize,
    /// Whether any DOCX document produced output images.
    pub has_docx_images: bool,
    /// Whether this run was requested in cover-only mode.
    pub cover_only: bool,
    /// Number of documents selected for processing after filtering and dedupe.
    pub documents_to_process: usize,
}

/// Domain event emitted while an extraction run progresses.
///
/// Events describe extraction-run facts, not terminal UI commands. The CLI
/// adapter decides how to render them as progress bars or warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// Document extraction has started.
    ExtractionStarted { total: usize, cover_only: bool },
    /// A document is about to be processed.
    DocumentStarted { path: PathBuf, display_name: String },
    /// A document failed to process; the run will continue.
    DocumentError { path: PathBuf, message: String },
    /// A document produced a user-visible warning while processing.
    DocumentWarning { path: PathBuf, message: String },
    /// A document has finished processing, successfully or not.
    DocumentFinished { path: PathBuf },
}

/// Observer for extraction-run events and the nested Document selection workflow.
pub trait RunObserver: DocumentSelectionObserver {
    /// Handles one event emitted by the extraction run.
    fn on_event(&mut self, event: RunEvent);
}

/// Executes one extraction run and returns its aggregate report.
///
/// Per-document failures are emitted as events and do not abort the run. Setup
/// failures that prevent the run from starting are returned as errors.
pub fn run(options: RunOptions, observer: &mut impl RunObserver) -> Result<RunReport> {
    let selected_documents = document_selection::select_documents(
        DocumentSelectionOptions {
            inputs: &options.inputs,
            recursive: options.recursive,
            output: options.output.as_deref(),
            cover_only: options.cover_only,
            epub_filter: &options.epub_filter,
        },
        observer,
    );
    let mut report = RunReport {
        cover_only: options.cover_only,
        documents_to_process: selected_documents.len(),
        ..RunReport::default()
    };

    if selected_documents.is_empty() {
        return Ok(report);
    }

    observer.on_event(RunEvent::ExtractionStarted {
        total: selected_documents.len(),
        cover_only: options.cover_only,
    });

    for selected_document in &selected_documents {
        let path = selected_document.path().to_path_buf();
        observer.on_event(RunEvent::DocumentStarted {
            path: path.clone(),
            display_name: selected_document.display_name().to_string(),
        });

        match process_file(
            selected_document,
            options.cover_only,
            options.cover_fallback,
            &options.image_write_pipeline,
        ) {
            Ok(result) => {
                for warning in result.warnings {
                    observer.on_event(RunEvent::DocumentWarning {
                        path: path.clone(),
                        message: warning.message(),
                    });
                }
                report.record_document_result(result.counts, selected_document.is_docx());
            }
            Err(e) => observer.on_event(RunEvent::DocumentError {
                path: path.clone(),
                message: e.to_string(),
            }),
        }

        observer.on_event(RunEvent::DocumentFinished { path: path.clone() });
    }

    Ok(report)
}

impl RunReport {
    /// Records the counts for one successfully processed document.
    fn record_document_result(&mut self, counts: ImageWriteCounts, is_docx: bool) {
        self.total_counts.extracted += counts.extracted;
        self.total_counts.gifs_routed += counts.gifs_routed;
        self.total_counts.converted += counts.converted;
        self.total_counts.skipped += counts.skipped;

        if counts.extracted > 0 {
            self.documents_with_output += 1;
            if is_docx {
                self.has_docx_images = true;
            }
        }
    }
}

/// Processes a single file based on its type.
fn process_file(
    selected_document: &SelectedDocument,
    cover_only: bool,
    cover_fallback: bool,
    pipeline: &ImageWritePipeline,
) -> Result<ImageWriteResult> {
    match selected_document {
        SelectedDocument::Docx { .. } => docx::process_file(
            selected_document.path(),
            selected_document.output_dir(),
            selected_document.base_name(),
            pipeline,
        ),
        SelectedDocument::Epub { .. } => epub::process_file(
            selected_document.path(),
            selected_document.output_dir(),
            selected_document.base_name(),
            cover_only,
            cover_fallback,
            pipeline,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_report_records_counts_and_docx_output_state() {
        let mut report = RunReport::default();

        report.record_document_result(
            ImageWriteCounts {
                extracted: 2,
                gifs_routed: 1,
                converted: 1,
                skipped: 0,
            },
            true,
        );
        report.record_document_result(
            ImageWriteCounts {
                extracted: 0,
                gifs_routed: 0,
                converted: 0,
                skipped: 1,
            },
            false,
        );

        assert_eq!(report.total_counts.extracted, 2);
        assert_eq!(report.total_counts.gifs_routed, 1);
        assert_eq!(report.total_counts.converted, 1);
        assert_eq!(report.total_counts.skipped, 1);
        assert_eq!(report.documents_with_output, 1);
        assert!(report.has_docx_images);
    }
}
