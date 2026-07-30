//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! This tool treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats.

mod conversion;
mod document_extraction;
mod document_selection;
mod epub_declarations;
mod extraction_run;
mod extraction_run_intake;
mod image_format;
mod image_write_pipeline;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

use crate::conversion::{ConversionPolicyError, ConversionTarget};
use crate::document_extraction::DocumentExtractionWarning;
use crate::document_selection::{
    DocumentSelectionDiagnostic, DocumentSelectionPhaseStatus, DocumentSelectionProgress,
    DocumentSelectionScanScope, EpubFilter, EpubMetadataPurpose,
};
use crate::extraction_run::{
    ExtractionOutputKind, ExtractionRunObservation, ExtractionRunObserver, ExtractionRunOutcome,
};
use crate::extraction_run_intake::{ExtractionRunIntakeError, PreRunNotice, PreparedExtractionRun};

/// Conversion target spelling accepted by the CLI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ConversionTargetArg {
    /// JPEG output.
    Jpg,
    /// PNG output.
    Png,
    /// WebP output.
    Webp,
}

impl From<ConversionTargetArg> for ConversionTarget {
    /// Maps CLI target spelling to the Clap-independent conversion target.
    fn from(target: ConversionTargetArg) -> Self {
        match target {
            ConversionTargetArg::Jpg => ConversionTarget::Jpg,
            ConversionTargetArg::Png => ConversionTarget::Png,
            ConversionTargetArg::Webp => ConversionTarget::Webp,
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract images from Word (.docx) and EPUB files", long_about = None)]
struct Args {
    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    inputs: Vec<PathBuf>,

    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    #[arg(short = 'i', long = "input", num_args = 1..)]
    named_inputs: Vec<PathBuf>,

    /// Optional output directory (defaults to each input file's directory)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Recursively search for .docx/.epub files if input is a directory
    #[arg(short, long)]
    recursive: bool,

    /// Image formats to extract (e.g., "png,jpg"). Defaults to all supported formats.
    #[arg(short, long, value_delimiter = ',', num_args = 0..)]
    formats: Option<Vec<String>>,

    /// Extract only cover image from EPUB files
    #[arg(short = 'c', long)]
    cover_only: bool,

    /// Fallback to extracting all images if cover not found (EPUB only, requires --cover-only)
    #[arg(long, requires = "cover_only")]
    cover_fallback: bool,

    /// Filter EPUB files by title (case-insensitive substring match)
    #[arg(long)]
    title: Option<String>,

    /// Filter EPUB files by author (case-insensitive substring match)
    #[arg(long)]
    author: Option<String>,

    /// Convert extracted images to specified format (jpg, png, webp)
    #[arg(short = 'C', long, conflicts_with = "gif_only")]
    convert: Option<ConversionTargetArg>,

    /// JPEG/WebP encoding quality override (1-100, default: 85)
    #[arg(short = 'q', long, requires = "convert", conflicts_with = "lossless", value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: Option<u8>,

    /// Use lossless WebP encoding instead of lossy
    #[arg(short = 'L', long, requires = "convert", conflicts_with = "quality")]
    lossless: bool,

    /// Extract only GIF files (skip all other image formats)
    #[arg(short = 'g', long, conflicts_with = "convert")]
    gif_only: bool,

    /// Separate output directory for GIF files
    #[arg(short = 'G', long)]
    gif_output: Option<PathBuf>,
}

/// Creates a standard progress bar style for collection phases
fn create_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} - {msg}")
        .expect("Invalid progress bar template")
        .progress_chars("=>-")
}

/// Creates a spinner style for phases where total count is unknown
fn create_spinner_style() -> ProgressStyle {
    ProgressStyle::default_spinner()
        .template("{spinner:.green} {msg}")
        .expect("Invalid spinner template")
}

/// Formats EPUB filter criteria for terminal progress messages.
fn epub_filter_description(filter: &EpubFilter) -> String {
    match (&filter.author, &filter.title) {
        (Some(author), Some(title)) => format!("author '{}' and title '{}'", author, title),
        (Some(author), None) => format!("author '{}'", author),
        (None, Some(title)) => format!("title '{}'", title),
        (None, None) => String::new(),
    }
}

/// Renders typed Extraction run intake failures using CLI-specific wording.
fn render_intake_error(error: ExtractionRunIntakeError) -> anyhow::Error {
    match error {
        ExtractionRunIntakeError::CurrentDirectory(error) => error.into(),
        ExtractionRunIntakeError::ConversionPolicy(error) => match error {
            ConversionPolicyError::QualityOutOfRange { quality } => {
                anyhow::anyhow!("--quality must be between 1 and 100 (got {quality})")
            }
            ConversionPolicyError::QualityUnsupportedForPng => anyhow::anyhow!(
                "--quality cannot be used with --convert png (PNG is a lossless format)"
            ),
            ConversionPolicyError::LosslessUnsupportedForTarget { .. } => {
                anyhow::anyhow!("--lossless can only be used with --convert webp")
            }
            ConversionPolicyError::LosslessConflictsWithQuality => {
                anyhow::anyhow!("--lossless cannot be used with --quality")
            }
        },
    }
}

/// Adapts cohesive live Extraction run observations to terminal presentation.
struct IndicatifRunObserver {
    scan_pb: Option<ProgressBar>,
    epub_filter_pb: Option<ProgressBar>,
    epub_dedup_pb: Option<ProgressBar>,
    extraction_pb: Option<ProgressBar>,
}

impl IndicatifRunObserver {
    /// Creates a progress observer with no active progress bars.
    fn new() -> Self {
        Self {
            scan_pb: None,
            epub_filter_pb: None,
            epub_dedup_pb: None,
            extraction_pb: None,
        }
    }

    /// Finishes the extraction progress bar with the final run summary.
    fn finish_extraction(&self, message: String) {
        if let Some(pb) = &self.extraction_pb {
            pb.finish_with_message(message);
        }
    }

    /// Runs a closure while the supplied progress bar is suspended, when present.
    fn suspend_progress_bar<F>(progress_bar: Option<&ProgressBar>, f: F)
    where
        F: FnOnce(),
    {
        if let Some(pb) = progress_bar {
            pb.suspend(f);
        } else {
            f();
        }
    }
}

impl IndicatifRunObserver {
    /// Renders one Document selection progress snapshot without relying on callback deltas.
    fn render_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
        match progress {
            DocumentSelectionProgress::Scanning {
                scope,
                discovered,
                status,
            } => match status {
                DocumentSelectionPhaseStatus::Running => {
                    if scope == DocumentSelectionScanScope::RecursiveDirectories
                        && self.scan_pb.is_none()
                    {
                        let pb = ProgressBar::new_spinner();
                        pb.set_style(create_spinner_style());
                        pb.set_message("Scanning directories for documents...");
                        pb.enable_steady_tick(std::time::Duration::from_millis(100));
                        self.scan_pb = Some(pb);
                    }
                    if discovered > 0
                        && let Some(pb) = &self.scan_pb
                    {
                        pb.set_message(format!("Found {} document(s)...", discovered));
                    }
                }
                DocumentSelectionPhaseStatus::Finished => {
                    if let Some(pb) = self.scan_pb.take() {
                        pb.finish_with_message(format!("Found {} document(s)", discovered));
                    }
                }
            },
            DocumentSelectionProgress::FilteringEpubs {
                filter,
                checked,
                total,
                matching,
                status,
            } => match status {
                DocumentSelectionPhaseStatus::Running => {
                    if self.epub_filter_pb.is_none() {
                        let pb = ProgressBar::new(total as u64);
                        pb.set_style(create_progress_style());
                        pb.set_message(format!(
                            "Filtering EPUBs by {}",
                            epub_filter_description(&filter)
                        ));
                        self.epub_filter_pb = Some(pb);
                    }
                    if let Some(pb) = &self.epub_filter_pb {
                        pb.set_position(checked as u64);
                    }
                }
                DocumentSelectionPhaseStatus::Finished => {
                    if let Some(pb) = self.epub_filter_pb.take() {
                        pb.set_position(checked as u64);
                        pb.finish_with_message(format!("Found {} matching EPUB(s)", matching));
                    }
                }
            },
            DocumentSelectionProgress::DeduplicatingEpubs {
                checked,
                total,
                duplicates_found,
                unique_remaining,
                status,
            } => match status {
                DocumentSelectionPhaseStatus::Running => {
                    if self.epub_dedup_pb.is_none() {
                        let pb = ProgressBar::new(total as u64);
                        pb.set_style(create_progress_style());
                        pb.set_message("Deduplicating EPUBs by metadata");
                        self.epub_dedup_pb = Some(pb);
                    }
                    if let Some(pb) = &self.epub_dedup_pb {
                        pb.set_position(checked as u64);
                    }
                }
                DocumentSelectionPhaseStatus::Finished => {
                    if let Some(pb) = self.epub_dedup_pb.take() {
                        pb.set_position(checked as u64);
                        if duplicates_found > 0 {
                            pb.finish_with_message(format!(
                                "Removed {} duplicate EPUB(s), {} unique remaining",
                                duplicates_found, unique_remaining
                            ));
                        } else {
                            pb.finish_and_clear();
                        }
                    }
                }
            },
        }
    }

    /// Renders one structured Document selection diagnostic with terminal wording.
    fn render_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        match diagnostic {
            DocumentSelectionDiagnostic::MissingInput { path } => {
                eprintln!("Warning: Input path does not exist: {}", path.display());
            }
            DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail } => {
                let render = || {
                    eprintln!(
                        "Warning: Could not inspect {} during document discovery: {}",
                        path.display(),
                        detail
                    );
                };
                // Recursive discovery can warn while its spinner is active; suspending
                // prevents the next redraw from corrupting or overwriting the warning.
                Self::suspend_progress_bar(self.scan_pb.as_ref(), render);
            }
            DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                path,
                purpose,
                detail,
            } => match purpose {
                EpubMetadataPurpose::Filtering => {
                    let render = || {
                        eprintln!("Warning: Could not read {}: {}", path.display(), detail);
                    };
                    Self::suspend_progress_bar(self.epub_filter_pb.as_ref(), render);
                }
                EpubMetadataPurpose::Deduplication => {
                    let render = || {
                        eprintln!(
                            "Warning: Could not read EPUB metadata from {} during deduplication; using filename fallback: {}",
                            path.display(),
                            detail
                        );
                    };
                    Self::suspend_progress_bar(self.epub_dedup_pb.as_ref(), render);
                }
            },
        }
    }
}

impl ExtractionRunObserver for IndicatifRunObserver {
    /// Renders one observation without exposing the nested selection observer seam.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        match observation {
            ExtractionRunObservation::DocumentSelectionProgress(progress) => {
                self.render_document_selection_progress(progress);
            }
            ExtractionRunObservation::DocumentSelectionDiagnostic(diagnostic) => {
                self.render_document_selection_diagnostic(diagnostic);
            }
            ExtractionRunObservation::ExtractionStarted { total, cover_only } => {
                let pb = ProgressBar::new(total as u64);
                pb.set_style(create_progress_style());
                let extraction_msg = if cover_only {
                    "Extracting cover images"
                } else {
                    "Extracting images from documents"
                };
                pb.set_message(extraction_msg);
                self.extraction_pb = Some(pb);
            }
            ExtractionRunObservation::DocumentStarted { display_name, .. } => {
                if let Some(pb) = &self.extraction_pb {
                    pb.set_message(display_name);
                }
            }
            ExtractionRunObservation::DocumentError { path, message } => {
                Self::suspend_progress_bar(self.extraction_pb.as_ref(), || {
                    eprintln!("Error processing {}: {}", path.display(), message);
                });
            }
            ExtractionRunObservation::DocumentWarning { warning, .. } => {
                // The document path stays run context only; presentation adds the
                // prefix and nothing else to the Document extraction-owned body.
                Self::suspend_progress_bar(self.extraction_pb.as_ref(), || {
                    eprintln!("{}", document_warning_line(&warning));
                });
            }
            ExtractionRunObservation::DocumentFinished { .. } => {
                if let Some(pb) = &self.extraction_pb {
                    pb.inc(1);
                }
            }
            ExtractionRunObservation::Terminal(outcome) => {
                if matches!(outcome, ExtractionRunOutcome::NoDocuments) {
                    println!("{}", final_summary_message(&outcome));
                } else {
                    self.finish_extraction(final_summary_message(&outcome));
                }
            }
        }
    }
}

/// Builds the terminal line shown for one opaque Document extraction warning.
///
/// Presentation is limited to the `Warning:` prefix; the stable body is read
/// from the warning value and the originating document path is not added.
fn document_warning_line(warning: &DocumentExtractionWarning) -> String {
    format!("Warning: {}", warning.get_message())
}

/// Builds the final extraction summary shown in the terminal.
fn final_summary_message(outcome: &ExtractionRunOutcome) -> String {
    match outcome {
        ExtractionRunOutcome::NoDocuments => "No documents found to process.".to_string(),
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images) => {
            "No images found".to_string()
        }
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Covers) => {
            "No cover images found".to_string()
        }
        ExtractionRunOutcome::ProducedOutput(output) => {
            let item_name = match output.output_kind() {
                ExtractionOutputKind::Images => "image(s)",
                ExtractionOutputKind::Covers => "cover(s)",
            };

            match (output.conversion(), output.gif_routing()) {
                (Some(conversion), Some(gif_routing)) => {
                    // D-04: Combined conversion + GIF routing message
                    format!(
                        "Extracted {} {}, converted {}, skipped {}, routed {} GIF(s) to {} from {} document(s)",
                        output.emitted_images(),
                        item_name,
                        conversion.converted_images(),
                        conversion.skipped_conversions(),
                        gif_routing.routed_gifs(),
                        gif_routing.destination().display(),
                        output.documents_with_output()
                    )
                }
                (Some(conversion), None) => {
                    // D-01: Conversion stats only
                    format!(
                        "Extracted {} {}, converted {}, skipped {} from {} document(s)",
                        output.emitted_images(),
                        item_name,
                        conversion.converted_images(),
                        conversion.skipped_conversions(),
                        output.documents_with_output()
                    )
                }
                (None, Some(gif_routing)) => {
                    // Existing GIF routing message (no conversion) -- unchanged
                    format!(
                        "Extracted {} {}, routed {} GIF(s) to {} from {} document(s)",
                        output.emitted_images(),
                        item_name,
                        gif_routing.routed_gifs(),
                        gif_routing.destination().display(),
                        output.documents_with_output()
                    )
                }
                (None, None) => {
                    // Existing default message -- unchanged
                    format!(
                        "Extracted {} {} from {} document(s)",
                        output.emitted_images(),
                        item_name,
                        output.documents_with_output()
                    )
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let PreparedExtractionRun { request, notices } =
        extraction_run_intake::prepare(args).map_err(render_intake_error)?;

    for notice in notices {
        match notice {
            PreRunNotice::DefaultedInput { path } => {
                println!(
                    "No input path specified, using current directory: {}",
                    path.display()
                );
            }
            PreRunNotice::IgnoredFormat { format } => {
                eprintln!("Warning: Unrecognized format '{}' ignored", format);
            }
        }
    }

    let mut observer = IndicatifRunObserver::new();
    extraction_run::run(request, &mut observer);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extraction_run::{ConversionFacts, GifRoutingFacts};
    use indicatif::{ProgressDrawTarget, TermLike};
    use std::fs;
    use std::io;
    use std::io::Write as _;
    use std::num::NonZeroUsize;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    #[derive(Debug, Default)]
    struct TerminalActivity {
        clear_lines: usize,
        writes: usize,
    }

    #[derive(Debug)]
    struct RecordingTerm {
        activity: Arc<Mutex<TerminalActivity>>,
    }

    impl TermLike for RecordingTerm {
        fn width(&self) -> u16 {
            80
        }

        fn move_cursor_up(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_cursor_down(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_cursor_right(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_cursor_left(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn write_line(&self, _s: &str) -> io::Result<()> {
            self.activity
                .lock()
                .expect("terminal activity should be available")
                .writes += 1;
            Ok(())
        }

        fn write_str(&self, _s: &str) -> io::Result<()> {
            self.activity
                .lock()
                .expect("terminal activity should be available")
                .writes += 1;
            Ok(())
        }

        fn clear_line(&self) -> io::Result<()> {
            self.activity
                .lock()
                .expect("terminal activity should be available")
                .clear_lines += 1;
            Ok(())
        }

        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FilesystemIndicatifObserver {
        inner: IndicatifRunObserver,
        remove_on_scan_start: Option<PathBuf>,
        terminal_activity: Arc<Mutex<TerminalActivity>>,
        discovery_diagnostics: usize,
        diagnostic_clear_lines: usize,
        diagnostic_writes: usize,
    }

    impl ExtractionRunObserver for FilesystemIndicatifObserver {
        /// Delegates observations while inducing one real post-classification traversal failure.
        fn on_observation(&mut self, observation: ExtractionRunObservation) {
            let starts_recursive_scan = matches!(
                &observation,
                ExtractionRunObservation::DocumentSelectionProgress(
                    DocumentSelectionProgress::Scanning {
                        scope: DocumentSelectionScanScope::RecursiveDirectories,
                        discovered: 0,
                        status: DocumentSelectionPhaseStatus::Running,
                    }
                )
            );
            let is_discovery_diagnostic = matches!(
                &observation,
                ExtractionRunObservation::DocumentSelectionDiagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { .. }
                )
            );
            // Snapshot before delegation so only the diagnostic's suspend operation
            // contributes to the clear/redraw deltas measured below.
            let before = if is_discovery_diagnostic {
                let activity = self
                    .terminal_activity
                    .lock()
                    .expect("terminal activity should be available");
                Some((activity.clear_lines, activity.writes))
            } else {
                None
            };

            self.inner.on_observation(observation);

            if starts_recursive_scan {
                // Delegation creates the production spinner. Attach a drawing terminal
                // before removing the classified root so traversal fails while it is active.
                let progress = self
                    .inner
                    .scan_pb
                    .as_ref()
                    .expect("recursive scanning should create a spinner");
                progress.disable_steady_tick();
                progress.set_draw_target(ProgressDrawTarget::term_like(Box::new(RecordingTerm {
                    activity: Arc::clone(&self.terminal_activity),
                })));
                progress.tick();
                if let Some(directory) = self.remove_on_scan_start.take() {
                    fs::remove_dir(directory)
                        .expect("classified directory should be removable before traversal");
                }
            }

            if let Some((clear_lines, writes)) = before {
                let activity = self
                    .terminal_activity
                    .lock()
                    .expect("terminal activity should be available");
                self.discovery_diagnostics += 1;
                self.diagnostic_clear_lines += activity.clear_lines - clear_lines;
                self.diagnostic_writes += activity.writes - writes;
            }
        }
    }

    /// Delegating observer that measures suspension around each warning presentation.
    ///
    /// It captures the opaque warning values the run transported so presentation
    /// can be asserted without the terminal test owning any stable wording.
    struct WarningPresentationObserver {
        inner: IndicatifRunObserver,
        terminal_activity: Arc<Mutex<TerminalActivity>>,
        warnings: Vec<DocumentExtractionWarning>,
        warning_clear_lines: usize,
        warning_writes: usize,
    }

    impl ExtractionRunObserver for WarningPresentationObserver {
        /// Delegates observations while recording warning values and suspension effects.
        fn on_observation(&mut self, observation: ExtractionRunObservation) {
            let starts_extraction = matches!(
                &observation,
                ExtractionRunObservation::ExtractionStarted { .. }
            );
            let warning = match &observation {
                ExtractionRunObservation::DocumentWarning { warning, .. } => Some(warning.clone()),
                _ => None,
            };
            // Snapshot before delegation so only the warning's suspend operation
            // contributes to the clear/redraw deltas measured below.
            let before = warning.as_ref().map(|_| {
                let activity = self
                    .terminal_activity
                    .lock()
                    .expect("terminal activity should be available");
                (activity.clear_lines, activity.writes)
            });

            self.inner.on_observation(observation);

            if starts_extraction {
                // Delegation creates the production progress bar. Attach a drawing
                // terminal and draw once so a later warning must suspend a live bar.
                let progress = self
                    .inner
                    .extraction_pb
                    .as_ref()
                    .expect("extraction start should create progress");
                progress.set_draw_target(ProgressDrawTarget::term_like(Box::new(RecordingTerm {
                    activity: Arc::clone(&self.terminal_activity),
                })));
                progress.tick();
            }

            if let (Some(warning), Some((clear_lines, writes))) = (warning, before) {
                let activity = self
                    .terminal_activity
                    .lock()
                    .expect("terminal activity should be available");
                self.warnings.push(warning);
                self.warning_clear_lines += activity.clear_lines - clear_lines;
                self.warning_writes += activity.writes - writes;
            }
        }
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

    /// Returns an isolated temporary directory for one observer test.
    fn observer_temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-observer-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    /// Builds one state-valid produced outcome through the production constructor.
    fn produced_outcome(
        output_kind: ExtractionOutputKind,
        emitted_images: usize,
        documents_with_output: usize,
        conversion: Option<ConversionFacts>,
        gif_routing: Option<(usize, PathBuf)>,
    ) -> ExtractionRunOutcome {
        ExtractionRunOutcome::try_produced(
            output_kind,
            NonZeroUsize::new(emitted_images).expect("produced output must be positive"),
            NonZeroUsize::new(documents_with_output)
                .expect("documents with output must be positive"),
            conversion,
            gif_routing.map(|(routed_gifs, destination)| {
                GifRoutingFacts::new(
                    NonZeroUsize::new(routed_gifs).expect("routed GIF count must be positive"),
                    destination,
                )
            }),
        )
        .expect("terminal test outcome should be semantically valid")
    }

    /// Verifies that the terminal observation finishes progress with adapter-owned wording.
    fn assert_terminal_observation_finishes_extraction(
        outcome: ExtractionRunOutcome,
        expected_message: &str,
    ) {
        let mut observer = IndicatifRunObserver::new();
        let cover_only = match &outcome {
            ExtractionRunOutcome::NoDocuments => false,
            ExtractionRunOutcome::NoOutput(output_kind) => {
                *output_kind == ExtractionOutputKind::Covers
            }
            ExtractionRunOutcome::ProducedOutput(output) => {
                output.output_kind() == ExtractionOutputKind::Covers
            }
        };
        observer.on_observation(ExtractionRunObservation::ExtractionStarted {
            total: 1,
            cover_only,
        });
        let progress = observer
            .extraction_pb
            .as_ref()
            .expect("Extraction start should create progress")
            .clone();

        observer.on_observation(ExtractionRunObservation::Terminal(outcome));

        assert!(progress.is_finished());
        assert_eq!(progress.message(), expected_message);
    }

    #[test]
    fn test_convert_flag_parses_all_formats() {
        let args = Args::try_parse_from(["test", "--convert", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Jpg));
        let args = Args::try_parse_from(["test", "--convert", "png"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Png));
        let args = Args::try_parse_from(["test", "--convert", "webp"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Webp));
    }

    #[test]
    fn terminal_epub_filter_description_preserves_existing_wording() {
        assert_eq!(
            epub_filter_description(&EpubFilter {
                title: Some("Magic Book".to_string()),
                author: Some("Test Author".to_string()),
            }),
            "author 'Test Author' and title 'Magic Book'"
        );
    }

    #[test]
    fn test_convert_short_flag() {
        let args = Args::try_parse_from(["test", "-C", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Jpg));
    }

    #[test]
    fn test_quality_with_convert_jpg() {
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "90"]).unwrap();
        assert_eq!(args.quality, Some(90));
    }

    #[test]
    fn test_quality_with_convert_webp() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--quality", "90"]).unwrap();
        assert_eq!(args.quality, Some(90));
    }

    #[test]
    fn test_quality_range_validation() {
        assert!(Args::try_parse_from(["test", "--convert", "jpg", "--quality", "0"]).is_err());
        assert!(Args::try_parse_from(["test", "--convert", "jpg", "--quality", "101"]).is_err());
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "1"]).unwrap();
        assert_eq!(args.quality, Some(1));
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "100"]).unwrap();
        assert_eq!(args.quality, Some(100));
    }

    #[test]
    fn test_quality_requires_convert() {
        assert!(Args::try_parse_from(["test", "--quality", "90"]).is_err());
    }

    #[test]
    fn test_quality_with_png_error() {
        let args = Args::try_parse_from(["test", "--convert", "png", "--quality", "90"]).unwrap();
        let err_msg = extraction_run_intake::prepare(args)
            .err()
            .map(render_intake_error)
            .expect("PNG quality should fail semantic intake")
            .to_string();
        assert!(
            err_msg.contains("--quality cannot be used with --convert png"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_convert_and_gif_only_conflict() {
        assert!(Args::try_parse_from(["test", "--convert", "jpg", "--gif-only"]).is_err());
    }

    #[test]
    fn test_gif_only_short_flag() {
        let args = Args::try_parse_from(["test", "-g"]).unwrap();
        assert!(args.gif_only);
    }

    #[test]
    fn test_gif_output_independent() {
        let args = Args::try_parse_from(["test", "--gif-output", "/tmp/gifs"]).unwrap();
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_gif_output_short_flag() {
        let args = Args::try_parse_from(["test", "-G", "/tmp/gifs"]).unwrap();
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_lossless_with_convert_webp() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--lossless"]).unwrap();
        assert!(args.lossless);
    }

    #[test]
    fn test_lossless_requires_convert() {
        assert!(Args::try_parse_from(["test", "--lossless"]).is_err());
    }

    #[test]
    fn test_lossless_conflicts_with_quality() {
        assert!(
            Args::try_parse_from(["test", "--convert", "webp", "--lossless", "--quality", "90"])
                .is_err()
        );
    }

    #[test]
    fn test_lossless_with_jpg_error() {
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--lossless"]).unwrap();
        let err_msg = extraction_run_intake::prepare(args)
            .err()
            .map(render_intake_error)
            .expect("JPEG lossless should fail semantic intake")
            .to_string();
        assert!(
            err_msg.contains("--lossless can only be used with --convert webp"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_lossless_with_png_error() {
        let args = Args::try_parse_from(["test", "--convert", "png", "--lossless"]).unwrap();
        let err_msg = extraction_run_intake::prepare(args)
            .err()
            .map(render_intake_error)
            .expect("PNG lossless should fail semantic intake")
            .to_string();
        assert!(
            err_msg.contains("--lossless can only be used with --convert webp"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_lossless_short_flag() {
        let args = Args::try_parse_from(["test", "-L", "--convert", "webp"]).unwrap();
        assert!(args.lossless);
    }

    #[test]
    fn test_existing_flags_unchanged() {
        let args = Args::try_parse_from([
            "test",
            "-o",
            "/tmp",
            "-r",
            "-f",
            "png,jpg",
            "-c",
            "--cover-fallback",
        ])
        .unwrap();
        assert_eq!(args.output, Some(PathBuf::from("/tmp")));
        assert!(args.recursive);
        assert!(args.cover_only);
        assert!(args.cover_fallback);
    }

    #[test]
    fn test_gif_only_and_gif_output_both_set() {
        let args =
            Args::try_parse_from(["test", "--gif-only", "--gif-output", "/tmp/gifs"]).unwrap();
        assert!(args.gif_only);
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_gif_output_without_gif_only() {
        let args = Args::try_parse_from(["test", "--gif-output", "/tmp/gifs"]).unwrap();
        assert!(!args.gif_only);
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_gif_only_without_gif_output() {
        let args = Args::try_parse_from(["test", "--gif-only"]).unwrap();
        assert!(args.gif_only);
        assert!(args.gif_output.is_none());
    }

    #[test]
    fn conversion_summary_reports_preserved_matching_source_as_unconverted() {
        let outcome = produced_outcome(
            ExtractionOutputKind::Images,
            1,
            1,
            Some(ConversionFacts::new(0, 0)),
            None,
        );

        let message = final_summary_message(&outcome);

        assert_eq!(
            message,
            "Extracted 1 image(s), converted 0, skipped 0 from 1 document(s)"
        );
    }

    #[test]
    fn combined_conversion_and_gif_summary_uses_semantic_outcome() {
        let gif_dir = std::path::PathBuf::from("/tmp/gifs");
        let outcome = produced_outcome(
            ExtractionOutputKind::Images,
            10,
            4,
            Some(ConversionFacts::new(5, 2)),
            Some((3, gif_dir.clone())),
        );

        let message = final_summary_message(&outcome);

        assert_eq!(
            message,
            format!(
                "Extracted 10 image(s), converted 5, skipped 2, routed 3 GIF(s) to {} from 4 document(s)",
                gif_dir.display()
            )
        );
    }

    #[test]
    fn epub_cover_fallback_summary_reports_normal_images() {
        let outcome = produced_outcome(ExtractionOutputKind::Images, 2, 1, None, None);

        let message = final_summary_message(&outcome);

        assert_eq!(message, "Extracted 2 image(s) from 1 document(s)");
    }

    #[test]
    fn no_documents_summary_preserves_existing_wording() {
        assert_eq!(
            final_summary_message(&ExtractionRunOutcome::NoDocuments),
            "No documents found to process."
        );
    }

    /// Verifies a real recursive failure suspends and then normally completes the scan spinner.
    #[test]
    fn recursive_discovery_diagnostic_suspends_active_scan_spinner() {
        let temp_dir = observer_temp_test_dir("recursive-suspension");
        let requested_directory = temp_dir.join("requested");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        let input = requested_directory.to_string_lossy().into_owned();
        let args = Args::try_parse_from(["test", input.as_str(), "--recursive"])
            .expect("recursive arguments should parse");
        let prepared =
            extraction_run_intake::prepare(args).expect("Extraction run intake should succeed");
        let terminal_activity = Arc::new(Mutex::new(TerminalActivity::default()));
        let mut observer = FilesystemIndicatifObserver {
            inner: IndicatifRunObserver::new(),
            remove_on_scan_start: Some(requested_directory),
            terminal_activity,
            discovery_diagnostics: 0,
            diagnostic_clear_lines: 0,
            diagnostic_writes: 0,
        };

        let outcome = extraction_run::run(prepared.request, &mut observer);

        assert_eq!(outcome, ExtractionRunOutcome::NoDocuments);
        assert_eq!(observer.discovery_diagnostics, 1);
        assert!(
            observer.diagnostic_clear_lines > 0,
            "suspension should clear the active spinner before rendering the warning"
        );
        assert!(
            observer.diagnostic_writes > 0,
            "suspension should redraw the active spinner after rendering the warning"
        );
        assert!(observer.inner.scan_pb.is_none());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    /// Verifies warning presentation adds one prefix and suspends the extraction bar.
    ///
    /// The stable body stays owned by Document extraction, so the assertions
    /// compare against the transported value rather than restating any wording.
    #[test]
    fn document_warning_presentation_adds_one_prefix_and_suspends_extraction_progress() {
        let temp_dir = observer_temp_test_dir("warning-presentation");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let document_path = temp_dir.join("warned.docx");
        let output_dir = temp_dir.join("output");
        write_docx(
            &document_path,
            &[("word/media/only.png", b"not actually a png")],
        );
        let args = Args::try_parse_from([
            "test",
            document_path.to_string_lossy().as_ref(),
            "--output",
            output_dir.to_string_lossy().as_ref(),
        ])
        .expect("warning fixture arguments should parse");
        let prepared =
            extraction_run_intake::prepare(args).expect("Extraction run intake should succeed");
        let mut observer = WarningPresentationObserver {
            inner: IndicatifRunObserver::new(),
            terminal_activity: Arc::new(Mutex::new(TerminalActivity::default())),
            warnings: Vec::new(),
            warning_clear_lines: 0,
            warning_writes: 0,
        };

        extraction_run::run(prepared.request, &mut observer);

        assert_eq!(observer.warnings.len(), 1);
        assert!(
            observer.warning_clear_lines > 0,
            "suspension should clear the active extraction bar before rendering the warning"
        );
        assert!(
            observer.warning_writes > 0,
            "suspension should redraw the active extraction bar after rendering the warning"
        );

        // Stripping exactly one prefix must leave the transported body untouched,
        // which rules out both a missing prefix and a doubled one without this
        // test knowing what the body says.
        let warning = &observer.warnings[0];
        let rendered = document_warning_line(warning);
        assert_eq!(
            rendered.strip_prefix("Warning: "),
            Some(warning.get_message())
        );
        assert!(
            !rendered.contains(&document_path.display().to_string()),
            "the document path is run context only and must not reach presentation"
        );

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn image_no_output_summary_preserves_existing_wording() {
        assert_eq!(
            final_summary_message(&ExtractionRunOutcome::NoOutput(
                ExtractionOutputKind::Images
            )),
            "No images found"
        );
    }

    #[test]
    fn cover_no_output_summary_preserves_existing_wording() {
        assert_eq!(
            final_summary_message(&ExtractionRunOutcome::NoOutput(
                ExtractionOutputKind::Covers
            )),
            "No cover images found"
        );
    }

    #[test]
    fn default_output_summary_preserves_existing_wording() {
        let outcome = produced_outcome(ExtractionOutputKind::Images, 3, 2, None, None);

        assert_eq!(
            final_summary_message(&outcome),
            "Extracted 3 image(s) from 2 document(s)"
        );
    }

    #[test]
    fn required_cover_summary_preserves_existing_wording() {
        let outcome = produced_outcome(ExtractionOutputKind::Covers, 1, 1, None, None);

        assert_eq!(
            final_summary_message(&outcome),
            "Extracted 1 cover(s) from 1 document(s)"
        );
    }

    #[test]
    fn gif_routing_summary_preserves_existing_wording() {
        let gif_dir = PathBuf::from("/tmp/gifs");
        let outcome = produced_outcome(
            ExtractionOutputKind::Images,
            2,
            1,
            None,
            Some((1, gif_dir.clone())),
        );

        assert_eq!(
            final_summary_message(&outcome),
            format!(
                "Extracted 2 image(s), routed 1 GIF(s) to {} from 1 document(s)",
                gif_dir.display()
            )
        );
    }

    #[test]
    fn terminal_observer_finishes_every_nonempty_outcome_with_existing_wording() {
        let gif_dir = PathBuf::from("/tmp/gifs");
        let cases = [
            (
                ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images),
                "No images found".to_string(),
            ),
            (
                ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Covers),
                "No cover images found".to_string(),
            ),
            (
                produced_outcome(ExtractionOutputKind::Images, 3, 2, None, None),
                "Extracted 3 image(s) from 2 document(s)".to_string(),
            ),
            (
                produced_outcome(
                    ExtractionOutputKind::Images,
                    3,
                    2,
                    Some(ConversionFacts::new(2, 1)),
                    None,
                ),
                "Extracted 3 image(s), converted 2, skipped 1 from 2 document(s)".to_string(),
            ),
            (
                produced_outcome(
                    ExtractionOutputKind::Images,
                    3,
                    2,
                    None,
                    Some((1, gif_dir.clone())),
                ),
                format!(
                    "Extracted 3 image(s), routed 1 GIF(s) to {} from 2 document(s)",
                    gif_dir.display()
                ),
            ),
            (
                produced_outcome(
                    ExtractionOutputKind::Images,
                    4,
                    2,
                    Some(ConversionFacts::new(2, 1)),
                    Some((1, gif_dir.clone())),
                ),
                format!(
                    "Extracted 4 image(s), converted 2, skipped 1, routed 1 GIF(s) to {} from 2 document(s)",
                    gif_dir.display()
                ),
            ),
            (
                produced_outcome(ExtractionOutputKind::Covers, 1, 1, None, None),
                "Extracted 1 cover(s) from 1 document(s)".to_string(),
            ),
            (
                produced_outcome(ExtractionOutputKind::Images, 2, 1, None, None),
                "Extracted 2 image(s) from 1 document(s)".to_string(),
            ),
        ];

        for (outcome, expected_message) in cases {
            assert_terminal_observation_finishes_extraction(outcome, &expected_message);
        }
    }

    #[test]
    fn test_convert_and_lossless_args_threaded() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--lossless"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Webp));
        assert!(args.lossless);
    }
}
