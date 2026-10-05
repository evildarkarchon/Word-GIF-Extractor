//! Tests for the extraction run workflow.

use super::*;
use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
use crate::document_extraction::{
    DocumentExtractionError, DocumentExtractionFacts, DocumentExtractionWarning,
};
use crate::emitted_image_tally::{EmittedImageTally, TallyRole};
use crate::extraction_run_intake::{self, Args};
use crate::extraction_run_observation::{DocumentDiscoveryScope, ProducedOutput};
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::ImageWriteWarning;
use crate::test_support::{
    DeclaredEpubDeclarations, InMemorySearchSurface, RecordingRunObserver,
    SilentExtractionRunObserver, no_fallback_directory, temp_test_dir, write_epub_document,
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

/// Returns the conversion policy `--convert jpg` yields, with no quality or lossless flag.
fn jpg_conversion() -> ConversionPolicy {
    ConversionPolicy::try_from(ConversionRequest {
        target: ConversionTarget::Jpg,
        quality: None,
        lossless: false,
    })
    .expect("test conversion policy should be valid")
}

/// Returns the tally of a document that wrote `count` normal images as extracted.
fn normal_images(count: usize) -> EmittedImageTally {
    normal_images_in(&vec![TallyRole::Preserved; count])
}

/// Returns the tally of a document that wrote one normal image per role, in order.
fn normal_images_in(roles: &[TallyRole]) -> EmittedImageTally {
    let mut tally = EmittedImageTally::default();
    for &role in roles {
        tally.record_normal_image(role);
    }
    tally
}

/// Returns the tally of an EPUB that wrote its one required cover as extracted.
fn one_cover() -> EmittedImageTally {
    let mut tally = EmittedImageTally::default();
    tally.record_cover(TallyRole::Preserved);
    tally
}

/// Returns the warning the Image write pipeline records when magic detection fails for one archive entry.
///
/// Built as an Image write pipeline fact rather than as wording, so the stable message
/// stays owned by Document extraction's translation.
fn extension_fallback(source_name: &str) -> ImageWriteWarning {
    ImageWriteWarning::ExtensionFallback {
        source_name: source_name.to_string(),
        format: ImageFormat::Png,
    }
}

/// Fabricates Document extraction facts from a tally and Image write warnings.
///
/// Each warning goes through Document extraction's warning entry point, so its
/// wording is translated exactly as a real one's is (ADR-0016). The tally needs
/// no translation: Document extraction hands it over as it is (ADR-0017).
fn facts(tally: EmittedImageTally, warnings: Vec<ImageWriteWarning>) -> DocumentExtractionFacts {
    DocumentExtractionFacts::fabricated(
        tally,
        warnings
            .into_iter()
            .map(DocumentExtractionWarning::fabricated)
            .collect(),
    )
}

/// Fabricates a completed outcome from a tally, with no warnings.
fn completed(tally: EmittedImageTally) -> DocumentExtractionOutcome {
    DocumentExtractionOutcome::Completed(facts(tally, Vec::new()))
}

/// Fabricates the completed outcome of a document that emitted nothing, such as a DOCX without media.
fn completed_without_images() -> DocumentExtractionOutcome {
    completed(EmittedImageTally::default())
}

/// Fabricates a failed outcome retaining `facts`, whose error is sealed from `cause`.
///
/// The cause is the test's own wording, not Document extraction's: the run only
/// transports the error's display, so a test compares against what it scripted.
fn failed(facts: DocumentExtractionFacts, cause: &str) -> DocumentExtractionOutcome {
    DocumentExtractionOutcome::Failed {
        facts,
        error: DocumentExtractionError::fabricated(anyhow::Error::msg(cause.to_string())),
    }
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
    let extraction = ScriptedDocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePolicy::new(
            ImageFormat::all_set(),
            Some(jpg_conversion()),
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

/// Verifies a cover-only run whose EPUB has no cover classifies as cover no-output.
#[test]
fn selected_epub_without_a_cover_returns_cover_no_output() {
    let surface = InMemorySearchSurface::new().with_file("empty.epub");
    let declarations = DeclaredEpubDeclarations::new().with_declarations(
        "empty.epub",
        Some("Test Creator"),
        Some("No Cover"),
    );
    // A missing required cover under `CoverOnly` emits nothing, which is all
    // Document extraction reports for it.
    let mut document_extraction = ScriptedDocumentExtraction::for_covers()
        .with_outcome("empty.epub", completed_without_images());

    let (outcome, observer) = run_scripted(
        &["empty.epub"],
        false,
        &surface,
        &declarations,
        &mut document_extraction,
    );

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
    // rather than emit anything; the EPUB writes its one required cover.
    let mut document_extraction = ScriptedDocumentExtraction::for_covers()
        .with_outcome(epub_path.clone(), completed(one_cover()));

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

/// Verifies requested conversion is reported even when nothing was converted or skipped.
#[test]
fn requested_conversion_retains_valid_zero_totals() {
    let surface = InMemorySearchSurface::new().with_file("matching.docx");
    // A JPG already in the `--convert jpg` target is emitted as-is: neither
    // converted nor skipped, so both conversion counts stay at zero.
    let mut document_extraction = ScriptedDocumentExtraction::new(
        None,
        ImageWritePolicy::new(ImageFormat::all_set(), Some(jpg_conversion()), None),
    )
    .with_outcome("matching.docx", completed(normal_images(1)));

    let (outcome, _) = run_scripted(
        &["matching.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );

    assert_eq!(
        produced(&outcome).conversion(),
        Some(&ConversionFacts::new(0, 0))
    );
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

/// Verifies a failed document's partial facts are kept and later documents still run.
#[test]
fn run_retains_partial_facts_and_continues_after_document_failure() {
    let failing_path = PathBuf::from("failing.docx");
    let succeeding_path = PathBuf::from("succeeding.docx");
    let failure_cause = "scripted failure routing third.gif";
    let surface = InMemorySearchSurface::new()
        .with_file(failing_path.clone())
        .with_file(succeeding_path.clone());
    // The failing document wrote two PNGs, each by extension fallback, before
    // routing its GIF failed. GIF routing is configured, so its fact group
    // applies, but no GIF was routed and the outcome omits it.
    let mut document_extraction = ScriptedDocumentExtraction::new(
        None,
        ImageWritePolicy::new(
            ImageFormat::all_set(),
            None,
            Some(PathBuf::from("blocked-gifs")),
        ),
    )
    .with_outcome(
        failing_path.clone(),
        failed(
            facts(
                normal_images(2),
                vec![
                    extension_fallback("word/media/first.png"),
                    extension_fallback("word/media/second.png"),
                ],
            ),
            failure_cause,
        ),
    )
    .with_outcome(succeeding_path.clone(), completed(normal_images(1)));

    let (outcome, observer) = run_scripted(
        &["failing.docx", "succeeding.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );
    let output = produced(&outcome);

    assert_eq!(output.output_kind(), ExtractionOutputKind::Images);
    assert_eq!(output.emitted_images(), 3);
    assert_eq!(output.documents_with_output(), 2);
    // The failed document's partial output still counts as output, and the failure
    // is a second fact the outcome carries beside it.
    assert!(output.conversion().is_none());
    assert!(output.gif_routing().is_none());
    assert_eq!(outcome.failed_documents(), NonZeroUsize::new(1));
    // Deliberate departure from ADR-0016's "assertions unchanged", as in
    // `cover_only_run_skips_requested_docx_and_diagnoses_it`: this test used to
    // assert that `failing_1.png`, `failing_2.png` and `succeeding.png` existed,
    // and that the error contained "Failed to create output directory". Scripted
    // extraction writes no files, and that wording belongs to the Image write
    // pipeline. The run-level half stays — emitted images and documents with
    // output above, and the error carrying the scripted cause below. Partial
    // output on disk and the wording stay checked in place by
    // `document_extraction::tests::failed_extraction_retains_document_extraction_facts`,
    // and a completed DOCX's output on disk by `tests/binary_smoke.rs`.

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
        .position(|observation| matches!(observation, ExtractionRunObservation::DocumentError { path, message } if path == &failing_path && message.contains(failure_cause)))
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
    let first_path = PathBuf::from("first.docx");
    let second_path = PathBuf::from("second.docx");
    let surface = InMemorySearchSurface::new()
        .with_file(first_path.clone())
        .with_file(second_path.clone());
    // The shared entry name makes the second document reproduce the first
    // document's second warning value, which anchors intra-document order
    // below without any test knowing the stable wording.
    let mut document_extraction = ScriptedDocumentExtraction::for_images()
        .with_outcome(
            first_path.clone(),
            DocumentExtractionOutcome::Completed(facts(
                normal_images(2),
                vec![
                    extension_fallback("word/media/alpha.png"),
                    extension_fallback("word/media/beta.png"),
                ],
            )),
        )
        .with_outcome(
            second_path.clone(),
            DocumentExtractionOutcome::Completed(facts(
                normal_images(1),
                vec![extension_fallback("word/media/beta.png")],
            )),
        );

    let (outcome, observer) = run_scripted(
        &["first.docx", "second.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );
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

/// Verifies every failed document is counted, whether or not it wrote anything first.
#[test]
fn every_failed_document_is_counted_in_the_outcome() {
    let partial_path = PathBuf::from("partial.docx");
    let broken_path = PathBuf::from("broken.docx");
    let surface = InMemorySearchSurface::new()
        .with_file(partial_path.clone())
        .with_file(broken_path.clone())
        .with_file("succeeding.docx");
    let mut document_extraction = ScriptedDocumentExtraction::for_images()
        .with_outcome(
            partial_path.clone(),
            failed(
                facts(normal_images(1), Vec::new()),
                "scripted failure after one image",
            ),
        )
        .with_outcome(
            broken_path.clone(),
            failed(
                facts(EmittedImageTally::default(), Vec::new()),
                "scripted failure before any image",
            ),
        )
        .with_outcome("succeeding.docx", completed(normal_images(1)));

    let (outcome, observer) = run_scripted(
        &["partial.docx", "broken.docx", "succeeding.docx"],
        false,
        &surface,
        &DeclaredEpubDeclarations::new(),
        &mut document_extraction,
    );

    let output = produced(&outcome);

    assert_eq!(output.output_kind(), ExtractionOutputKind::Images);
    assert_eq!(output.emitted_images(), 2);
    assert_eq!(output.documents_with_output(), 2);
    assert!(output.conversion().is_none());
    assert!(output.gif_routing().is_none());
    assert_eq!(outcome.failed_documents(), NonZeroUsize::new(2));
    // The outcome counts a failure exactly where the stream reports one.
    assert_eq!(
        observer
            .observations
            .iter()
            .filter_map(|observation| match observation {
                ExtractionRunObservation::DocumentError { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![partial_path, broken_path]
    );
    assert_single_terminal_observation(&observer, &outcome);
}
