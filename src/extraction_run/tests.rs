//! Tests for the extraction run workflow.

use super::*;
use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
use crate::document_extraction::DocumentExtractionFacts;
use crate::extraction_run_intake::{self, Args};
use crate::extraction_run_observation::{DocumentDiscoveryScope, ProducedOutput};
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWriteCounts, ImageWriteResult, NormalImageOutput};
use crate::test_support::{
    DeclaredEpubDeclarations, InMemorySearchSurface, RecordingRunObserver,
    SilentExtractionRunObserver, no_fallback_directory, temp_test_dir, valid_png, write_docx,
    write_epub_document,
};
use clap::Parser;
use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroUsize;

/// Prepares one production request from directly built options.
///
/// Extraction run intake owns [`Args`], so a run-level test can name the options it
/// cares about and leave the rest at their parsed-with-no-flags values. Nothing here
/// has to know how a flag is spelled.
fn prepare_request_from(args: Args) -> ExtractionRunRequest {
    let prepared = extraction_run_intake::prepare(args, no_fallback_directory)
        .expect("Extraction run intake should succeed");
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

/// Document extraction scripted by document path, for run tests whose subject is not archives.
///
/// Cover intent and the Applicable outcome facts are delegated to a real
/// [`DocumentExtraction`] built from real policies, so a test that needs
/// conversion to apply builds a policy with conversion, as production does; only
/// `extract` is scripted (ADR-0016). Each scripted outcome can be taken once.
/// Extracting a path twice, or a path nobody scripted, panics: that is what turns
/// the request's "consumed exactly once" into something a test enforces, since
/// ownership alone guarantees one move per handoff but not one per path.
struct ScriptedDocumentExtraction {
    real_extraction: DocumentExtraction,
    /// A path whose slot is `None` has already been extracted, which is what
    /// tells a repeat apart from a path that was never scripted at all.
    outcomes: BTreeMap<PathBuf, Option<DocumentExtractionOutcome>>,
}

impl ScriptedDocumentExtraction {
    /// Wraps a real Document extraction bound to these policies, with nothing scripted yet.
    fn new(cover_policy: Option<EpubCoverPolicy>, image_write_policy: ImageWritePolicy) -> Self {
        Self {
            real_extraction: DocumentExtraction::new(
                cover_policy,
                ImageWritePipeline::new(image_write_policy),
            ),
            outcomes: BTreeMap::new(),
        }
    }

    /// Binds the policies a run with no flags gets: normal images, no conversion, no GIF routing.
    fn for_images() -> Self {
        Self::new(None, default_image_write_policy())
    }

    /// Binds the policies a `--cover-only` run gets, with no conversion or GIF routing.
    fn for_covers() -> Self {
        Self::new(
            Some(EpubCoverPolicy::CoverOnly),
            default_image_write_policy(),
        )
    }

    /// Scripts the outcome that extracting `path` returns.
    ///
    /// # Panics
    ///
    /// Panics when `path` already has a scripted outcome, since a second one
    /// could never be taken and would only hide a mistake in the test.
    fn with_outcome(
        mut self,
        path: impl Into<PathBuf>,
        outcome: DocumentExtractionOutcome,
    ) -> Self {
        let path = path.into();
        let previous = self.outcomes.insert(path.clone(), Some(outcome));
        assert!(
            previous.is_none(),
            "{} was scripted more than once",
            path.display()
        );
        self
    }
}

impl RunDocumentExtraction for ScriptedDocumentExtraction {
    fn is_epub_cover_extraction_configured(&self) -> bool {
        self.real_extraction.is_epub_cover_extraction_configured()
    }

    fn applicable_outcome_facts(&self) -> ApplicableOutcomeFacts {
        self.real_extraction.applicable_outcome_facts()
    }

    fn extract(&mut self, document: SelectedDocument) -> DocumentExtractionOutcome {
        let path = document.get_path();
        match self.outcomes.get_mut(path) {
            None => panic!(
                "no Document extraction outcome was scripted for {}",
                path.display()
            ),
            Some(slot) => slot
                .take()
                .unwrap_or_else(|| panic!("{} was extracted more than once", path.display())),
        }
    }
}

/// Returns the Image write policy intake builds when no image flag is given.
fn default_image_write_policy() -> ImageWritePolicy {
    ImageWritePolicy::new(ImageFormat::all_set(), None, None)
}

/// Fabricates a completed outcome from Image write counts, through Document extraction's own translation.
///
/// Going through the test entry point rather than around it means the partition
/// guard checks these counts as it would a real result's (ADR-0016).
fn completed(
    counts: ImageWriteCounts,
    normal_image_output: NormalImageOutput,
) -> DocumentExtractionOutcome {
    DocumentExtractionOutcome::Completed(DocumentExtractionFacts::fabricated(
        ImageWriteResult::new(counts, Vec::new(), normal_image_output),
    ))
}

/// Fabricates the completed outcome of a document that emitted nothing, such as a DOCX without media.
fn completed_without_images() -> DocumentExtractionOutcome {
    completed(ImageWriteCounts::default(), NormalImageOutput::Absent)
}

/// Runs the inner function with in-memory selection inputs and scripted extraction.
///
/// Document selection stays real, so selection diagnostics and extraction
/// observations interleave exactly as the two modules produce them together.
/// Inputs carry no output directory and no EPUB filter: placement is consumed
/// only by extraction, which is scripted here.
fn run_scripted(
    inputs: &[&str],
    recursive: bool,
    surface: &InMemorySearchSurface,
    declarations: &DeclaredEpubDeclarations,
    document_extraction: &mut ScriptedDocumentExtraction,
) -> (ExtractionRunOutcome, RecordingRunObserver) {
    let mut observer = RecordingRunObserver::default();
    let outcome = run_with(
        RunSelectionInputs {
            inputs: inputs.iter().map(PathBuf::from).collect(),
            recursive,
            output: None,
            epub_filter: EpubFilter::default(),
        },
        surface,
        declarations,
        document_extraction,
        &mut observer,
    );
    (outcome, observer)
}

/// Selects one declared DOCX through real Document selection, to obtain a handoff.
///
/// Selecting the same path twice is the only way to hold two handoffs for one
/// path, which is what the adapter's repeat-extraction guard needs to be shown.
fn select_docx_from_surface(surface: &InMemorySearchSurface, path: &str) -> SelectedDocument {
    let inputs = [PathBuf::from(path)];
    let mut selected = document_selection::select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
            epub_only: false,
        },
        surface,
        &DeclaredEpubDeclarations::new(),
        &mut SilentExtractionRunObserver,
    );
    assert_eq!(selected.len(), 1, "{path} should be selected exactly once");
    selected.remove(0)
}

/// Verifies the scripted adapter answers cover intent and outcome facts from its real policies.
#[test]
fn scripted_document_extraction_delegates_policy_facts_to_real_document_extraction() {
    let conversion = ConversionPolicy::try_from(ConversionRequest {
        target: ConversionTarget::Jpg,
        quality: None,
        lossless: false,
    })
    .expect("test conversion policy should be valid");
    let extraction = ScriptedDocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePolicy::new(
            ImageFormat::all_set(),
            Some(conversion),
            Some(PathBuf::from("gifs")),
        ),
    );

    assert!(extraction.is_epub_cover_extraction_configured());
    let applicable = extraction.applicable_outcome_facts();
    assert!(applicable.is_conversion_applicable());
    assert_eq!(
        applicable.into_gif_destination(),
        Some(PathBuf::from("gifs"))
    );

    let images = ScriptedDocumentExtraction::for_images();
    assert!(!images.is_epub_cover_extraction_configured());
    let applicable = images.applicable_outcome_facts();
    assert!(!applicable.is_conversion_applicable());
    assert_eq!(applicable.into_gif_destination(), None);
}

/// Verifies the scripted adapter refuses to hand one path's outcome out twice.
#[test]
#[should_panic(expected = "was extracted more than once")]
fn scripted_document_extraction_panics_when_a_path_is_extracted_twice() {
    let surface = InMemorySearchSurface::new().with_file("sample.docx");
    let mut extraction = ScriptedDocumentExtraction::for_images()
        .with_outcome("sample.docx", completed_without_images());

    let _ = extraction.extract(select_docx_from_surface(&surface, "sample.docx"));
    let _ = extraction.extract(select_docx_from_surface(&surface, "sample.docx"));
}

/// Verifies a run reaching an unscripted document fails loudly rather than inventing an outcome.
#[test]
#[should_panic(expected = "no Document extraction outcome was scripted")]
fn scripted_document_extraction_panics_on_an_unscripted_path() {
    let surface = InMemorySearchSurface::new().with_file("sample.docx");

    let _ = run_scripted(
        &["sample.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut ScriptedDocumentExtraction::for_images(),
    );
}

/// Verifies a run that selects nothing short-circuits to one No documents terminal observation.
#[test]
fn no_selected_documents_returns_no_documents_outcome() {
    let surface = InMemorySearchSurface::new().with_directory("empty");

    let (outcome, observer) = run_scripted(
        &["empty"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut ScriptedDocumentExtraction::for_images(),
    );

    assert_eq!(outcome, ExtractionRunOutcome::NoDocuments);
    assert_single_terminal_observation(&observer, &outcome);
    assert!(!observer.observations.iter().any(|observation| matches!(
        observation,
        ExtractionRunObservation::ExtractionStarted { .. }
            | ExtractionRunObservation::DocumentStarted { .. }
    )));
}

/// Verifies inspection failures on every requested input still end in exactly one terminal observation.
#[test]
fn all_failed_requested_inputs_reach_one_no_documents_terminal_observation() {
    // A link whose target is gone fails inspection rather than reading as a
    // missing input, which is the failure an unrepresentable path used to stage.
    let first_failed_path = PathBuf::from("first-input.docx");
    let second_failed_path = PathBuf::from("second-input.epub");
    let surface = InMemorySearchSurface::new()
        .with_link(first_failed_path.clone(), None)
        .with_link(second_failed_path.clone(), None);

    let (outcome, observer) = run_scripted(
        &["first-input.docx", "second-input.epub"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut ScriptedDocumentExtraction::for_images(),
    );

    assert_eq!(outcome, ExtractionRunOutcome::NoDocuments);
    assert_eq!(observer.observations.len(), 5);
    assert!(matches!(
        &observer.observations[0],
        ExtractionRunObservation::DocumentDiscoveryFailed { path, detail } if path == &first_failed_path && !detail.is_empty()
    ));
    assert!(matches!(
        &observer.observations[1],
        ExtractionRunObservation::DocumentDiscoveryFailed { path, detail } if path == &second_failed_path && !detail.is_empty()
    ));
    assert_eq!(
        observer.observations[2],
        ExtractionRunObservation::DiscoveringDocuments {
            scope: DocumentDiscoveryScope::RequestedInputs,
            discovered: 0
        }
    );
    assert_eq!(
        observer.observations[3],
        ExtractionRunObservation::DocumentDiscoveryFinished {
            scope: DocumentDiscoveryScope::RequestedInputs,
            discovered: 0
        }
    );
    assert_single_terminal_observation(&observer, &outcome);
    assert!(!observer.observations.iter().any(|observation| matches!(
        observation,
        ExtractionRunObservation::ExtractionStarted { .. }
            | ExtractionRunObservation::DocumentStarted { .. }
    )));
}

/// Verifies in-scan discovery diagnostics retain their order through the run seam.
#[test]
fn nested_discovery_failure_precedes_later_progress_and_extraction_in_run_stream() {
    let broken_link = PathBuf::from("requested/broken-link");
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_link(broken_link.clone(), None)
        .with_file("readable.docx");
    let mut document_extraction = ScriptedDocumentExtraction::for_images()
        .with_outcome("readable.docx", completed_without_images());

    let (outcome, observer) = run_scripted(
        &["requested", "readable.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: None
        }
    );
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0,
                    .. },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1,
                    .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1,
                    .. },
            ExtractionRunObservation::ExtractionStarted { total: 1, .. },
            ..
        ] if path == &broken_link && !detail.is_empty()
    ));
    assert_single_terminal_observation(&observer, &outcome);
}

/// Verifies recursive discovery diagnostics retain order in the unified run stream.
#[test]
fn recursive_discovery_failure_precedes_later_progress_and_extraction() {
    let broken_link = PathBuf::from("requested/broken-link");
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_link(broken_link.clone(), None)
        .with_file("readable.docx");
    let mut document_extraction = ScriptedDocumentExtraction::for_images()
        .with_outcome("readable.docx", completed_without_images());

    let (outcome, observer) = run_scripted(
        &["requested", "readable.docx"],
        true,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: None
        }
    );
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                    discovered: 0 },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                    discovered: 1 },
            ExtractionRunObservation::DocumentDiscoveryFinished { scope: DocumentDiscoveryScope::RecursiveDirectories,
                    discovered: 1 },
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
                ExtractionRunObservation::DocumentDiscoveryFailed { .. }
            ))
            .count(),
        1
    );
    assert_single_terminal_observation(&observer, &outcome);
}

/// Verifies selection's diagnostic and completion arrive before extraction in one ordered stream.
#[test]
fn selection_diagnostic_and_completion_precede_extraction_in_one_observation_stream() {
    let missing_path = PathBuf::from("missing.docx");
    let input_path = PathBuf::from("sample.docx");
    let surface = InMemorySearchSurface::new().with_file(input_path.clone());
    let mut document_extraction = ScriptedDocumentExtraction::for_images()
        .with_outcome(input_path.clone(), completed_without_images());

    let (outcome, observer) = run_scripted(
        &["missing.docx", "sample.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: None
        }
    );
    assert_eq!(
        observer.observations,
        vec![
            ExtractionRunObservation::MissingInput { path: missing_path },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RequestedInputs,
                discovered: 0
            },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RequestedInputs,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RequestedInputs,
                discovered: 1
            },
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
            ExtractionRunObservation::Terminal(ExtractionRunOutcome::NoOutput {
                output_kind: ExtractionOutputKind::Images,
                failed_documents: None
            }),
        ]
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
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: None
        }
    );
    assert_single_terminal_observation(&observer, &outcome);
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
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Covers,
            failed_documents: None
        }
    );
    assert_single_terminal_observation(&observer, &outcome);
}

/// Verifies a cover-only run diagnoses a requested DOCX and extracts only the EPUB.
#[test]
fn cover_only_run_skips_requested_docx_and_diagnoses_it() {
    // --cover-only extracts EPUB covers, and a DOCX has no cover to extract, so
    // it is not eligible work for the run at all. Selection drops it before the
    // run counts documents, which is why `total` is 1 rather than 2 and the DOCX
    // never reaches extraction. Naming the DOCX on the command line is what earns
    // the diagnostic; a DOCX swept up by traversal is dropped silently.
    let docx_path = PathBuf::from("sample.docx");
    let epub_path = PathBuf::from("book.epub");
    let surface = InMemorySearchSurface::new()
        .with_file(docx_path.clone())
        .with_file(epub_path.clone());
    let declarations = DeclaredEpubDeclarations::new().with_declarations(
        epub_path.clone(),
        Some("Test Creator"),
        Some("Covered"),
    );
    // Only the EPUB is scripted, so the DOCX reaching extraction would panic
    // rather than emit anything; one emitted image with no normal-image output
    // is what a written required cover looks like to Document extraction.
    let mut document_extraction = ScriptedDocumentExtraction::for_covers().with_outcome(
        epub_path.clone(),
        completed(
            ImageWriteCounts {
                extracted: 1,
                ..ImageWriteCounts::default()
            },
            NormalImageOutput::Absent,
        ),
    );

    let (outcome, observer) = run_scripted(
        &["sample.docx", "book.epub"],
        false,
        &surface,
        &declarations,
        &mut document_extraction,
    );
    let output = produced(&outcome);

    assert_eq!(output.output_kind(), ExtractionOutputKind::Covers);
    assert_eq!(output.emitted_images(), 1);
    assert_eq!(output.documents_with_output(), 1);
    // Deliberate departure from ADR-0016's "assertions unchanged": this test
    // used to assert that `Test Creator - Covered.jpg` existed and `sample.png`
    // did not. With extraction scripted, no file is written, so the run-level
    // half of that claim is asserted instead — the EPUB is the only document
    // handed to extraction, under its Document identity. The written cover name
    // stays checked on disk by
    // `epub_identity_is_consistent_across_normal_and_cover_runs`.
    assert_eq!(
        observer
            .observations
            .iter()
            .filter_map(|observation| match observation {
                ExtractionRunObservation::DocumentStarted { path, display_name } => {
                    Some((path.clone(), display_name.as_str()))
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![(epub_path.clone(), "Test Creator - Covered")]
    );
    assert_eq!(
        observer.selection_diagnostics(),
        vec![ExtractionRunObservation::SkippedNonEpubInput {
            path: docx_path.clone()
        }]
    );
    assert_single_terminal_observation(&observer, &outcome);
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
}

/// Verifies a run whose only document fails reports that failure without output.
///
/// "No images found" and "every document failed" used to be the same outcome; the
/// failure count is what now tells them apart.
#[test]
fn failed_document_without_output_is_counted_in_no_output() {
    let temp_dir = temp_test_dir("run", "failed-without-output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let broken_path = temp_dir.join("broken.docx");
    fs::write(&broken_path, b"not a zip archive").expect("broken DOCX should be writable");
    let request = prepare_request(vec![
        "test".to_string(),
        broken_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        temp_dir.join("output").to_string_lossy().into_owned(),
    ]);
    let mut observer = RecordingRunObserver::default();

    let outcome = run(request, &mut observer);

    assert_eq!(
        outcome,
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Images,
            failed_documents: NonZeroUsize::new(1),
        }
    );
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
    // The failed document's partial output still counts as output, and the failure
    // is a second fact the outcome carries beside it.
    assert_eq!(
        outcome,
        ExtractionRunOutcome::try_produced(
            ExtractionOutputKind::Images,
            NonZeroUsize::new(3).expect("three is nonzero"),
            NonZeroUsize::new(2).expect("two is nonzero"),
            None,
            None,
            NonZeroUsize::new(1),
        )
        .expect("expected outcome should be semantically valid")
    );
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
}
