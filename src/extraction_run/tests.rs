//! Tests for the extraction run workflow.

use super::*;
use crate::document_selection::{
    DocumentSelectionDiagnostic, DocumentSelectionPhaseStatus, DocumentSelectionProgress,
    DocumentSelectionScanScope,
};
use crate::extraction_run_intake::{self, Args};
use crate::test_support::{
    create_directory_link, remove_directory_link, temp_test_dir, write_docx, write_epub_document,
};
use clap::Parser;
use image::DynamicImage;
use std::fs;
use std::io::Cursor;

/// Records every live fact observed through the production run seam.
#[derive(Default)]
struct RecordingRunObserver {
    observations: Vec<ExtractionRunObservation>,
}

impl ExtractionRunObserver for RecordingRunObserver {
    /// Records the complete ordered live Extraction run timeline.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        self.observations.push(observation);
    }
}

/// Prepares one production request from directly built options.
///
/// Extraction run intake owns [`Args`], so a run-level test can name the options it
/// cares about and leave the rest at their parsed-with-no-flags values. Nothing here
/// has to know how a flag is spelled.
fn prepare_request_from(args: Args) -> ExtractionRunRequest {
    let prepared =
        extraction_run_intake::prepare(args).expect("Extraction run intake should succeed");
    assert!(prepared.notices.is_empty());
    prepared.request
}

/// Prepares one production request from argument strings.
///
/// The tests still using this predate intake owning the argument structure; they
/// convert to [`prepare_request_from`] separately, one behaviour at a time.
fn prepare_request(arguments: Vec<String>) -> ExtractionRunRequest {
    prepare_request_from(Args::try_parse_from(arguments).expect("test arguments should parse"))
}

/// Executes one production-built request with a recording observer.
fn execute(arguments: Vec<String>) -> ExtractionRunOutcome {
    let request = prepare_request(arguments);
    let mut observer = RecordingRunObserver::default();
    run(request, &mut observer)
}

/// Borrows produced-output facts from one semantic run outcome.
fn produced(outcome: &ExtractionRunOutcome) -> &ProducedOutput {
    match outcome {
        ExtractionRunOutcome::ProducedOutput(output) => output,
        other => panic!("expected produced output, got {other:?}"),
    }
}

/// Verifies that one run ends with exactly one terminal observation matching its return value.
fn assert_single_terminal_observation(
    observer: &RecordingRunObserver,
    outcome: &ExtractionRunOutcome,
) {
    let terminal_observations: Vec<_> = observer
        .observations
        .iter()
        .filter_map(|observation| match observation {
            ExtractionRunObservation::Terminal(observed_outcome) => Some(observed_outcome),
            _ => None,
        })
        .collect();

    assert_eq!(terminal_observations, vec![outcome]);
    assert_eq!(
        observer.observations.last(),
        Some(&ExtractionRunObservation::Terminal(outcome.clone()))
    );
}

/// Encodes a valid PNG payload for run-level conversion assertions.
fn valid_png() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::new_rgba8(1, 1)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("test PNG should encode");
    cursor.into_inner()
}

#[test]
fn no_selected_documents_returns_no_documents_outcome() {
    let temp_dir = temp_test_dir("run", "no-documents");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let request = prepare_request_from(Args {
        inputs: vec![temp_dir.clone()],
        ..Args::default()
    });
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(outcome, ExtractionRunOutcome::NoDocuments);
    assert_single_terminal_observation(&observer, &outcome);
    assert!(!observer.observations.iter().any(|observation| matches!(
        observation,
        ExtractionRunObservation::ExtractionStarted { .. }
            | ExtractionRunObservation::DocumentStarted { .. }
    )));

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn all_failed_requested_inputs_reach_one_no_documents_terminal_observation() {
    let temp_dir = temp_test_dir("run", "all-failed-requested-inputs");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let first_failed_path = temp_dir.join("first\0input.docx");
    let second_failed_path = temp_dir.join("second\0input.epub");
    let request = prepare_request_from(Args {
        inputs: vec![first_failed_path.clone(), second_failed_path.clone()],
        ..Args::default()
    });
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(outcome, ExtractionRunOutcome::NoDocuments);
    assert_eq!(observer.observations.len(), 5);
    assert!(matches!(
        &observer.observations[0],
        ExtractionRunObservation::DocumentSelectionDiagnostic(
            DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
        ) if path == &first_failed_path && !detail.is_empty()
    ));
    assert!(matches!(
        &observer.observations[1],
        ExtractionRunObservation::DocumentSelectionDiagnostic(
            DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
        ) if path == &second_failed_path && !detail.is_empty()
    ));
    assert_eq!(
        observer.observations[2],
        ExtractionRunObservation::DocumentSelectionProgress(DocumentSelectionProgress::Scanning {
            scope: DocumentSelectionScanScope::RequestedInputs,
            discovered: 0,
            status: DocumentSelectionPhaseStatus::Running,
        })
    );
    assert_eq!(
        observer.observations[3],
        ExtractionRunObservation::DocumentSelectionProgress(DocumentSelectionProgress::Scanning {
            scope: DocumentSelectionScanScope::RequestedInputs,
            discovered: 0,
            status: DocumentSelectionPhaseStatus::Finished,
        })
    );
    assert_single_terminal_observation(&observer, &outcome);
    assert!(!observer.observations.iter().any(|observation| matches!(
        observation,
        ExtractionRunObservation::ExtractionStarted { .. }
            | ExtractionRunObservation::DocumentStarted { .. }
    )));

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

/// Verifies in-scan discovery diagnostics retain their order through the run seam.
#[test]
fn nested_discovery_failure_precedes_later_progress_and_extraction_in_run_stream() {
    let temp_dir = temp_test_dir("run", "nested-discovery-failure");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    let readable_sibling = temp_dir.join("readable.docx");
    let output_directory = temp_dir.join("output");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    fs::create_dir_all(&output_directory).expect("output directory should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");
    write_docx(&readable_sibling, &[]);
    let request = prepare_request(vec![
        "test".to_string(),
        requested_directory.to_string_lossy().into_owned(),
        readable_sibling.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_directory.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images)
    );
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }
            ),
            ExtractionRunObservation::DocumentSelectionDiagnostic(
                DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                }
            ),
            ExtractionRunObservation::ExtractionStarted { total: 1, .. },
            ..
        ] if path == &broken_link && !detail.is_empty()
    ));
    assert_single_terminal_observation(&observer, &outcome);

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

/// Verifies recursive discovery diagnostics retain order in the unified run stream.
#[test]
fn recursive_discovery_failure_precedes_later_progress_and_extraction() {
    let temp_dir = temp_test_dir("run", "recursive-nested-discovery-failure");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    let readable_sibling = temp_dir.join("readable.docx");
    let output_directory = temp_dir.join("output");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    fs::create_dir_all(&output_directory).expect("output directory should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");
    write_docx(&readable_sibling, &[]);
    let request = prepare_request_from(Args {
        inputs: vec![requested_directory, readable_sibling],
        recursive: true,
        output: Some(output_directory),
        ..Args::default()
    });
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images)
    );
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                }
            ),
            ExtractionRunObservation::DocumentSelectionDiagnostic(
                DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                }
            ),
            ExtractionRunObservation::ExtractionStarted { total: 1, .. },
            ..
        ] if path == &broken_link && !detail.is_empty()
    ));
    assert_eq!(
        observer
            .observations
            .iter()
            .filter(|observation| matches!(
                observation,
                ExtractionRunObservation::DocumentSelectionDiagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { .. }
                )
            ))
            .count(),
        1
    );
    assert_single_terminal_observation(&observer, &outcome);

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn selection_diagnostic_and_completion_precede_extraction_in_one_observation_stream() {
    let temp_dir = temp_test_dir("run", "selection-extraction-boundary");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let missing_path = temp_dir.join("missing.docx");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("output");
    write_docx(&input_path, &[]);
    let request = prepare_request(vec![
        "test".to_string(),
        missing_path.to_string_lossy().into_owned(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images)
    );
    assert_eq!(
        observer.observations,
        vec![
            ExtractionRunObservation::DocumentSelectionDiagnostic(
                DocumentSelectionDiagnostic::MissingInput { path: missing_path }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RequestedInputs,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RequestedInputs,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                }
            ),
            ExtractionRunObservation::DocumentSelectionProgress(
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RequestedInputs,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                }
            ),
            ExtractionRunObservation::ExtractionStarted {
                total: 1,
                cover_only: false,
            },
            ExtractionRunObservation::DocumentStarted {
                path: input_path.clone(),
                display_name: "sample.docx".to_string(),
            },
            ExtractionRunObservation::DocumentFinished {
                path: input_path.clone(),
            },
            ExtractionRunObservation::Terminal(ExtractionRunOutcome::NoOutput(
                ExtractionOutputKind::Images,
            )),
        ]
    );

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn produced_outcome_rejects_inconsistent_semantic_totals() {
    let one = NonZeroUsize::new(1).expect("one should be nonzero");
    let two = NonZeroUsize::new(2).expect("two should be nonzero");

    assert!(
        ExtractionRunOutcome::try_produced(ExtractionOutputKind::Images, one, two, None, None,)
            .is_none()
    );
    assert!(
        ExtractionRunOutcome::try_produced(
            ExtractionOutputKind::Images,
            one,
            one,
            Some(ConversionFacts::new(1, 0)),
            Some(GifRoutingFacts::new(one, PathBuf::from("gifs"))),
        )
        .is_none()
    );
}

#[test]
fn selected_document_without_images_returns_image_no_output() {
    let temp_dir = temp_test_dir("run", "image-no-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("empty.docx");
    let output_dir = temp_dir.join("output");
    write_docx(&input_path, &[]);

    let request = prepare_request(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Images)
    );
    assert_single_terminal_observation(&observer, &outcome);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn selected_epub_without_a_cover_returns_cover_no_output() {
    let temp_dir = temp_test_dir("run", "cover-no-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("empty.epub");
    let output_dir = temp_dir.join("output");
    write_epub_document(&input_path, "Test Creator", "No Cover", None);

    let request = prepare_request(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
        "--cover-only".to_string(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Covers)
    );
    assert_single_terminal_observation(&observer, &outcome);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn normal_document_output_returns_produced_images() {
    let temp_dir = temp_test_dir("run", "normal-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("output");
    write_docx(
        &input_path,
        &[("word/media/image.png", b"\x89PNG\r\n\x1A\n")],
    );

    let request = prepare_request(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);
    let output = produced(&outcome);

    assert_eq!(output.output_kind(), ExtractionOutputKind::Images);
    assert_eq!(output.emitted_images(), 1);
    assert_eq!(output.documents_with_output(), 1);
    assert!(output.conversion().is_none());
    assert!(output.gif_routing().is_none());
    assert_single_terminal_observation(&observer, &outcome);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn epub_normal_fallback_is_classified_as_images() {
    let temp_dir = temp_test_dir("run", "normal-fallback-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("fallback.epub");
    let output_dir = temp_dir.join("output");
    write_epub_document(
        &input_path,
        "Test Creator",
        "Fallback",
        Some(("interior.jpg", b"\xFF\xD8\xFFinterior", false)),
    );

    let outcome = execute(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
        "--cover-only".to_string(),
        "--cover-fallback".to_string(),
    ]);

    assert_eq!(
        produced(&outcome).output_kind(),
        ExtractionOutputKind::Images
    );

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn requested_conversion_retains_valid_zero_totals() {
    let temp_dir = temp_test_dir("run", "zero-conversion-totals");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("matching.docx");
    let output_dir = temp_dir.join("output");
    write_docx(
        &input_path,
        &[("word/media/image.jpg", b"\xFF\xD8\xFFmatching")],
    );

    let outcome = execute(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
        "--convert".to_string(),
        "jpg".to_string(),
    ]);

    assert_eq!(
        produced(&outcome).conversion(),
        Some(&ConversionFacts::new(0, 0))
    );

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn routed_gif_retains_its_count_and_destination() {
    let temp_dir = temp_test_dir("run", "gif-routing");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("animation.docx");
    let output_dir = temp_dir.join("output");
    let gif_destination = temp_dir.join("gifs");
    write_docx(&input_path, &[("word/media/animation.gif", b"GIF89a")]);

    let outcome = execute(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
        "--gif-output".to_string(),
        gif_destination.to_string_lossy().into_owned(),
    ]);
    let output = produced(&outcome);
    let gif_routing = output
        .gif_routing()
        .expect("GIF routing facts should apply");

    assert!(output.conversion().is_none());
    assert_eq!(gif_routing.routed_gifs(), 1);
    assert_eq!(gif_routing.destination(), gif_destination);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn produced_outcome_retains_combined_conversion_and_gif_routing_facts() {
    let temp_dir = temp_test_dir("run", "document-fact-aggregation");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("output");
    let gif_output = temp_dir.join("gifs");
    let png = valid_png();
    write_docx(
        &input_path,
        &[
            ("word/media/image.png", &png),
            ("word/media/animation.gif", b"GIF89a"),
            ("word/media/vector.svg", b"<svg/>"),
        ],
    );
    let request = prepare_request(vec![
        "test".to_string(),
        input_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
        "--formats".to_string(),
        "png,gif,svg".to_string(),
        "--convert".to_string(),
        "jpg".to_string(),
        "--gif-output".to_string(),
        gif_output.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);
    let output = produced(&outcome);

    assert_eq!(output.output_kind(), ExtractionOutputKind::Images);
    assert_eq!(output.emitted_images(), 3);
    assert_eq!(output.documents_with_output(), 1);
    assert_eq!(output.conversion(), Some(&ConversionFacts::new(1, 1)));
    let gif_routing = output
        .gif_routing()
        .expect("routed GIF facts should be present");
    assert_eq!(gif_routing.routed_gifs(), 1);
    assert_eq!(gif_routing.destination(), gif_output);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn epub_identity_is_consistent_across_normal_and_cover_runs() {
    let temp_dir = temp_test_dir("run", "epub-identity-across-policies");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("filename.epub");
    write_epub_document(
        &input_path,
        "Test Creator",
        "Declared Title",
        Some(("cover.jpg", b"\xFF\xD8\xFFcover", true)),
    );
    let run_cases = [
        (
            false,
            ExtractionOutputKind::Images,
            temp_dir.join("normal-output"),
        ),
        (
            true,
            ExtractionOutputKind::Covers,
            temp_dir.join("cover-output"),
        ),
    ];
    let mut display_names = Vec::new();

    for (cover_only, expected_output_kind, output_dir) in run_cases {
        let mut arguments = vec![
            "test".to_string(),
            input_path.to_string_lossy().into_owned(),
            "--output".to_string(),
            output_dir.to_string_lossy().into_owned(),
            "--formats".to_string(),
            "jpg".to_string(),
        ];
        if cover_only {
            arguments.push("--cover-only".to_string());
        }
        let request = prepare_request(arguments);
        let mut observer = RecordingRunObserver::default();

        let outcome = run(request, &mut observer);

        assert_eq!(produced(&outcome).output_kind(), expected_output_kind);
        assert!(observer.observations.iter().any(|observation| {
            matches!(
                observation,
                ExtractionRunObservation::ExtractionStarted {
                    total: 1,
                    cover_only: observed_cover_only
                } if *observed_cover_only == cover_only
            )
        }));
        assert!(!observer.observations.iter().any(|observation| matches!(
            observation,
            ExtractionRunObservation::DocumentError { .. }
        )));
        let display_name = observer
            .observations
            .iter()
            .find_map(|observation| match observation {
                ExtractionRunObservation::DocumentStarted { display_name, .. } => {
                    Some(display_name.clone())
                }
                _ => None,
            })
            .expect("selected EPUB should emit a start observation");
        display_names.push(display_name);
    }

    assert_eq!(
        display_names,
        [
            "Test Creator - Declared Title",
            "Test Creator - Declared Title"
        ]
    );

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn run_retains_partial_facts_and_continues_after_document_failure() {
    let temp_dir = temp_test_dir("run", "partial-failure-continuation");
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
            ("word/media/second.png", b"also not actually a png"),
            ("word/media/third.gif", b"GIF89a"),
        ],
    );
    write_docx(
        &succeeding_path,
        &[("word/media/image.png", b"\x89PNG\r\n\x1A\n")],
    );
    let request = prepare_request(vec![
        "test".to_string(),
        failing_path.to_string_lossy().into_owned(),
        succeeding_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
        "--formats".to_string(),
        "png,gif".to_string(),
        "--gif-output".to_string(),
        blocked_gif_output.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);
    let output = produced(&outcome);

    assert_eq!(output.output_kind(), ExtractionOutputKind::Images);
    assert_eq!(output.emitted_images(), 3);
    assert_eq!(output.documents_with_output(), 2);
    assert!(output_dir.join("failing_1.png").exists());
    assert!(output_dir.join("failing_2.png").exists());
    assert!(output_dir.join("succeeding.png").exists());

    let failing_start = observer
        .observations
        .iter()
        .position(|observation| matches!(observation, ExtractionRunObservation::DocumentStarted { path, .. } if path == &failing_path))
        .expect("failed document should start");
    let warning_indices: Vec<_> = observer
        .observations
        .iter()
        .enumerate()
        .filter_map(|(index, observation)| {
            matches!(observation, ExtractionRunObservation::DocumentWarning { path, .. } if path == &failing_path)
                .then_some(index)
        })
        .collect();
    assert_eq!(warning_indices.len(), 2);
    // Wording belongs to Document extraction; the run only has to keep both
    // distinct opaque values instead of collapsing them into one.
    let warnings: Vec<_> = warning_indices
        .iter()
        .map(|index| match &observer.observations[*index] {
            ExtractionRunObservation::DocumentWarning { warning, .. } => warning,
            _ => unreachable!("warning indices must reference warning observations"),
        })
        .collect();
    assert_ne!(warnings[0], warnings[1]);
    let error_index = observer
        .observations
        .iter()
        .position(|observation| matches!(observation, ExtractionRunObservation::DocumentError { path, message } if path == &failing_path && message.contains("Failed to create output directory")))
        .expect("failed document error should be emitted");
    let failing_finish_indices: Vec<_> = observer
        .observations
        .iter()
        .enumerate()
        .filter_map(|(index, observation)| {
            matches!(observation, ExtractionRunObservation::DocumentFinished { path } if path == &failing_path)
                .then_some(index)
        })
        .collect();
    assert_eq!(failing_finish_indices.len(), 1);
    assert!(failing_start < warning_indices[0]);
    assert!(warning_indices[0] < warning_indices[1]);
    assert!(warning_indices[1] < error_index);
    assert!(error_index < failing_finish_indices[0]);
    let succeeding_start = observer
        .observations
        .iter()
        .position(|observation| matches!(observation, ExtractionRunObservation::DocumentStarted { path, .. } if path == &succeeding_path))
        .expect("later document should start");
    assert!(failing_finish_indices[0] < succeeding_start);
    let succeeding_finish_indices: Vec<_> = observer
        .observations
        .iter()
        .enumerate()
        .filter_map(|(index, observation)| {
            matches!(observation, ExtractionRunObservation::DocumentFinished { path } if path == &succeeding_path)
                .then_some(index)
        })
        .collect();
    assert_eq!(succeeding_finish_indices.len(), 1);
    assert!(succeeding_start < succeeding_finish_indices[0]);
    assert_single_terminal_observation(&observer, &outcome);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

/// Verifies the run transports opaque warning values with their document paths.
///
/// Stable wording is owned by Document extraction, so this asserts only the
/// carried value, its path, its multiplicity, and its observation position.
#[test]
fn run_carries_opaque_document_extraction_warnings_with_originating_paths() {
    let temp_dir = temp_test_dir("run", "opaque-warning-transport");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let first_path = temp_dir.join("first.docx");
    let second_path = temp_dir.join("second.docx");
    let output_dir = temp_dir.join("output");
    write_docx(
        &first_path,
        &[
            ("word/media/alpha.png", b"not actually a png"),
            ("word/media/beta.png", b"also not actually a png"),
        ],
    );
    // The shared entry name makes the second document reproduce the first
    // document's second warning value, which anchors intra-document order
    // below without any test knowing the stable wording.
    write_docx(&second_path, &[("word/media/beta.png", b"still not a png")]);
    let request = prepare_request(vec![
        "test".to_string(),
        first_path.to_string_lossy().into_owned(),
        second_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_dir.to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);
    let output = produced(&outcome);

    assert_eq!(output.emitted_images(), 3);
    let warnings: Vec<_> = observer
        .observations
        .iter()
        .enumerate()
        .filter_map(|(index, observation)| match observation {
            ExtractionRunObservation::DocumentWarning { path, warning } => {
                Some((index, path.clone(), warning.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(warnings.len(), 3);
    assert_eq!(
        warnings
            .iter()
            .map(|(_, path, _)| path.clone())
            .collect::<Vec<_>>(),
        vec![first_path.clone(), first_path.clone(), second_path.clone()]
    );
    // Distinct source facts must survive transport as distinct opaque values.
    assert_ne!(warnings[0].2, warnings[1].2);
    // Both documents warn about `word/media/beta.png`, so the shared value
    // pins which slot each source occupies: a reordered or deduplicated run
    // would move that value out of the first document's second slot.
    assert_eq!(warnings[1].2, warnings[2].2);
    assert_ne!(warnings[0].2, warnings[2].2);

    let started_at = |path: &PathBuf| {
        observer
            .observations
            .iter()
            .position(|observation| {
                matches!(observation, ExtractionRunObservation::DocumentStarted { path: observed, .. } if observed == path)
            })
            .expect("document should start")
    };
    let finished_at = |path: &PathBuf| {
        observer
            .observations
            .iter()
            .position(|observation| {
                matches!(observation, ExtractionRunObservation::DocumentFinished { path: observed } if observed == path)
            })
            .expect("document should finish")
    };
    assert!(started_at(&first_path) < warnings[0].0);
    assert!(warnings[0].0 < warnings[1].0);
    assert!(warnings[1].0 < finished_at(&first_path));
    assert!(finished_at(&first_path) < started_at(&second_path));
    assert!(started_at(&second_path) < warnings[2].0);
    assert!(warnings[2].0 < finished_at(&second_path));
    assert_single_terminal_observation(&observer, &outcome);

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}
