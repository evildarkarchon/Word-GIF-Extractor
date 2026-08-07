use super::*;

use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::Parser;

use crate::extraction_run::run as execute_extraction_run;
use crate::extraction_run_intake::prepare as prepare_extraction_run;
use crate::extraction_run_observation::{ConversionFacts, GifRoutingFacts};
use crate::test_support::{temp_test_dir, write_docx};

/// What a captured progress display did while single lines were written through it.
///
/// Suspension is visible as a clear followed by a redraw, so both delegating
/// observers below measure it the same way: snapshot the capture, delegate one
/// observation, then add the difference to a running total.
///
/// The totals are lower bounds rather than exact counts. A live spinner keeps a
/// steady tick running, and a tick landing inside a measured window adds to both
/// counters; the production spinner is not asked to stop ticking just because a
/// test is watching, because that would make presentation behave differently when
/// captured. The window is microseconds against a hundred-millisecond tick, and
/// removing the suspend still drops both totals to zero.
#[derive(Default)]
struct SuspensionActivity {
    clear_lines: usize,
    writes: usize,
}

impl SuspensionActivity {
    /// Reads the counters to hand back to [`SuspensionActivity::accumulate`].
    fn snapshot(capture: &Capture) -> (usize, usize) {
        (capture.clear_lines(), capture.writes())
    }

    /// Adds everything the capture recorded since `before` to the running totals.
    fn accumulate(&mut self, capture: &Capture, before: (usize, usize)) {
        self.clear_lines += capture.clear_lines() - before.0;
        self.writes += capture.writes() - before.1;
    }
}

/// Delegating presentation that induces one real post-classification traversal failure.
///
/// The capture is the same one the presentation writes into, so the wrapper can
/// measure what suspension did around a single diagnostic without touching the
/// progress display the presentation owns.
struct FilesystemPresentationObserver {
    inner: ExtractionRunPresentation,
    capture: Capture,
    remove_on_scan_start: Option<PathBuf>,
    discovery_diagnostics: usize,
    diagnostic_suspension: SuspensionActivity,
}

impl ExtractionRunObserver for FilesystemPresentationObserver {
    /// Delegates observations while deleting the classified root mid-scan.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        let starts_recursive_scan = matches!(
            &observation,
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0,
            }
        );
        let is_discovery_diagnostic = matches!(
            &observation,
            ExtractionRunObservation::DocumentDiscoveryFailed { .. }
        );
        // Snapshot before delegation so only the diagnostic's suspend operation
        // contributes to the clear/redraw deltas measured below.
        let before = is_discovery_diagnostic.then(|| SuspensionActivity::snapshot(&self.capture));

        self.inner.on_observation(observation);

        if starts_recursive_scan {
            // Delegation created and drew the spinner. Removing the classified root
            // now makes traversal fail while that spinner is live.
            if let Some(directory) = self.remove_on_scan_start.take() {
                fs::remove_dir(directory)
                    .expect("classified directory should be removable before traversal");
            }
        }

        if let Some(before) = before {
            self.discovery_diagnostics += 1;
            self.diagnostic_suspension.accumulate(&self.capture, before);
        }
    }
}

/// Delegating presentation that measures suspension around each warning.
///
/// It captures the opaque warning values the run transported so presentation
/// can be asserted without the terminal test owning any stable wording.
struct WarningPresentationObserver {
    inner: ExtractionRunPresentation,
    capture: Capture,
    warnings: Vec<DocumentExtractionWarning>,
    warning_suspension: SuspensionActivity,
}

impl ExtractionRunObserver for WarningPresentationObserver {
    /// Delegates observations while recording warning values and suspension effects.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        let warning = match &observation {
            ExtractionRunObservation::DocumentWarning { warning, .. } => Some(warning.clone()),
            _ => None,
        };
        // Snapshot before delegation so only the warning's suspend operation
        // contributes to the clear/redraw deltas measured below.
        let before = warning
            .as_ref()
            .map(|_| SuspensionActivity::snapshot(&self.capture));

        self.inner.on_observation(observation);

        if let (Some(warning), Some(before)) = (warning, before) {
            self.warnings.push(warning);
            self.warning_suspension.accumulate(&self.capture, before);
        }
    }
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
        NonZeroUsize::new(documents_with_output).expect("documents with output must be positive"),
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

/// Verifies one non-empty terminal outcome is rendered onto the progress display.
///
/// The summary each outcome produces is asserted word for word against
/// [`final_summary_message`] in the tests above. What those cannot see is where the
/// summary goes: every non-empty outcome ends the run by drawing on the extraction
/// display, and reaches neither text stream. That is the whole difference between
/// these outcomes and the no-documents one, and it is visible through the capture
/// without the test holding the display the presentation owns.
fn assert_terminal_observation_draws_the_summary(outcome: ExtractionRunOutcome) {
    let summary = final_summary_message(&outcome);
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);
    let cover_only = match &outcome {
        ExtractionRunOutcome::NoDocuments => false,
        ExtractionRunOutcome::NoOutput(output_kind) => *output_kind == ExtractionOutputKind::Covers,
        ExtractionRunOutcome::ProducedOutput(output) => {
            output.output_kind() == ExtractionOutputKind::Covers
        }
    };
    presentation.on_observation(ExtractionRunObservation::ExtractionStarted {
        total: 1,
        cover_only,
    });
    let drawn_before = capture.writes();

    presentation.on_observation(ExtractionRunObservation::Terminal(outcome));

    assert!(
        capture.writes() > drawn_before,
        "the terminal outcome should be drawn on the extraction display"
    );
    assert!(
        capture.progress_text().contains(&summary),
        "the drawn summary should be readable back; wanted {:?} within {:?}",
        summary,
        capture.progress_text()
    );
    assert_eq!(
        capture.stdout(),
        "",
        "a non-empty outcome must not reach standard output"
    );
    assert_eq!(
        capture.stderr(),
        "",
        "a non-empty outcome must not reach standard error"
    );
}

#[test]
fn terminal_epub_filter_description_preserves_existing_wording() {
    assert_eq!(
        epub_filter_description(Some("Magic Book"), Some("Test Author")),
        "author 'Test Author' and title 'Magic Book'"
    );
}

#[test]
fn test_quality_with_png_error() {
    let args = Args::try_parse_from(["test", "--convert", "png", "--quality", "90"]).unwrap();
    let err_msg = prepare_extraction_run(args)
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
fn test_lossless_with_jpg_error() {
    let args = Args::try_parse_from(["test", "--convert", "jpg", "--lossless"]).unwrap();
    let err_msg = prepare_extraction_run(args)
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
    let err_msg = prepare_extraction_run(args)
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
    let gif_dir = PathBuf::from("/tmp/gifs");
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

/// Verifies a real recursive failure suspends the scan spinner and then releases it.
#[test]
fn recursive_discovery_diagnostic_suspends_active_scan_spinner() {
    let temp_dir = temp_test_dir("presentation", "recursive-suspension");
    let requested_directory = temp_dir.join("requested");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    let input = requested_directory.to_string_lossy().into_owned();
    let args = Args::try_parse_from(["test", input.as_str(), "--recursive"])
        .expect("recursive arguments should parse");
    let prepared = prepare_extraction_run(args).expect("Extraction run intake should succeed");
    let (output, capture) = TerminalOutput::captured();
    let mut observer = FilesystemPresentationObserver {
        inner: ExtractionRunPresentation::new(output),
        capture,
        remove_on_scan_start: Some(requested_directory),
        discovery_diagnostics: 0,
        diagnostic_suspension: SuspensionActivity::default(),
    };

    let outcome = execute_extraction_run(prepared.request, &mut observer);

    assert_eq!(outcome, ExtractionRunOutcome::NoDocuments);
    assert_eq!(observer.discovery_diagnostics, 1);
    assert!(
        observer.diagnostic_suspension.clear_lines > 0,
        "suspension should clear the active spinner before rendering the warning"
    );
    assert!(
        observer.diagnostic_suspension.writes > 0,
        "suspension should redraw the active spinner after rendering the warning"
    );
    assert!(
        observer
            .capture
            .stderr()
            .contains("during document discovery"),
        "the diagnostic belongs on standard error: {}",
        observer.capture.stderr()
    );
    assert!(
        observer
            .capture
            .stdout()
            .contains("No documents found to process."),
        "the terminal summary belongs on standard output: {}",
        observer.capture.stdout()
    );

    // The finished scan phase must also have released its spinner. A diagnostic
    // arriving now therefore has no live display to suspend, so it clears and
    // redraws nothing -- which is what the released slot looks like from outside.
    let after_run = SuspensionActivity::snapshot(&observer.capture);
    let mut released = SuspensionActivity::default();
    observer
        .inner
        .on_observation(ExtractionRunObservation::DocumentDiscoveryFailed {
            path: PathBuf::from("late"),
            detail: "arrived after the scan phase finished".to_string(),
        });
    released.accumulate(&observer.capture, after_run);
    assert_eq!(
        (released.clear_lines, released.writes),
        (0, 0),
        "the finished scan spinner should have been released, leaving nothing to suspend"
    );

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

/// Verifies warning presentation adds one prefix and suspends the extraction bar.
///
/// The stable body stays owned by Document extraction, so the assertions
/// compare against the transported value rather than restating any wording.
#[test]
fn document_warning_presentation_adds_one_prefix_and_suspends_extraction_progress() {
    let temp_dir = temp_test_dir("presentation", "warning-presentation");
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
    let prepared = prepare_extraction_run(args).expect("Extraction run intake should succeed");
    let (output, capture) = TerminalOutput::captured();
    let mut observer = WarningPresentationObserver {
        inner: ExtractionRunPresentation::new(output),
        capture,
        warnings: Vec::new(),
        warning_suspension: SuspensionActivity::default(),
    };

    execute_extraction_run(prepared.request, &mut observer);

    assert_eq!(observer.warnings.len(), 1);
    assert!(
        observer.warning_suspension.clear_lines > 0,
        "suspension should clear the active extraction bar before rendering the warning"
    );
    assert!(
        observer.warning_suspension.writes > 0,
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
    // The same line is what the run actually wrote, and it went to standard error.
    assert!(
        observer.capture.stderr().contains(&rendered),
        "the warning belongs on standard error: {}",
        observer.capture.stderr()
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

/// Verifies every outcome shape ends on the progress display rather than a stream.
#[test]
fn terminal_observer_draws_every_nonempty_outcome_on_the_extraction_display() {
    let gif_dir = PathBuf::from("/tmp/gifs");
    let outcomes = [
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images),
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Covers),
        produced_outcome(ExtractionOutputKind::Images, 3, 2, None, None),
        produced_outcome(
            ExtractionOutputKind::Images,
            3,
            2,
            Some(ConversionFacts::new(2, 1)),
            None,
        ),
        produced_outcome(
            ExtractionOutputKind::Images,
            3,
            2,
            None,
            Some((1, gif_dir.clone())),
        ),
        produced_outcome(
            ExtractionOutputKind::Images,
            4,
            2,
            Some(ConversionFacts::new(2, 1)),
            Some((1, gif_dir.clone())),
        ),
        produced_outcome(ExtractionOutputKind::Covers, 1, 1, None, None),
        produced_outcome(ExtractionOutputKind::Images, 2, 1, None, None),
    ];

    for outcome in outcomes {
        assert_terminal_observation_draws_the_summary(outcome);
    }
}

/// Verifies the capture reads back progress text other than the final summary.
///
/// The summary assertion in
/// [`assert_terminal_observation_draws_the_summary`] covers the last thing a run
/// draws. This covers what it drew on the way there: a phase caption and the
/// per-document message that replaces it both belong to the progress display and
/// reach neither text stream, so the capture is the only place they are readable.
#[test]
fn captured_progress_text_retains_the_captions_drawn_before_the_summary() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    presentation.on_observation(ExtractionRunObservation::ExtractionStarted {
        total: 2,
        cover_only: false,
    });
    presentation.on_observation(ExtractionRunObservation::DocumentStarted {
        path: PathBuf::from("/tmp/documents/chapter.docx"),
        display_name: "chapter.docx".to_string(),
    });

    let drawn = capture.progress_text();
    assert!(
        drawn.contains("Extracting images from documents"),
        "the extraction caption should be readable back; got {:?}",
        drawn
    );
    assert!(
        drawn.contains("chapter.docx"),
        "the per-document message should be readable back; got {:?}",
        drawn
    );
    assert_eq!(capture.stdout(), "");
    assert_eq!(capture.stderr(), "");
}

/// Verifies the one outcome that has no display to finish is printed instead.
///
/// No documents means no extraction ever started, so there is no progress display
/// to carry the summary. This is the contrast case for
/// [`terminal_observer_draws_every_nonempty_outcome_on_the_extraction_display`]:
/// the same observation lands on standard output and draws nothing.
#[test]
fn no_documents_terminal_outcome_is_printed_rather_than_drawn() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    presentation.on_observation(ExtractionRunObservation::Terminal(
        ExtractionRunOutcome::NoDocuments,
    ));

    assert_eq!(capture.stdout(), "No documents found to process.\n");
    assert_eq!(capture.stderr(), "");
    assert_eq!(capture.writes(), 0);
    assert_eq!(capture.clear_lines(), 0);
    assert_eq!(capture.progress_text(), "");
}

/// Verifies the two pre-run notices keep their existing wording and streams.
#[test]
fn pre_run_notices_keep_their_wording_and_streams() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    presentation.render_pre_run_notices(vec![
        PreRunNotice::DefaultedInput {
            path: PathBuf::from("/tmp/documents"),
        },
        PreRunNotice::IgnoredFormat {
            format: "tiffff".to_string(),
        },
    ]);

    assert_eq!(
        capture.stdout(),
        format!(
            "No input path specified, using current directory: {}\n",
            PathBuf::from("/tmp/documents").display()
        )
    );
    assert_eq!(
        capture.stderr(),
        "Warning: Unrecognized format 'tiffff' ignored\n"
    );
}

/// Verifies the published entry point renders a whole run into a supplied destination.
#[test]
fn run_cli_renders_a_complete_run_into_the_supplied_destination() {
    let temp_dir = temp_test_dir("presentation", "run-cli-entry-point");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let args = Args::try_parse_from(["test", temp_dir.to_string_lossy().as_ref()])
        .expect("entry point arguments should parse");
    let (output, capture) = TerminalOutput::captured();

    run_cli(args, output).expect("an empty input directory is not a failure");

    assert_eq!(capture.stdout(), "No documents found to process.\n");
    assert_eq!(capture.stderr(), "");

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

/// Verifies intake failures are returned rather than written to the destination.
#[test]
fn run_cli_returns_intake_failures_without_writing_to_the_destination() {
    let args = Args::try_parse_from(["test", "--convert", "png", "--quality", "90"])
        .expect("conflicting conversion arguments still parse");
    let (output, capture) = TerminalOutput::captured();

    let error = run_cli(args, output).expect_err("PNG quality should fail semantic intake");

    assert!(
        error
            .to_string()
            .contains("--quality cannot be used with --convert png"),
        "Error was: {}",
        error
    );
    assert_eq!(capture.stdout(), "");
    assert_eq!(capture.stderr(), "");
}
