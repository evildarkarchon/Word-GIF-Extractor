//! Extraction run workflow for document selection, sequencing, and aggregation.

use anyhow::Result;
use std::path::PathBuf;

use crate::document_extraction::{DocumentExtraction, DocumentExtractionOutcome};
use crate::document_selection::{
    self, DocumentSelectionObserver, DocumentSelectionOptions, EpubFilter,
};
use crate::image_write_pipeline::ImageWriteCounts;

/// Options for one extraction run after CLI arguments have been normalized.
///
/// The CLI adapter owns parsing and validation. This module receives only the
/// workflow facts it needs to select and sequence documents and aggregate the
/// final outcome.
pub struct RunOptions {
    /// Input files or directories to scan.
    pub inputs: Vec<PathBuf>,
    /// Whether directory inputs should be scanned recursively.
    pub recursive: bool,
    /// Optional output directory shared by every input file.
    pub output: Option<PathBuf>,
    /// EPUB metadata filter criteria.
    pub epub_filter: EpubFilter,
    /// Ready Document extraction module configured for the run.
    pub document_extraction: DocumentExtraction,
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
    /// Whether any normal batch image was emitted.
    pub has_normal_image_output: bool,
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
    let document_extraction_policy = options.document_extraction.get_policy();
    let cover_only = document_extraction_policy.is_epub_cover_only();
    let selected_documents = document_selection::select_documents(
        DocumentSelectionOptions {
            inputs: &options.inputs,
            recursive: options.recursive,
            output: options.output.as_deref(),
            document_extraction_policy,
            epub_filter: &options.epub_filter,
        },
        observer,
    );
    let mut report = RunReport {
        cover_only,
        documents_to_process: selected_documents.len(),
        ..RunReport::default()
    };

    if selected_documents.is_empty() {
        return Ok(report);
    }

    observer.on_event(RunEvent::ExtractionStarted {
        total: selected_documents.len(),
        cover_only,
    });

    for selected_document in &selected_documents {
        let path = selected_document.get_path().to_path_buf();
        observer.on_event(RunEvent::DocumentStarted {
            path: path.clone(),
            display_name: selected_document.get_display_name().to_string(),
        });

        let (result, error) = match options.document_extraction.extract(selected_document) {
            DocumentExtractionOutcome::Completed(result) => (result, None),
            DocumentExtractionOutcome::Failed { partial, error } => (partial, Some(error)),
        };
        let has_normal_image_output = result.has_normal_image_output();
        for warning in result.warnings {
            observer.on_event(RunEvent::DocumentWarning {
                path: path.clone(),
                message: warning.message(),
            });
        }
        report.record_document_result(result.counts, has_normal_image_output);
        if let Some(error) = error {
            observer.on_event(RunEvent::DocumentError {
                path: path.clone(),
                message: error.to_string(),
            });
        }

        observer.on_event(RunEvent::DocumentFinished { path: path.clone() });
    }

    Ok(report)
}

impl RunReport {
    /// Records Image write facts retained by one completed or failed document outcome.
    fn record_document_result(&mut self, counts: ImageWriteCounts, has_normal_image_output: bool) {
        self.total_counts.extracted += counts.extracted;
        self.total_counts.gifs_routed += counts.gifs_routed;
        self.total_counts.converted += counts.converted;
        self.total_counts.skipped += counts.skipped;

        if counts.extracted > 0 {
            self.documents_with_output += 1;
            self.has_normal_image_output |= has_normal_image_output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_extraction::{DocumentExtraction, DocumentExtractionPolicy};
    use crate::document_selection::{DocumentSelectionDiagnostic, DocumentSelectionProgress};
    use crate::image_format::ImageFormat;
    use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    #[derive(Default)]
    struct RecordingRunObserver {
        events: Vec<RunEvent>,
    }

    impl DocumentSelectionObserver for RecordingRunObserver {
        fn on_document_selection_progress(&mut self, _progress: DocumentSelectionProgress) {}

        fn on_document_selection_diagnostic(&mut self, _diagnostic: DocumentSelectionDiagnostic) {}
    }

    impl RunObserver for RecordingRunObserver {
        fn on_event(&mut self, event: RunEvent) {
            self.events.push(event);
        }
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-run-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    /// Writes a DOCX fixture containing the supplied archive entries in order.
    fn write_docx(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("test DOCX should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        for (name, data) in entries {
            zip.start_file(*name, SimpleFileOptions::default())
                .expect("ZIP entry should start");
            zip.write_all(data)
                .expect("ZIP entry payload should be writable");
        }
        zip.finish().expect("test DOCX should finish");
    }

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
        assert!(report.has_normal_image_output);
    }

    #[test]
    fn run_retains_partial_facts_and_continues_after_document_failure() {
        let temp_dir = temp_test_dir("partial-failure-continuation");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let failing_path = temp_dir.join("failing.docx");
        let succeeding_path = temp_dir.join("succeeding.docx");
        let output_dir = temp_dir.join("output");
        let blocked_gif_output = temp_dir.join("blocked-gifs");
        fs::write(&blocked_gif_output, b"not a directory")
            .expect("blocked GIF destination should be creatable");
        write_docx(
            &failing_path,
            &[
                ("word/media/first.png", b"not actually a png"),
                ("word/media/second.gif", b"GIF89a"),
            ],
        );
        write_docx(
            &succeeding_path,
            &[("word/media/image.png", b"\x89PNG\r\n\x1A\n")],
        );
        let options = RunOptions {
            inputs: vec![failing_path.clone(), succeeding_path.clone()],
            recursive: false,
            output: Some(output_dir.clone()),
            epub_filter: EpubFilter::default(),
            document_extraction: DocumentExtraction::new(
                DocumentExtractionPolicy::NormalImages,
                ImageWritePipeline::new(ImageWritePolicy::new(
                    HashSet::from([ImageFormat::Png, ImageFormat::Gif]),
                    None,
                    Some(blocked_gif_output),
                )),
            ),
        };
        let mut observer = RecordingRunObserver::default();

        let report = run(options, &mut observer).expect("Extraction run should complete");

        assert_eq!(report.total_counts.extracted, 2);
        assert_eq!(report.documents_with_output, 2);
        assert!(report.has_normal_image_output);
        assert!(output_dir.join("failing_1.png").exists());
        assert!(output_dir.join("succeeding.png").exists());

        let warning_index = observer
            .events
            .iter()
            .position(|event| matches!(event, RunEvent::DocumentWarning { path, .. } if path == &failing_path))
            .expect("failed document warning should be emitted");
        let error_index = observer
            .events
            .iter()
            .position(|event| matches!(event, RunEvent::DocumentError { path, message } if path == &failing_path && message.contains("Failed to create output directory")))
            .expect("failed document error should be emitted");
        let failing_finish_indices: Vec<_> = observer
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                matches!(event, RunEvent::DocumentFinished { path } if path == &failing_path)
                    .then_some(index)
            })
            .collect();
        assert_eq!(failing_finish_indices.len(), 1);
        assert!(warning_index < error_index);
        assert!(error_index < failing_finish_indices[0]);
        let succeeding_start = observer
            .events
            .iter()
            .position(|event| matches!(event, RunEvent::DocumentStarted { path, .. } if path == &succeeding_path))
            .expect("later document should start");
        assert!(failing_finish_indices[0] < succeeding_start);

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }
}
