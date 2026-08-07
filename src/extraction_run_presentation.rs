//! Extraction run presentation: Extraction run observations rendered for a terminal.
//!
//! This module owns every word the tool says: progress display, Document selection
//! progress and diagnostics, per-document errors and warnings, pre-run notices,
//! intake error wording, and the four terminal summary templates. It owns none of
//! the decisions behind them — outcome classification and observation ordering
//! belong to the Extraction run.
//!
//! # The output destination
//!
//! Presentation writes to three sinks: the progress display, standard output and
//! standard error. They are coupled rather than independent, because every direct
//! write is wrapped in a progress-display suspend so that the next redraw cannot
//! overwrite it. [`TerminalOutput`] bundles all three so the coupling has one
//! owner, and it has two constructors: [`TerminalOutput::stdio`] for production and
//! [`TerminalOutput::captured`] for a run whose entire terminal side can be
//! asserted. Supplying the destination at construction is what lets a test observe
//! suspension without reaching into a progress display afterwards.
//!
//! Routing the two text streams through the progress library instead was rejected:
//! it collapses the standard-output and standard-error split that pre-run notice
//! behaviour depends on, which would be a behaviour change rather than a move.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle, TermLike};

use crate::conversion::ConversionPolicyError;
use crate::document_extraction::DocumentExtractionWarning;
use crate::extraction_run_intake::{
    Args, ExtractionRunIntakeError, PreRunNotice, PreparedExtractionRun,
};
use crate::extraction_run_observation::{
    DocumentDiscoveryScope, EpubMetadataPurpose, ExtractionOutputKind, ExtractionRunObservation,
    ExtractionRunObserver, ExtractionRunOutcome,
};

/// Runs one extraction end to end and renders it to the supplied destination.
///
/// This is the whole of what the binary does after `clap` has parsed its
/// arguments. Intake failures are returned rather than rendered here: the process
/// exit path already prints a returned error, and returning one keeps the failure
/// wording out of the destination a caller is capturing.
pub fn run_cli(args: Args, output: TerminalOutput) -> Result<()> {
    let PreparedExtractionRun { request, notices } =
        crate::extraction_run_intake::prepare(args).map_err(render_intake_error)?;

    let mut presentation = ExtractionRunPresentation::new(output);
    presentation.render_pre_run_notices(notices);
    crate::extraction_run::run(request, &mut presentation);

    Ok(())
}

/// What a captured progress display did, and what it said doing it.
///
/// Clearing and redrawing is what suspension does, so the two counters are how a
/// captured run shows that a direct write was made without a redraw racing it.
/// The counters alone cannot show *what* was drawn, which leaves the terminal
/// summaries of the two output-bearing outcomes -- they are rendered onto the
/// progress display and never touch a text stream -- unreadable, so the rendered
/// text is kept alongside them.
#[derive(Debug, Default)]
struct TerminalActivity {
    clear_lines: usize,
    writes: usize,
    rendered: String,
}

impl TerminalActivity {
    /// Records one drawn terminal line, counting it and keeping its text.
    ///
    /// Both write operations land here terminated, because the progress library
    /// makes one call per drawn line and uses the unterminated one only for the
    /// last line of a draw, to leave the terminal cursor on it. Terminating both
    /// keeps the readback one line per drawn line instead of running consecutive
    /// redraws together.
    fn record_write(&mut self, line: &str) {
        self.writes += 1;
        self.rendered.push_str(line);
        self.rendered.push('\n');
    }
}

/// Progress-display terminal that records operations instead of performing them.
#[derive(Debug)]
struct RecordingTerm {
    activity: Arc<Mutex<TerminalActivity>>,
}

impl RecordingTerm {
    /// Borrows the shared recording for the duration of one recorded operation.
    fn counters(&self) -> MutexGuard<'_, TerminalActivity> {
        self.activity
            .lock()
            .expect("terminal activity should be available")
    }
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

    fn write_line(&self, s: &str) -> io::Result<()> {
        self.counters().record_write(s);
        Ok(())
    }

    fn write_str(&self, s: &str) -> io::Result<()> {
        self.counters().record_write(s);
        Ok(())
    }

    fn clear_line(&self) -> io::Result<()> {
        self.counters().clear_lines += 1;
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }
}

/// Where the progress display of one run draws.
enum ProgressSink {
    /// The terminal, through the progress library's own buffered stderr target.
    Terminal,
    /// A recording terminal shared with the [`Capture`] handed back to the caller.
    Recording(Arc<Mutex<TerminalActivity>>),
}

/// Where one of the two direct text streams of a run goes.
enum TextSink {
    Stdout,
    Stderr,
    Captured(Arc<Mutex<String>>),
}

impl TextSink {
    /// Writes one line, terminated, to this stream.
    fn write_line(&self, line: &str) -> io::Result<()> {
        match self {
            // Both standard streams are written through their locks so a line
            // cannot interleave with another; the newline flushes the
            // line-buffered standard output.
            Self::Stdout => writeln!(io::stdout().lock(), "{line}"),
            Self::Stderr => writeln!(io::stderr().lock(), "{line}"),
            Self::Captured(captured) => {
                let mut captured = captured.lock().expect("captured text should be available");
                captured.push_str(line);
                captured.push('\n');
                Ok(())
            }
        }
    }
}

/// The three coupled sinks one Extraction run presentation writes to.
///
/// A destination is consumed by the run that renders into it. Its interface is
/// deliberately just the two constructors: everything presentation needs from it
/// stays crate-visible so that neither the progress library nor [`io::Write`]
/// appears in what this library publishes.
pub struct TerminalOutput {
    progress: ProgressSink,
    stdout: TextSink,
    stderr: TextSink,
}

impl TerminalOutput {
    /// Builds the production destination: a real progress display and the real streams.
    pub fn stdio() -> Self {
        Self {
            progress: ProgressSink::Terminal,
            stdout: TextSink::Stdout,
            stderr: TextSink::Stderr,
        }
    }

    /// Builds a destination that records everything, with a handle to read it back.
    ///
    /// The capture shares the destination's storage, so it stays readable after the
    /// destination has been consumed by a run.
    pub fn captured() -> (Self, Capture) {
        let progress_activity = Arc::new(Mutex::new(TerminalActivity::default()));
        let stdout = Arc::new(Mutex::new(String::new()));
        let stderr = Arc::new(Mutex::new(String::new()));

        (
            Self {
                progress: ProgressSink::Recording(Arc::clone(&progress_activity)),
                stdout: TextSink::Captured(Arc::clone(&stdout)),
                stderr: TextSink::Captured(Arc::clone(&stderr)),
            },
            Capture {
                progress_activity,
                stdout,
                stderr,
            },
        )
    }

    /// Returns the draw target every progress display of this run must be created with.
    ///
    /// `ProgressDrawTarget::stderr()` is exactly what the progress library picks by
    /// itself, so naming it costs the production path nothing and keeps both
    /// destinations on one code path.
    fn progress_draw_target(&self) -> ProgressDrawTarget {
        match &self.progress {
            ProgressSink::Terminal => ProgressDrawTarget::stderr(),
            ProgressSink::Recording(activity) => {
                ProgressDrawTarget::term_like(Box::new(RecordingTerm {
                    activity: Arc::clone(activity),
                }))
            }
        }
    }

    /// Writes one line to standard output.
    fn print(&self, line: &str) {
        // Writing to a destination returns a failure where the print macros panic.
        // The realistic failure is a closed pipe -- output piped into a command
        // that exits first -- and presentation has nothing useful left to say once
        // its destination is gone, so the result is dropped rather than escalated.
        let _ = self.stdout.write_line(line);
    }

    /// Writes one line to standard error.
    fn print_error(&self, line: &str) {
        // Dropped for the same reason as in `print`.
        let _ = self.stderr.write_line(line);
    }
}

/// Everything a captured [`TerminalOutput`] recorded during one run.
pub struct Capture {
    progress_activity: Arc<Mutex<TerminalActivity>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
}

impl Capture {
    /// Returns everything written to standard output so far.
    pub fn stdout(&self) -> String {
        self.stdout
            .lock()
            .expect("captured text should be available")
            .clone()
    }

    /// Returns everything written to standard error so far.
    pub fn stderr(&self) -> String {
        self.stderr
            .lock()
            .expect("captured text should be available")
            .clone()
    }

    /// Returns how many drawn lines the progress display has cleared so far.
    pub fn clear_lines(&self) -> usize {
        self.progress_activity
            .lock()
            .expect("terminal activity should be available")
            .clear_lines
    }

    /// Returns how many times the progress display has written to the terminal.
    pub fn writes(&self) -> usize {
        self.progress_activity
            .lock()
            .expect("terminal activity should be available")
            .writes
    }

    /// Returns every line the progress display has drawn so far, each terminated.
    ///
    /// This is the readback for the terminal summaries that end an output-bearing
    /// run: they are drawn onto the extraction display rather than printed, so
    /// [`Capture::stdout`] and [`Capture::stderr`] stay empty for them.
    ///
    /// A live progress display redraws the same line on every update, so a phase
    /// contributes one entry per redraw and an assertion wants to match a summary
    /// within the text rather than compare the whole of it. The progress library
    /// renders through its own styling, so entries may carry terminal escapes
    /// around the parts a style colours.
    pub fn progress_text(&self) -> String {
        self.progress_activity
            .lock()
            .expect("terminal activity should be available")
            .rendered
            .clone()
    }
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
///
/// The criteria arrive as observation facts rather than as the Document
/// selection filter that holds them, so the two parts are passed separately.
/// Parameter order matches both the match below and the observation's field
/// order, because two `Option<&str>` arguments would otherwise swap silently.
fn epub_filter_description(title: Option<&str>, author: Option<&str>) -> String {
    match (title, author) {
        (Some(title), Some(author)) => format!("author '{}' and title '{}'", author, title),
        (None, Some(author)) => format!("author '{}'", author),
        (Some(title), None) => format!("title '{}'", title),
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

/// Renders cohesive live Extraction run observations into one terminal destination.
///
/// Crate-visible on purpose: the library publishes [`run_cli`] and the destination
/// it renders into, not the renderer. Only in-crate tests construct one directly.
pub(crate) struct ExtractionRunPresentation {
    output: TerminalOutput,
    discovery_pb: Option<ProgressBar>,
    epub_filter_pb: Option<ProgressBar>,
    epub_dedup_pb: Option<ProgressBar>,
    extraction_pb: Option<ProgressBar>,
}

impl ExtractionRunPresentation {
    /// Creates a presentation with no active progress bars over one destination.
    pub(crate) fn new(output: TerminalOutput) -> Self {
        Self {
            output,
            discovery_pb: None,
            epub_filter_pb: None,
            epub_dedup_pb: None,
            extraction_pb: None,
        }
    }

    /// Renders the ordered facts intake produced before the run started.
    ///
    /// The defaulted-input notice is normal output and the ignored-format notice is
    /// a warning, which is why the two land on different streams. No progress
    /// display exists yet, so neither write needs suspending.
    pub(crate) fn render_pre_run_notices(&mut self, notices: Vec<PreRunNotice>) {
        for notice in notices {
            match notice {
                PreRunNotice::DefaultedInput { path } => {
                    self.output.print(&format!(
                        "No input path specified, using current directory: {}",
                        path.display()
                    ));
                }
                PreRunNotice::IgnoredFormat { format } => {
                    self.output.print_error(&format!(
                        "Warning: Unrecognized format '{}' ignored",
                        format
                    ));
                }
            }
        }
    }

    /// Creates one progress bar already drawing to this run's destination.
    ///
    /// The draw target belongs to construction rather than to a later setter: the
    /// first message is what draws a bar for the first time, so a bar built with the
    /// default target would send that draw to the terminal even when capturing.
    fn new_progress_bar(&self, length: Option<u64>, style: ProgressStyle) -> ProgressBar {
        let progress = ProgressBar::with_draw_target(length, self.output.progress_draw_target());
        progress.set_style(style);
        progress
    }

    /// Finishes the extraction progress bar with the final run summary.
    fn finish_extraction(&self, message: String) {
        if let Some(pb) = &self.extraction_pb {
            pb.finish_with_message(message);
        }
    }

    /// Writes one line to standard error with the supplied progress bar suspended.
    ///
    /// Suspension is the whole reason the sinks are bundled: the bar clears the
    /// lines it drew, the line is written, and the bar redraws, so a redraw cannot
    /// land on top of the message.
    fn print_error_suspended(&self, progress: Option<&ProgressBar>, line: &str) {
        match progress {
            Some(pb) => pb.suspend(|| self.output.print_error(line)),
            None => self.output.print_error(line),
        }
    }

    /// Creates or advances the Document discovery spinner from one running observation.
    ///
    /// Rendered without relying on callback deltas: every running observation
    /// carries the phase's full current state, so nothing is accumulated here.
    /// The spinner is raised only for recursive traversal, which is the case slow
    /// enough to be worth showing. Whether this is the phase's first running
    /// observation is read from `is_none` rather than carried by the observation.
    fn render_discovering_documents(&mut self, scope: DocumentDiscoveryScope, discovered: usize) {
        if scope == DocumentDiscoveryScope::RecursiveDirectories && self.discovery_pb.is_none() {
            let pb = self.new_progress_bar(None, create_spinner_style());
            pb.set_message("Scanning directories for documents...");
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            self.discovery_pb = Some(pb);
        }
        if discovered > 0
            && let Some(pb) = &self.discovery_pb
        {
            pb.set_message(format!("Found {} document(s)...", discovered));
        }
    }

    /// Creates or advances the EPUB filtering bar from one running observation.
    ///
    /// Rendered without relying on callback deltas: `checked` is the phase's full
    /// current count, so the bar is positioned absolutely rather than incremented.
    /// The filter caption is set once, when the bar is created.
    fn render_filtering_epubs(
        &mut self,
        title: Option<String>,
        author: Option<String>,
        checked: usize,
        total: usize,
    ) {
        if self.epub_filter_pb.is_none() {
            let pb = self.new_progress_bar(Some(total as u64), create_progress_style());
            pb.set_message(format!(
                "Filtering EPUBs by {}",
                epub_filter_description(title.as_deref(), author.as_deref())
            ));
            self.epub_filter_pb = Some(pb);
        }
        if let Some(pb) = &self.epub_filter_pb {
            pb.set_position(checked as u64);
        }
    }

    /// Creates or advances the EPUB deduplication bar from one running observation.
    ///
    /// Rendered without relying on callback deltas: `checked` is the phase's full
    /// current count, so the bar is positioned absolutely rather than incremented.
    fn render_deduplicating_epubs(&mut self, checked: usize, total: usize) {
        if self.epub_dedup_pb.is_none() {
            let pb = self.new_progress_bar(Some(total as u64), create_progress_style());
            pb.set_message("Deduplicating EPUBs by metadata");
            self.epub_dedup_pb = Some(pb);
        }
        if let Some(pb) = &self.epub_dedup_pb {
            pb.set_position(checked as u64);
        }
    }
}

impl ExtractionRunObserver for ExtractionRunPresentation {
    /// Renders one observation from the single run-wide vocabulary.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        match observation {
            ExtractionRunObservation::DiscoveringDocuments { scope, discovered } => {
                self.render_discovering_documents(scope, discovered);
            }
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered, .. } => {
                if let Some(pb) = self.discovery_pb.take() {
                    pb.finish_with_message(format!("Found {} document(s)", discovered));
                }
            }
            ExtractionRunObservation::FilteringEpubs {
                title,
                author,
                checked,
                total,
                ..
            } => {
                self.render_filtering_epubs(title, author, checked, total);
            }
            ExtractionRunObservation::EpubFilteringFinished {
                checked, matching, ..
            } => {
                if let Some(pb) = self.epub_filter_pb.take() {
                    pb.set_position(checked as u64);
                    pb.finish_with_message(format!("Found {} matching EPUB(s)", matching));
                }
            }
            ExtractionRunObservation::DeduplicatingEpubs { checked, total, .. } => {
                self.render_deduplicating_epubs(checked, total);
            }
            ExtractionRunObservation::EpubDeduplicationFinished {
                checked,
                duplicates_found,
                unique_remaining,
                ..
            } => {
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
            // The three arms below render structured Document selection
            // diagnostics with terminal wording. Each suspends the progress bar
            // belonging to the phase that produced it, because a diagnostic can
            // arrive while that bar is live and the next redraw would otherwise
            // corrupt or overwrite the line.
            ExtractionRunObservation::MissingInput { path } => {
                self.output.print_error(&format!(
                    "Warning: Input path does not exist: {}",
                    path.display()
                ));
            }
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail } => {
                // Recursive discovery can warn while its spinner is active; suspending
                // prevents the next redraw from corrupting or overwriting the warning.
                self.print_error_suspended(
                    self.discovery_pb.as_ref(),
                    &format!(
                        "Warning: Could not inspect {} during document discovery: {}",
                        path.display(),
                        detail
                    ),
                );
            }
            ExtractionRunObservation::UnreadableEpubMetadata {
                path,
                purpose,
                detail,
            } => match purpose {
                EpubMetadataPurpose::Filtering => {
                    self.print_error_suspended(
                        self.epub_filter_pb.as_ref(),
                        &format!("Warning: Could not read {}: {}", path.display(), detail),
                    );
                }
                EpubMetadataPurpose::Deduplication => {
                    self.print_error_suspended(
                        self.epub_dedup_pb.as_ref(),
                        &format!(
                            "Warning: Could not read EPUB metadata from {} during deduplication; using filename fallback: {}",
                            path.display(),
                            detail
                        ),
                    );
                }
            },
            ExtractionRunObservation::ExtractionStarted { total, cover_only } => {
                let pb = self.new_progress_bar(Some(total as u64), create_progress_style());
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
                self.print_error_suspended(
                    self.extraction_pb.as_ref(),
                    &format!("Error processing {}: {}", path.display(), message),
                );
            }
            ExtractionRunObservation::DocumentWarning { warning, .. } => {
                // The document path stays run context only; presentation adds the
                // prefix and nothing else to the Document extraction-owned body.
                self.print_error_suspended(
                    self.extraction_pb.as_ref(),
                    &document_warning_line(&warning),
                );
            }
            ExtractionRunObservation::DocumentFinished { .. } => {
                if let Some(pb) = &self.extraction_pb {
                    pb.inc(1);
                }
            }
            ExtractionRunObservation::Terminal(outcome) => {
                if matches!(outcome, ExtractionRunOutcome::NoDocuments) {
                    self.output.print(&final_summary_message(&outcome));
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

#[cfg(test)]
mod tests;
