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
        crate::extraction_run_intake::prepare(args, std::env::current_dir)
            .map_err(render_intake_error)?;

    let mut presentation = ExtractionRunPresentation::new(output);
    presentation.render_pre_run_notices(notices);
    crate::extraction_run::run(request, &mut presentation);

    Ok(())
}

/// One thing a captured run did to its terminal, in the order it did it.
///
/// Clearing and redrawing is what suspension does, and its whole point is where a
/// direct write lands relative to them: after the display cleared, before it drew
/// again. Recording all three sinks into one ordered sequence makes that a
/// property of the sequence. Counting clears and draws separately could only give
/// lower bounds, because a steady tick anywhere in a measured window adds to both.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptEntry {
    /// The progress display cleared one line it had drawn.
    Cleared,
    /// The progress display drew one line.
    ///
    /// The progress library makes one call per drawn line and leaves the last
    /// line of a draw unterminated, to keep the terminal cursor on it. Both calls
    /// land here as one entry each, so the readback is one line per drawn line.
    Drawn(String),
    /// One line reached standard output.
    Stdout(String),
    /// One line reached standard error.
    Stderr(String),
}

impl TranscriptEntry {
    /// Returns whether the progress display, rather than a text stream, produced this entry.
    fn is_progress(&self) -> bool {
        matches!(self, Self::Cleared | Self::Drawn(_))
    }

    /// Returns the line this entry wrote to either text stream, if it wrote one.
    fn text_line(&self) -> Option<&str> {
        match self {
            Self::Stdout(line) | Self::Stderr(line) => Some(line),
            Self::Cleared | Self::Drawn(_) => None,
        }
    }
}

/// The ordered record shared by every sink of one captured destination.
///
/// One lock serializes all three sinks, so the order of entries is the order the
/// operations happened in, including writes made from inside a suspend.
type Transcript = Arc<Mutex<Vec<TranscriptEntry>>>;

/// Appends one entry to a shared transcript.
fn record(transcript: &Transcript, entry: TranscriptEntry) {
    transcript
        .lock()
        .expect("captured transcript should be available")
        .push(entry);
}

/// Progress-display terminal that records operations instead of performing them.
#[derive(Debug)]
struct RecordingTerm {
    transcript: Transcript,
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
        record(&self.transcript, TranscriptEntry::Drawn(s.to_string()));
        Ok(())
    }

    fn write_str(&self, s: &str) -> io::Result<()> {
        record(&self.transcript, TranscriptEntry::Drawn(s.to_string()));
        Ok(())
    }

    fn clear_line(&self) -> io::Result<()> {
        record(&self.transcript, TranscriptEntry::Cleared);
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
    Recording(Transcript),
}

/// Where one of the two direct text streams of a run goes.
enum TextSink {
    Stdout,
    Stderr,
    /// The shared transcript, with the entry constructor naming which stream this is.
    Captured(Transcript, fn(String) -> TranscriptEntry),
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
            Self::Captured(transcript, stream) => {
                record(transcript, stream(line.to_string()));
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
        let transcript = Transcript::default();

        (
            Self {
                progress: ProgressSink::Recording(Arc::clone(&transcript)),
                stdout: TextSink::Captured(Arc::clone(&transcript), TranscriptEntry::Stdout),
                stderr: TextSink::Captured(Arc::clone(&transcript), TranscriptEntry::Stderr),
            },
            Capture { transcript },
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
            ProgressSink::Recording(transcript) => {
                ProgressDrawTarget::term_like(Box::new(RecordingTerm {
                    transcript: Arc::clone(transcript),
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
///
/// All three sinks record into one ordered transcript. The readers below either
/// project one sink out of it or, for suspension, ask a question about its order.
pub struct Capture {
    transcript: Transcript,
}

impl Capture {
    /// Borrows the ordered transcript for the duration of one read.
    fn entries(&self) -> MutexGuard<'_, Vec<TranscriptEntry>> {
        self.transcript
            .lock()
            .expect("captured transcript should be available")
    }

    /// Joins the selected entries' lines, each terminated, in transcript order.
    fn lines(&self, select: impl Fn(&TranscriptEntry) -> Option<&str>) -> String {
        self.entries()
            .iter()
            .filter_map(select)
            .flat_map(|line| [line, "\n"])
            .collect()
    }

    /// Returns everything written to standard output so far.
    pub fn stdout(&self) -> String {
        self.lines(|entry| match entry {
            TranscriptEntry::Stdout(line) => Some(line),
            _ => None,
        })
    }

    /// Returns everything written to standard error so far.
    pub fn stderr(&self) -> String {
        self.lines(|entry| match entry {
            TranscriptEntry::Stderr(line) => Some(line),
            _ => None,
        })
    }

    /// Returns whether every write of `line` happened with the progress display suspended.
    ///
    /// Suspended means the display cleared before the write and drew again after it:
    /// the nearest progress-display entry before each occurrence is a clear, and the
    /// nearest one after it is a draw. Writes to either text stream count as
    /// occurrences and are skipped when looking for those neighbours. `line` is
    /// compared whole and without its terminator. A line that was never written
    /// returns `false`, so a mistyped expectation cannot pass by matching nothing.
    ///
    /// The answer is exact rather than a lower bound: the progress library holds the
    /// display's lock for the whole of a suspend, so a steady tick cannot draw
    /// between the clear and the write it protects.
    pub fn suspended_around(&self, line: &str) -> bool {
        let entries = self.entries();
        let mut occurrences = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.text_line() == Some(line))
            .map(|(index, _)| index)
            .peekable();
        if occurrences.peek().is_none() {
            return false;
        }

        occurrences.all(|index| {
            let cleared_before = matches!(
                entries[..index]
                    .iter()
                    .rev()
                    .find(|entry| entry.is_progress()),
                Some(TranscriptEntry::Cleared)
            );
            let drawn_after = matches!(
                entries[index + 1..]
                    .iter()
                    .find(|entry| entry.is_progress()),
                Some(TranscriptEntry::Drawn(_))
            );
            cleared_before && drawn_after
        })
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
        self.lines(|entry| match entry {
            TranscriptEntry::Drawn(line) => Some(line),
            _ => None,
        })
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
        // The flag spellings below are checked against `Args` by a guard test, so a
        // renamed flag fails that test instead of leaving this wording stale.
        //
        // From the command line, `clap` rejects an out-of-range quality and the
        // quality/lossless conflict before intake runs, so only the other two arms
        // are reached there. All four stay: the Conversion policy validates
        // independently of `clap`, and an `Args` built directly reaches every arm.
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

/// The phase whose observations drew the progress display that is currently live.
///
/// The tag records who drew the display, not where that phase falls in the run:
/// presentation never learns the phase order, it only refuses to let one phase's
/// observations advance or finish a display another phase drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayPhase {
    Discovery,
    Filtering,
    Deduplication,
    Extraction,
}

/// The one progress display a run shows at a time, with the phase that drew it.
struct LiveDisplay {
    phase: DisplayPhase,
    bar: ProgressBar,
}

/// Renders cohesive live Extraction run observations into one terminal destination.
///
/// Crate-visible on purpose: the library publishes [`run_cli`] and the destination
/// it renders into, not the renderer. Only in-crate tests construct one directly.
///
/// Phases never overlap, so a single slot holds whichever progress display is
/// live. Every direct write to standard error suspends that display, whichever
/// phase drew it, and the terminal summary finishes it or is printed when none is
/// live.
pub(crate) struct ExtractionRunPresentation {
    output: TerminalOutput,
    live: Option<LiveDisplay>,
}

impl ExtractionRunPresentation {
    /// Creates a presentation with no active progress bars over one destination.
    pub(crate) fn new(output: TerminalOutput) -> Self {
        Self { output, live: None }
    }

    /// Returns the live display's bar when `phase` drew it.
    fn live_bar(&self, phase: DisplayPhase) -> Option<&ProgressBar> {
        self.live
            .as_ref()
            .filter(|live| live.phase == phase)
            .map(|live| &live.bar)
    }

    /// Releases and returns the live display's bar when `phase` drew it.
    ///
    /// A display drawn by another phase stays live, so a finished observation
    /// that arrives out of turn cannot end a display it does not own.
    fn take_live(&mut self, phase: DisplayPhase) -> Option<ProgressBar> {
        if self.live_bar(phase).is_some() {
            self.live.take().map(|live| live.bar)
        } else {
            None
        }
    }

    /// Makes `bar` the live display, drawn by `phase`.
    fn raise(&mut self, phase: DisplayPhase, bar: ProgressBar) {
        self.live = Some(LiveDisplay { phase, bar });
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

    /// Ends the run with its summary: finishes the live display, or prints when none is live.
    ///
    /// The routing follows display state rather than the outcome, so a summary can
    /// never be dropped: no documents leaves nothing live and is printed, and any
    /// other outcome finishes the extraction display the run raised before it.
    fn finish_run(&mut self, summary: String) {
        match self.live.take() {
            Some(live) => live.bar.finish_with_message(summary),
            None => self.output.print(&summary),
        }
    }

    /// Writes one line to standard error with the live progress display suspended.
    ///
    /// Suspension is the whole reason the sinks are bundled: the bar clears the
    /// lines it drew, the line is written, and the bar redraws, so a redraw cannot
    /// land on top of the message. Whichever phase drew the live display, it is the
    /// one that would redraw, so it is the one suspended; with nothing live the
    /// line is written directly.
    fn print_error_suspended(&self, line: &str) {
        match &self.live {
            Some(live) => live.bar.suspend(|| self.output.print_error(line)),
            None => self.output.print_error(line),
        }
    }

    /// Creates or advances the Document discovery spinner from one running observation.
    ///
    /// Rendered without relying on callback deltas: every running observation
    /// carries the phase's full current state, so nothing is accumulated here.
    /// The spinner is raised only for recursive traversal, which is the case slow
    /// enough to be worth showing. Whether this is the phase's first running
    /// observation is read from whether this phase's display is live rather than
    /// carried by the observation.
    fn render_discovering_documents(&mut self, scope: DocumentDiscoveryScope, discovered: usize) {
        if scope == DocumentDiscoveryScope::RecursiveDirectories
            && self.live_bar(DisplayPhase::Discovery).is_none()
        {
            let pb = self.new_progress_bar(None, create_spinner_style());
            pb.set_message("Scanning directories for documents...");
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            self.raise(DisplayPhase::Discovery, pb);
        }
        if discovered > 0
            && let Some(pb) = self.live_bar(DisplayPhase::Discovery)
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
        if self.live_bar(DisplayPhase::Filtering).is_none() {
            let pb = self.new_progress_bar(Some(total as u64), create_progress_style());
            pb.set_message(format!(
                "Filtering EPUBs by {}",
                epub_filter_description(title.as_deref(), author.as_deref())
            ));
            self.raise(DisplayPhase::Filtering, pb);
        }
        if let Some(pb) = self.live_bar(DisplayPhase::Filtering) {
            pb.set_position(checked as u64);
        }
    }

    /// Creates or advances the EPUB deduplication bar from one running observation.
    ///
    /// Rendered without relying on callback deltas: `checked` is the phase's full
    /// current count, so the bar is positioned absolutely rather than incremented.
    fn render_deduplicating_epubs(&mut self, checked: usize, total: usize) {
        if self.live_bar(DisplayPhase::Deduplication).is_none() {
            let pb = self.new_progress_bar(Some(total as u64), create_progress_style());
            pb.set_message("Deduplicating EPUBs by metadata");
            self.raise(DisplayPhase::Deduplication, pb);
        }
        if let Some(pb) = self.live_bar(DisplayPhase::Deduplication) {
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
                if let Some(pb) = self.take_live(DisplayPhase::Discovery) {
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
                if let Some(pb) = self.take_live(DisplayPhase::Filtering) {
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
                if let Some(pb) = self.take_live(DisplayPhase::Deduplication) {
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
            // The arms below render structured Document selection diagnostics
            // with terminal wording. Every one suspends whatever progress display
            // is live, because a diagnostic can arrive while one is drawing and
            // the next redraw would otherwise corrupt or overwrite the line. A
            // missing input is reported before any display exists, so for it the
            // suspend is a direct write.
            ExtractionRunObservation::MissingInput { path } => {
                self.print_error_suspended(&format!(
                    "Warning: Input path does not exist: {}",
                    path.display()
                ));
            }
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail } => {
                // Recursive discovery can warn while its spinner is active; suspending
                // prevents the next redraw from corrupting or overwriting the warning.
                self.print_error_suspended(&format!(
                    "Warning: Could not inspect {} during document discovery: {}",
                    path.display(),
                    detail
                ));
            }
            ExtractionRunObservation::UnreadableEpubMetadata {
                path,
                purpose,
                detail,
            } => match purpose {
                EpubMetadataPurpose::Filtering => {
                    self.print_error_suspended(&format!(
                        "Warning: Could not read {}: {}",
                        path.display(),
                        detail
                    ));
                }
                EpubMetadataPurpose::Deduplication => {
                    self.print_error_suspended(
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
                self.raise(DisplayPhase::Extraction, pb);
            }
            ExtractionRunObservation::DocumentStarted { display_name, .. } => {
                if let Some(pb) = self.live_bar(DisplayPhase::Extraction) {
                    pb.set_message(display_name);
                }
            }
            ExtractionRunObservation::DocumentError { path, message } => {
                self.print_error_suspended(&format!(
                    "Error processing {}: {}",
                    path.display(),
                    message
                ));
            }
            ExtractionRunObservation::DocumentWarning { warning, .. } => {
                // The document path stays run context only; presentation adds the
                // prefix and nothing else to the Document extraction-owned body.
                self.print_error_suspended(&document_warning_line(&warning));
            }
            ExtractionRunObservation::DocumentFinished { .. } => {
                if let Some(pb) = self.live_bar(DisplayPhase::Extraction) {
                    pb.inc(1);
                }
            }
            ExtractionRunObservation::Terminal(outcome) => {
                self.finish_run(final_summary_message(&outcome));
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
