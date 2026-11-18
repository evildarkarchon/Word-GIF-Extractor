use super::*;

use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::{CommandFactory, Parser};

use crate::conversion::ConversionTarget;
use crate::document_extraction::{ApplicableOutcomeFacts, DocumentExtractionFacts};
use crate::emitted_image_tally::{EmittedImageTally, TallyRole};
use crate::extraction_run_intake::prepare as prepare_extraction_run;
use crate::extraction_run_observation::{ConversionFacts, ExtractionRunOutcomeAccumulator};
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::ImageWriteWarning;
use crate::test_support::{MINIMAL_PNG, no_fallback_directory, temp_test_dir, write_docx};

/// Builds one produced outcome of the given shape through the outcome accumulator.
///
/// The accumulator is the only constructor of a produced outcome, so a test can
/// never hold one production could not produce (ADR-0017). The shape is turned
/// back into documents: one role per emitted image (converted, then
/// conversion-skipped, then routed GIFs, then preserved images for the rest) is
/// dealt round-robin across `documents_with_output` tallies, so each document
/// emits at least one image. A covers outcome records covers and finishes with
/// cover intent. Conversion and GIF-routing facts apply exactly when the caller
/// passes them, through the Applicable outcome facts the accumulator is seeded with.
///
/// Panics on a shape the fold cannot express: zero images, documents or routed
/// GIFs, more documents than images, or more classified images than emitted
/// ones. It does not police pipeline rules beyond that, such as covers never
/// being conversion-skipped; callers pass shapes a run produces.
fn produced_outcome(
    output_kind: ExtractionOutputKind,
    emitted_images: usize,
    documents_with_output: usize,
    conversion: Option<ConversionFacts>,
    gif_routing: Option<(usize, PathBuf)>,
) -> ExtractionRunOutcome {
    assert!(
        documents_with_output > 0,
        "documents with output must be positive"
    );
    assert!(
        documents_with_output <= emitted_images,
        "every document with output must emit at least one image"
    );
    let (converted, skipped) = conversion.map_or((0, 0), |facts| {
        (facts.converted_images(), facts.skipped_conversions())
    });
    let routed_gifs = gif_routing.as_ref().map_or(0, |(routed_gifs, _)| {
        assert!(*routed_gifs > 0, "routed GIF count must be positive");
        *routed_gifs
    });
    let classified = converted + skipped + routed_gifs;
    assert!(
        classified <= emitted_images,
        "classified images cannot exceed the emitted images"
    );

    let roles = std::iter::repeat_n(TallyRole::Converted, converted)
        .chain(std::iter::repeat_n(TallyRole::ConversionSkipped, skipped))
        .chain(std::iter::repeat_n(TallyRole::RoutedGif, routed_gifs))
        .chain(std::iter::repeat_n(
            TallyRole::Preserved,
            emitted_images - classified,
        ));
    let covers = output_kind == ExtractionOutputKind::Covers;
    let mut tallies = vec![EmittedImageTally::default(); documents_with_output];
    for (image, role) in roles.enumerate() {
        let tally = &mut tallies[image % documents_with_output];
        if covers {
            tally.record_cover(role);
        } else {
            tally.record_normal_image(role);
        }
    }

    let mut accumulator = ExtractionRunOutcomeAccumulator::new(ApplicableOutcomeFacts::fabricated(
        conversion.is_some(),
        gif_routing.map(|(_, destination)| destination),
    ));
    for tally in tallies {
        accumulator.fold(&DocumentExtractionFacts::fabricated(tally, Vec::new()));
    }
    let outcome = accumulator.finish(covers);
    assert!(
        matches!(outcome, ExtractionRunOutcome::ProducedOutput(_)),
        "a shape with emitted images should produce output, got {outcome:?}"
    );
    outcome
}

/// Verifies one non-empty terminal outcome clears the extraction display, then prints.
///
/// The tests above check each outcome's summary word for word against
/// [`final_summary_message`]. What they cannot see is where the summary goes. Every
/// non-empty outcome ends a run whose extraction display is live. The summary must
/// leave that display behind and land on standard output, the same stream the
/// no-documents summary uses. A summary drawn onto the display vanishes whenever
/// standard error is not a terminal (ADR-0026).
fn assert_terminal_observation_clears_then_prints_the_summary(outcome: ExtractionRunOutcome) {
    let summary = final_summary_message(&outcome);
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);
    let cover_only = match &outcome {
        ExtractionRunOutcome::NoDocuments => false,
        ExtractionRunOutcome::NoOutput { output_kind, .. } => {
            *output_kind == ExtractionOutputKind::Covers
        }
        ExtractionRunOutcome::ProducedOutput(output) => {
            output.output_kind() == ExtractionOutputKind::Covers
        }
    };
    presentation.on_observation(ExtractionRunObservation::ExtractionStarted {
        total: 1,
        cover_only,
    });

    presentation.on_observation(ExtractionRunObservation::Terminal(outcome));

    assert_eq!(capture.stdout(), format!("{summary}\n"));
    assert_eq!(
        capture.stderr(),
        "",
        "a summary is output, not a warning, so it must not reach standard error"
    );
    assert!(
        !capture.progress_text().contains(&summary),
        "the summary must not be drawn, or a hidden display would swallow it: {:?}",
        capture.progress_text()
    );
    // The summary is the last thing the run does, and the display cleared just
    // before it. A draw after it would be a bar left on screen beneath the result.
    let entries = capture.entries();
    let (last, earlier) = entries
        .split_last()
        .expect("the run should have recorded something");
    assert_eq!(*last, TranscriptEntry::Stdout(summary));
    assert_eq!(
        earlier.iter().rev().find(|entry| entry.is_progress()),
        Some(&TranscriptEntry::Cleared),
        "the extraction display should clear before the summary: {earlier:?}"
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
    let err_msg = prepare_extraction_run(args, no_fallback_directory)
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
    let err_msg = prepare_extraction_run(args, no_fallback_directory)
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
    let err_msg = prepare_extraction_run(args, no_fallback_directory)
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

/// Verifies every flag the intake error wording names is a flag `Args` defines.
///
/// The wording stays here because this module owns every word the tool says, which
/// leaves it far from the `clap` definitions it spells. A renamed field or `long`
/// override would leave it naming a flag that no longer exists, with nothing failing
/// to compile. This renders all four Conversion policy arms -- including the two
/// `clap` pre-empts on the command line -- and resolves every `--name` it finds,
/// plus the value that follows a flag with a closed set of values, against the
/// command definition itself.
#[test]
fn intake_error_wording_names_only_flags_args_defines() {
    let command = Args::command();
    let errors = [
        ConversionPolicyError::QualityOutOfRange { quality: 0 },
        ConversionPolicyError::QualityUnsupportedForPng,
        ConversionPolicyError::LosslessUnsupportedForTarget {
            target: ConversionTarget::Jpg,
        },
        ConversionPolicyError::LosslessConflictsWithQuality,
    ];

    for error in errors {
        let message =
            render_intake_error(ExtractionRunIntakeError::ConversionPolicy(error)).to_string();
        let words = message.split_whitespace().collect::<Vec<_>>();
        let mut flags_named = 0;

        for (index, word) in words.iter().enumerate() {
            let Some(long) = word.strip_prefix("--") else {
                continue;
            };
            flags_named += 1;
            let argument = command
                .get_arguments()
                .find(|argument| argument.get_long() == Some(long))
                .unwrap_or_else(|| {
                    panic!("{message:?} names --{long}, which Args does not define")
                });

            // A switch reports `true` and `false` as its possible values without
            // taking either from the command line, so only a flag that takes a
            // value has one to check in the wording.
            let possible_values = argument.get_possible_values();
            if argument.get_action().takes_values() && !possible_values.is_empty() {
                let value = words
                    .get(index + 1)
                    .unwrap_or_else(|| panic!("{message:?} names --{long} without its value"));
                assert!(
                    possible_values
                        .iter()
                        .any(|possible| possible.matches(value, false)),
                    "{message:?} gives --{long} the value {value:?}, which Args does not accept"
                );
            }
        }

        // A message that names no flag would pass the loop above vacuously.
        assert!(flags_named > 0, "{message:?} names no flag to check");
    }
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

/// Verifies a recursive discovery failure suspends the scan spinner and then releases it.
///
/// The observations are the ones Document selection emits when the only requested
/// directory fails mid-scan, followed by the run's terminal outcome. Feeding them
/// directly keeps the test about presentation; which failure produces that stream
/// is Document selection's to test.
#[test]
fn recursive_discovery_diagnostic_suspends_active_scan_spinner() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);
    let scope = DocumentDiscoveryScope::RecursiveDirectories;

    for observation in [
        ExtractionRunObservation::DiscoveringDocuments {
            scope,
            discovered: 0,
        },
        ExtractionRunObservation::DocumentDiscoveryFailed {
            path: PathBuf::from("requested"),
            detail: "directory vanished during traversal".to_string(),
        },
        ExtractionRunObservation::DiscoveringDocuments {
            scope,
            discovered: 0,
        },
        ExtractionRunObservation::DocumentDiscoveryFinished {
            scope,
            discovered: 0,
        },
        ExtractionRunObservation::Terminal(ExtractionRunOutcome::NoDocuments),
    ] {
        presentation.on_observation(observation);
    }

    let stderr = capture.stderr();
    let diagnostics = stderr
        .lines()
        .filter(|line| line.contains("during document discovery"))
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics.len(),
        1,
        "the diagnostic belongs on standard error, once: {stderr}"
    );
    assert!(
        capture.suspended_around(diagnostics[0]),
        "the active spinner should clear before the warning and redraw after it"
    );
    assert!(
        capture.stdout().contains("No documents found to process."),
        "the terminal summary belongs on standard output: {}",
        capture.stdout()
    );

    // The finished scan phase must also have released its spinner. A diagnostic
    // arriving now therefore has no live display to suspend, so nothing clears
    // before it or redraws after it -- which is what the released slot looks like
    // from outside. A finished bar still held would clear and redraw around it.
    let late = ExtractionRunObservation::DocumentDiscoveryFailed {
        path: PathBuf::from("late"),
        detail: "arrived after the scan phase finished".to_string(),
    };
    presentation.on_observation(late);
    let late_line = capture
        .stderr()
        .lines()
        .last()
        .expect("the late diagnostic should reach standard error")
        .to_string();
    assert!(late_line.contains("arrived after the scan phase finished"));
    assert!(
        !capture.suspended_around(&late_line),
        "the finished scan spinner should have been released, leaving nothing to suspend"
    );
}

/// Verifies a skipped duplicate names both EPUBs and suspends the deduplication bar.
///
/// The diagnostic arrives while the deduplication bar is drawing, ahead of the running
/// observation that counts it, so a write that skipped the suspend would be overdrawn.
#[test]
fn skipped_duplicate_names_both_epubs_and_suspends_the_deduplication_bar() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);
    let running = |checked, duplicates_found, unique_remaining| {
        ExtractionRunObservation::DeduplicatingEpubs {
            checked,
            total: 2,
            duplicates_found,
            unique_remaining,
        }
    };

    for observation in [
        running(0, 0, 0),
        running(1, 0, 1),
        ExtractionRunObservation::SkippedDuplicateEpub {
            path: PathBuf::from("downloads").join("copy.epub"),
            duplicate_of: PathBuf::from("library").join("book.epub"),
        },
        running(2, 1, 1),
        ExtractionRunObservation::EpubDeduplicationFinished {
            checked: 2,
            total: 2,
            duplicates_found: 1,
            unique_remaining: 1,
        },
    ] {
        presentation.on_observation(observation);
    }

    let expected = format!(
        "Warning: Skipped {}: it declares the same book as {} (use --keep-duplicates to extract both)",
        PathBuf::from("downloads").join("copy.epub").display(),
        PathBuf::from("library").join("book.epub").display()
    );
    assert_eq!(
        capture.stderr().lines().collect::<Vec<_>>(),
        [expected.as_str()]
    );
    assert!(
        capture.suspended_around(&expected),
        "the deduplication bar should clear before the warning and redraw after it"
    );
}

/// Verifies warning presentation adds one prefix and the document path, and
/// suspends the extraction bar.
///
/// The stable body stays owned by Document extraction, so the warning is
/// fabricated through its test entry point and the assertions compare against
/// that value rather than restating any wording. The observations are the ones
/// the run emits around one warned document.
#[test]
fn document_warning_presentation_adds_prefix_and_path_and_suspends_extraction_progress() {
    let document_path = PathBuf::from("documents").join("warned.docx");
    let warning = DocumentExtractionWarning::fabricated(ImageWriteWarning::ExtensionFallback {
        source_name: "word/media/only.png".to_string(),
        format: ImageFormat::Png,
    });
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    for observation in [
        ExtractionRunObservation::ExtractionStarted {
            total: 1,
            cover_only: false,
        },
        ExtractionRunObservation::DocumentStarted {
            path: document_path.clone(),
            display_name: "warned.docx".to_string(),
        },
        ExtractionRunObservation::DocumentWarning {
            path: document_path.clone(),
            warning: warning.clone(),
        },
        ExtractionRunObservation::DocumentFinished {
            path: document_path.clone(),
        },
        ExtractionRunObservation::Terminal(produced_outcome(
            ExtractionOutputKind::Images,
            1,
            1,
            None,
            None,
        )),
    ] {
        presentation.on_observation(observation);
    }

    // Stripping exactly one prefix, then the document path, must leave the
    // transported body untouched. That rules out a missing or doubled prefix and
    // a missing path without this test knowing what the body says.
    let rendered = document_warning_line(&document_path, &warning);
    let after_prefix = rendered
        .strip_prefix("Warning: ")
        .expect("the line should open with one warning prefix");
    assert_eq!(
        after_prefix.strip_prefix(&format!("{}: ", document_path.display())),
        Some(warning.get_message()),
        "the document path should sit between the prefix and the body: {rendered}"
    );
    // The same line is what presentation actually wrote, and it went to standard error.
    assert!(
        capture.stderr().contains(&rendered),
        "the warning belongs on standard error: {}",
        capture.stderr()
    );
    assert!(
        capture.suspended_around(&rendered),
        "the active extraction bar should clear before the warning and redraw after it"
    );
}

/// Finishes an images run of `documents_with_output` one-image documents and
/// `failed` failed documents, through the accumulator as production does.
fn outcome_with_failures(documents_with_output: usize, failed: usize) -> ExtractionRunOutcome {
    let mut accumulator =
        ExtractionRunOutcomeAccumulator::new(ApplicableOutcomeFacts::fabricated(false, None));
    for _ in 0..documents_with_output {
        let mut tally = EmittedImageTally::default();
        tally.record_normal_image(TallyRole::Preserved);
        accumulator.fold(&DocumentExtractionFacts::fabricated(tally, Vec::new()));
    }
    for _ in 0..failed {
        accumulator.record_failed_document();
    }
    accumulator.finish(false)
}

/// Verifies a produced summary names how many documents failed.
#[test]
fn produced_output_summary_appends_failed_documents() {
    assert_eq!(
        final_summary_message(&outcome_with_failures(2, 30)),
        "Extracted 2 image(s) from 2 document(s), 30 document(s) failed"
    );
}

/// Verifies a run with no output still says its documents failed, so "every
/// document failed" reads differently from "the documents held no images".
#[test]
fn no_output_summary_appends_failed_documents() {
    assert_eq!(
        final_summary_message(&outcome_with_failures(0, 3)),
        "No images found, 3 document(s) failed"
    );
    assert_eq!(
        final_summary_message(&ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Covers,
            failed_documents: NonZeroUsize::new(1),
        }),
        "No cover images found, 1 document(s) failed"
    );
}

/// Verifies the failure count follows every produced template, conversion and
/// GIF routing included, rather than one of them.
#[test]
fn conversion_and_gif_summary_appends_failed_documents() {
    let mut accumulator = ExtractionRunOutcomeAccumulator::new(ApplicableOutcomeFacts::fabricated(
        true,
        Some(PathBuf::from("gifs")),
    ));
    let mut tally = EmittedImageTally::default();
    tally.record_normal_image(TallyRole::Converted);
    tally.record_normal_image(TallyRole::RoutedGif);
    accumulator.fold(&DocumentExtractionFacts::fabricated(tally, Vec::new()));
    accumulator.record_failed_document();

    assert_eq!(
        final_summary_message(&accumulator.finish(false)),
        format!(
            "Extracted 2 image(s), converted 1, skipped 0, routed 1 GIF(s) to {} from 1 document(s), 1 document(s) failed",
            PathBuf::from("gifs").display()
        )
    );
}

#[test]
fn image_no_output_summary_preserves_existing_wording() {
    assert_eq!(
        final_summary_message(&ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: None
        }),
        "No images found"
    );
}

#[test]
fn cover_no_output_summary_preserves_existing_wording() {
    assert_eq!(
        final_summary_message(&ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Covers,
            failed_documents: None
        }),
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

/// Verifies every outcome shape clears the extraction display and prints its summary.
#[test]
fn terminal_observer_prints_every_nonempty_outcome_after_clearing_the_display() {
    let gif_dir = PathBuf::from("/tmp/gifs");
    let outcomes = [
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: None,
        },
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Covers,
            failed_documents: None,
        },
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
        assert_terminal_observation_clears_then_prints_the_summary(outcome);
    }
}

/// Verifies the capture reads back what a run draws before its summary.
///
/// The summary itself is printed, not drawn; see
/// [`assert_terminal_observation_clears_then_prints_the_summary`]. What a run does
/// draw on the way there is a phase caption and the per-document message that
/// replaces it. Both belong to the progress display and reach neither text stream,
/// so the capture is the only place they are readable.
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

/// Verifies the one outcome that has no display to clear is printed with no clear.
///
/// No documents means no extraction ever started, so there is no progress display
/// to clear. This is the companion case for
/// [`terminal_observer_prints_every_nonempty_outcome_after_clearing_the_display`].
/// The summary lands on the same stream, with nothing cleared or drawn around it.
#[test]
fn no_documents_terminal_outcome_is_printed_rather_than_drawn() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    presentation.on_observation(ExtractionRunObservation::Terminal(
        ExtractionRunOutcome::NoDocuments,
    ));

    // The whole transcript is one line on standard output: nothing was cleared or
    // drawn before it, and nothing reached standard error.
    assert_eq!(
        *capture.entries(),
        [TranscriptEntry::Stdout(
            "No documents found to process.".to_string()
        )]
    );
}

/// Builds a capture over a hand-written transcript, to pin what its readers conclude.
fn capture_of(entries: Vec<TranscriptEntry>) -> Capture {
    Capture {
        transcript: Transcript::new(Mutex::new(entries)),
    }
}

/// Verifies a line between a clear and a draw counts as suspended, past text entries.
#[test]
fn suspended_around_finds_the_nearest_progress_entries_past_text() {
    use TranscriptEntry::{Cleared, Drawn, Stderr, Stdout};

    let direct = capture_of(vec![
        Drawn("bar".to_string()),
        Cleared,
        Stderr("warning".to_string()),
        Drawn("bar".to_string()),
    ]);
    assert!(direct.suspended_around("warning"));

    // Other text written inside the same suspend is skipped, on either stream.
    let among_text = capture_of(vec![
        Drawn("bar".to_string()),
        Cleared,
        Stdout("before".to_string()),
        Stderr("warning".to_string()),
        Stderr("after".to_string()),
        Drawn("bar".to_string()),
    ]);
    assert!(among_text.suspended_around("warning"));
    assert!(among_text.suspended_around("before"));
}

/// Verifies a write missing either half of a suspend is not suspended.
#[test]
fn suspended_around_requires_a_clear_before_and_a_draw_after() {
    use TranscriptEntry::{Cleared, Drawn, Stderr};

    let never_cleared = capture_of(vec![
        Drawn("bar".to_string()),
        Stderr("warning".to_string()),
        Drawn("bar".to_string()),
    ]);
    assert!(!never_cleared.suspended_around("warning"));

    let never_redrawn = capture_of(vec![
        Drawn("bar".to_string()),
        Cleared,
        Stderr("warning".to_string()),
    ]);
    assert!(!never_redrawn.suspended_around("warning"));
}

/// Verifies every occurrence must be suspended, and an absent line never is.
#[test]
fn suspended_around_requires_every_occurrence_and_at_least_one() {
    use TranscriptEntry::{Cleared, Drawn, Stderr};

    let once_unsuspended = capture_of(vec![
        Drawn("bar".to_string()),
        Cleared,
        Stderr("warning".to_string()),
        Drawn("bar".to_string()),
        Stderr("warning".to_string()),
    ]);
    assert!(!once_unsuspended.suspended_around("warning"));

    // Lines are compared whole, so a prefix of a suspended line is a line never written.
    let suspended = capture_of(vec![
        Cleared,
        Stderr("warning".to_string()),
        Drawn("bar".to_string()),
    ]);
    assert!(!suspended.suspended_around("warn"));
    assert!(!capture_of(Vec::new()).suspended_around("warning"));
}

/// Verifies a summary arriving with no live display is printed rather than lost.
///
/// The run always starts extraction before a non-empty outcome, so this sequence
/// is unreachable through it today. The terminal arm must not depend on that
/// ordering: whatever the outcome, a summary with nothing live to finish belongs
/// on standard output, exactly as the no-documents summary does.
#[test]
fn terminal_summary_without_a_live_display_is_printed_rather_than_dropped() {
    let outcome = produced_outcome(ExtractionOutputKind::Images, 1, 1, None, None);
    let summary = final_summary_message(&outcome);
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    presentation.on_observation(ExtractionRunObservation::Terminal(outcome));

    assert_eq!(capture.stdout(), format!("{summary}\n"));
    assert_eq!(capture.stderr(), "");
    assert_eq!(capture.progress_text(), "");
}

/// The Document selection observations of a recursive, title-filtered EPUB run that
/// found three documents, kept two by the filter and skipped one as a duplicate.
///
/// Every phase opens its display with a running observation before its finished
/// one, as Document selection emits them. The duplicate diagnostic itself is left
/// out, because these tests are about the phase conclusions.
fn selection_phase_observations() -> Vec<ExtractionRunObservation> {
    let scope = DocumentDiscoveryScope::RecursiveDirectories;
    let filtering = |checked, matching| ExtractionRunObservation::FilteringEpubs {
        title: Some("Magic Book".to_string()),
        author: None,
        checked,
        total: 3,
        matching,
    };
    let deduplicating = |checked, duplicates_found, unique_remaining| {
        ExtractionRunObservation::DeduplicatingEpubs {
            checked,
            total: 2,
            duplicates_found,
            unique_remaining,
        }
    };

    vec![
        ExtractionRunObservation::DiscoveringDocuments {
            scope,
            discovered: 0,
        },
        ExtractionRunObservation::DiscoveringDocuments {
            scope,
            discovered: 3,
        },
        ExtractionRunObservation::DocumentDiscoveryFinished {
            scope,
            discovered: 3,
        },
        filtering(0, 0),
        filtering(3, 2),
        ExtractionRunObservation::EpubFilteringFinished {
            checked: 3,
            total: 3,
            matching: 2,
        },
        deduplicating(0, 0, 0),
        deduplicating(2, 1, 1),
        ExtractionRunObservation::EpubDeduplicationFinished {
            checked: 2,
            total: 2,
            duplicates_found: 1,
            unique_remaining: 1,
        },
    ]
}

/// The phase conclusions [`selection_phase_observations`] produces, in order.
const SELECTION_PHASE_CONCLUSIONS: [&str; 3] = [
    "Found 3 document(s)",
    "Found 2 matching EPUB(s)",
    "Removed 1 duplicate EPUB(s), 1 unique remaining",
];

/// Verifies each phase's conclusion is printed once its display has cleared.
///
/// A conclusion is meant to stay on screen after its phase ends. Left on the
/// finished bar, it would vanish with the display whenever standard error is not a
/// terminal (ADR-0026). Each one must instead come after a clear, with no draw
/// between that clear and the line.
#[test]
fn phase_conclusions_are_printed_after_their_display_clears() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    for observation in selection_phase_observations() {
        presentation.on_observation(observation);
    }

    assert_eq!(
        capture.stdout().lines().collect::<Vec<_>>(),
        SELECTION_PHASE_CONCLUSIONS
    );
    assert_eq!(capture.stderr(), "");
    let entries = capture.entries();
    for conclusion in SELECTION_PHASE_CONCLUSIONS {
        let index = entries
            .iter()
            .position(|entry| entry.text_line() == Some(conclusion))
            .expect("every conclusion was printed");
        assert_eq!(
            entries[..index]
                .iter()
                .rev()
                .find(|entry| entry.is_progress()),
            Some(&TranscriptEntry::Cleared),
            "{conclusion:?} should follow the clear of its phase's display"
        );
    }
    drop(entries);
    // Compared on the end of each drawn line, not as a substring: the running
    // discovery caption "Found 3 document(s)..." contains the conclusion's words
    // but is a different line. A finished bar would end with the conclusion.
    let progress = capture.progress_text();
    assert!(
        progress.lines().all(|drawn| SELECTION_PHASE_CONCLUSIONS
            .iter()
            .all(|conclusion| !drawn.trim_end().ends_with(conclusion))),
        "no conclusion should be drawn: {progress:?}"
    );
}

/// Verifies a deduplication phase that skipped nothing clears without a line.
#[test]
fn deduplication_without_duplicates_concludes_silently() {
    let (output, capture) = TerminalOutput::captured();
    let mut presentation = ExtractionRunPresentation::new(output);

    for observation in [
        ExtractionRunObservation::DeduplicatingEpubs {
            checked: 0,
            total: 2,
            duplicates_found: 0,
            unique_remaining: 0,
        },
        ExtractionRunObservation::EpubDeduplicationFinished {
            checked: 2,
            total: 2,
            duplicates_found: 0,
            unique_remaining: 2,
        },
    ] {
        presentation.on_observation(observation);
    }

    assert_eq!(capture.stdout(), "");
    assert_eq!(capture.stderr(), "");
    // The bar still has to go, or it would sit on screen under the next phase.
    assert_eq!(
        capture
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.is_progress()),
        Some(&TranscriptEntry::Cleared)
    );
}

/// Verifies a run with no terminal still says everything a user has to see.
///
/// This is H3 from the 2026-11-14 audit, pinned in-process. With the display hidden,
/// as it is under a pipe or a redirect, the phase conclusions, a warning and the
/// summary must all reach a text stream, in the order they happened.
#[test]
fn a_run_without_a_terminal_reports_conclusions_warnings_and_summary() {
    let (output, capture) = TerminalOutput::captured_without_terminal();
    let mut presentation = ExtractionRunPresentation::new(output);
    let document_path = PathBuf::from("documents").join("warned.docx");
    let warning = DocumentExtractionWarning::fabricated(ImageWriteWarning::ExtensionFallback {
        source_name: "word/media/only.png".to_string(),
        format: ImageFormat::Png,
    });
    let outcome = produced_outcome(ExtractionOutputKind::Images, 1, 1, None, None);
    let summary = final_summary_message(&outcome);

    let mut observations = selection_phase_observations();
    observations.extend([
        ExtractionRunObservation::ExtractionStarted {
            total: 1,
            cover_only: false,
        },
        ExtractionRunObservation::DocumentStarted {
            path: document_path.clone(),
            display_name: "warned.docx".to_string(),
        },
        ExtractionRunObservation::DocumentWarning {
            path: document_path.clone(),
            warning: warning.clone(),
        },
        ExtractionRunObservation::DocumentFinished {
            path: document_path.clone(),
        },
        ExtractionRunObservation::Terminal(outcome),
    ]);
    for observation in observations {
        presentation.on_observation(observation);
    }

    let expected = SELECTION_PHASE_CONCLUSIONS
        .iter()
        .map(|line| TranscriptEntry::Stdout(line.to_string()))
        .chain([
            TranscriptEntry::Stderr(document_warning_line(&document_path, &warning)),
            TranscriptEntry::Stdout(summary),
        ])
        .collect::<Vec<_>>();
    // The whole transcript, with no progress entries anywhere: a hidden display
    // draws and clears nothing, so only the text streams carry the run.
    assert_eq!(*capture.entries(), expected);
    assert_eq!(capture.progress_text(), "");
}

/// Verifies the published entry point prints its summary to a destination with no terminal.
///
/// This drives a real document through [`run_cli`], as the binary does under a
/// redirect. Before ADR-0026 the run below said nothing at all.
#[test]
fn run_cli_prints_the_summary_to_standard_output_without_a_terminal() {
    let temp_dir = temp_test_dir("presentation", "run-cli-without-terminal");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let document_path = temp_dir.join("sample.docx");
    write_docx(&document_path, &[("word/media/image1.png", MINIMAL_PNG)]);
    let args = Args::try_parse_from([
        "test",
        document_path.to_string_lossy().as_ref(),
        "--output",
        temp_dir.join("output").to_string_lossy().as_ref(),
    ])
    .expect("entry point arguments should parse");
    let (output, capture) = TerminalOutput::captured_without_terminal();

    let exit = run_cli(args, output).expect("a readable document is not an intake failure");

    assert_eq!(exit, ExitCode::SUCCESS);
    assert_eq!(
        capture.stdout(),
        "Extracted 1 image(s) from 1 document(s)\n"
    );
    assert_eq!(capture.stderr(), "");
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

    let exit = run_cli(args, output).expect("an empty input directory is not an intake failure");

    // Finding nothing is an answer rather than a failure, so it exits successfully.
    assert_eq!(exit, ExitCode::SUCCESS);
    assert_eq!(capture.stdout(), "No documents found to process.\n");
    assert_eq!(capture.stderr(), "");
}

/// Verifies a document that fails to extract makes the whole run exit with failure.
///
/// The exit status follows what the terminal shows: the run printed an `Error
/// processing` line, so it reports failure even though it reached its summary.
#[test]
fn run_cli_exits_with_failure_when_a_document_fails() {
    let temp_dir = temp_test_dir("presentation", "run-cli-failed-document");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let broken_path = temp_dir.join("broken.docx");
    fs::write(&broken_path, b"not a zip archive").expect("broken DOCX should be writable");
    let args = Args::try_parse_from([
        "test",
        broken_path.to_string_lossy().as_ref(),
        "--output",
        temp_dir.join("output").to_string_lossy().as_ref(),
    ])
    .expect("entry point arguments should parse");
    let (output, capture) = TerminalOutput::captured();

    let exit = run_cli(args, output).expect("a failed document is not an intake failure");

    assert_eq!(exit, ExitCode::FAILURE);
    assert!(
        capture.stderr().contains("Error processing"),
        "the failure should have been reported: {}",
        capture.stderr()
    );
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
