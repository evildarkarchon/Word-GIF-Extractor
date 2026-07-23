//! Extraction run workflow for document selection, sequencing, and aggregation.

use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::conversion::ConversionPolicy;
use crate::document_extraction::{
    DocumentExtraction, DocumentExtractionFacts, DocumentExtractionOutcome,
    DocumentExtractionPolicy,
};
use crate::document_selection::{
    self, DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionOptions,
    DocumentSelectionProgress, EpubFilter,
};
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};

/// Opaque, ready-to-execute handoff produced by Extraction run intake.
///
/// Construction derives presentation-relevant intent from the same policies
/// used for extraction, so conversion and GIF-routing facts cannot drift from
/// the configured workflow. An Extraction run consumes this request by value.
pub(crate) struct ExtractionRunRequest {
    inputs: Vec<PathBuf>,
    recursive: bool,
    output: Option<PathBuf>,
    epub_filter: EpubFilter,
    document_extraction: DocumentExtraction,
    conversion_requested: bool,
    gif_destination: Option<PathBuf>,
}

impl ExtractionRunRequest {
    /// Builds one valid request from normalized inputs and workflow policies.
    ///
    /// Conversion intent and the GIF destination are retained alongside the
    /// configured Image write pipeline for outcome classification.
    #[expect(
        clippy::too_many_arguments,
        reason = "named normalized facts avoid exposing a parallel request-parts model"
    )]
    pub(crate) fn new(
        inputs: Vec<PathBuf>,
        recursive: bool,
        output: Option<PathBuf>,
        epub_filter: EpubFilter,
        document_extraction_policy: DocumentExtractionPolicy,
        allowed_formats: HashSet<ImageFormat>,
        conversion: Option<ConversionPolicy>,
        gif_destination: Option<PathBuf>,
    ) -> Self {
        let conversion_requested = conversion.is_some();
        let image_write_pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            allowed_formats,
            conversion,
            gif_destination.clone(),
        ));

        Self {
            inputs,
            recursive,
            output,
            epub_filter,
            document_extraction: DocumentExtraction::new(
                document_extraction_policy,
                image_write_pipeline,
            ),
            conversion_requested,
            gif_destination,
        }
    }
}

/// Semantic classification of output produced or sought by an Extraction run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtractionOutputKind {
    /// Normal document images, including EPUB normal-image fallback output.
    Images,
    /// Required EPUB covers when no normal-image output was emitted.
    Covers,
}

/// Conversion totals retained only when conversion was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConversionFacts {
    converted_images: usize,
    skipped_conversions: usize,
}

impl ConversionFacts {
    /// Creates applicable conversion facts, including valid zero totals.
    pub(crate) fn new(converted_images: usize, skipped_conversions: usize) -> Self {
        Self {
            converted_images,
            skipped_conversions,
        }
    }

    /// Returns the number of images converted before emission.
    pub(crate) fn converted_images(&self) -> usize {
        self.converted_images
    }

    /// Returns the number of requested conversions that preserved source bytes.
    pub(crate) fn skipped_conversions(&self) -> usize {
        self.skipped_conversions
    }
}

/// Routed-GIF facts retained together with their required destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GifRoutingFacts {
    routed_gifs: NonZeroUsize,
    destination: PathBuf,
}

impl GifRoutingFacts {
    /// Creates applicable GIF-routing facts with a positive routed count.
    pub(crate) fn new(routed_gifs: NonZeroUsize, destination: PathBuf) -> Self {
        Self {
            routed_gifs,
            destination,
        }
    }

    /// Returns the positive number of GIFs routed by the run.
    pub(crate) fn routed_gifs(&self) -> usize {
        self.routed_gifs.get()
    }

    /// Returns the destination that received the routed GIFs.
    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }
}

/// State-valid facts for an Extraction run that emitted output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProducedOutput {
    output_kind: ExtractionOutputKind,
    emitted_images: NonZeroUsize,
    documents_with_output: NonZeroUsize,
    conversion: Option<ConversionFacts>,
    gif_routing: Option<GifRoutingFacts>,
}

impl ProducedOutput {
    /// Returns whether the emitted files are normal images or required covers.
    pub(crate) fn output_kind(&self) -> ExtractionOutputKind {
        self.output_kind
    }

    /// Returns the positive number of emitted image files.
    pub(crate) fn emitted_images(&self) -> usize {
        self.emitted_images.get()
    }

    /// Returns the positive number of documents that emitted output.
    pub(crate) fn documents_with_output(&self) -> usize {
        self.documents_with_output.get()
    }

    /// Returns conversion facts exactly when conversion was requested.
    pub(crate) fn conversion(&self) -> Option<&ConversionFacts> {
        self.conversion.as_ref()
    }

    /// Returns routing facts exactly when at least one GIF was routed.
    pub(crate) fn gif_routing(&self) -> Option<&GifRoutingFacts> {
        self.gif_routing.as_ref()
    }
}

/// Closed terminal result of one Extraction run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtractionRunOutcome {
    /// Document selection found no eligible documents.
    NoDocuments,
    /// Documents were selected, but no image file was emitted.
    NoOutput(ExtractionOutputKind),
    /// At least one selected document emitted at least one image file.
    ProducedOutput(ProducedOutput),
}

impl ExtractionRunOutcome {
    /// Creates a produced-output outcome when all semantic totals are consistent.
    ///
    /// Positive count types prevent terminal adapters and tests from creating a
    /// produced state with zero output. The remaining checks ensure documents
    /// and classified output facts cannot exceed the emitted-image total.
    pub(crate) fn try_produced(
        output_kind: ExtractionOutputKind,
        emitted_images: NonZeroUsize,
        documents_with_output: NonZeroUsize,
        conversion: Option<ConversionFacts>,
        gif_routing: Option<GifRoutingFacts>,
    ) -> Option<Self> {
        let conversion_total = match conversion {
            Some(facts) => facts
                .converted_images
                .checked_add(facts.skipped_conversions)?,
            None => 0,
        };
        let routed_gifs = gif_routing
            .as_ref()
            .map_or(0, |facts| facts.routed_gifs.get());
        let classified_output = conversion_total.checked_add(routed_gifs)?;
        if documents_with_output.get() > emitted_images.get()
            || classified_output > emitted_images.get()
        {
            return None;
        }

        Some(Self::ProducedOutput(ProducedOutput {
            output_kind,
            emitted_images,
            documents_with_output,
            conversion,
            gif_routing,
        }))
    }
}

#[derive(Default)]
struct ConversionAggregation {
    converted_images: usize,
    skipped_conversions: usize,
}

struct GifRoutingAggregation {
    routed_gifs: usize,
    destination: PathBuf,
}

/// Run-private aggregation that can be consumed only into a valid outcome.
struct RunAggregation {
    emitted_images: usize,
    documents_with_output: usize,
    has_normal_image_output: bool,
    conversion: Option<ConversionAggregation>,
    gif_routing: Option<GifRoutingAggregation>,
}

/// One structured, ordered fact emitted during an Extraction run.
///
/// Observations retain Document selection's structured domain values alongside
/// per-document extraction lifecycle facts and the final semantic outcome. They
/// exclude terminal wording and user-interface commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtractionRunObservation {
    /// One immutable Document selection phase snapshot.
    DocumentSelectionProgress(DocumentSelectionProgress),
    /// One structured, non-fatal Document selection diagnostic.
    DocumentSelectionDiagnostic(DocumentSelectionDiagnostic),
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
    /// The final semantic outcome of the run; no observation follows it.
    Terminal(ExtractionRunOutcome),
}

/// Receives the complete ordered stream of Extraction run observations.
///
/// The observer does not inherit the lower Document selection interface; the
/// run privately translates those callbacks into this cohesive seam and ends
/// every stream with exactly one terminal outcome.
pub(crate) trait ExtractionRunObserver {
    /// Handles one structured observation emitted by the Extraction run.
    fn on_observation(&mut self, observation: ExtractionRunObservation);
}

/// Run-private bridge from Document selection callbacks to run observations.
struct DocumentSelectionObservationAdapter<'observer, Observer> {
    observer: &'observer mut Observer,
}

impl<'observer, Observer> DocumentSelectionObservationAdapter<'observer, Observer> {
    /// Borrows the run observer for the duration of one Document selection call.
    fn new(observer: &'observer mut Observer) -> Self {
        Self { observer }
    }
}

impl<Observer> DocumentSelectionObserver for DocumentSelectionObservationAdapter<'_, Observer>
where
    Observer: ExtractionRunObserver,
{
    /// Preserves one structured progress value in the run observation stream.
    fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
        self.observer
            .on_observation(ExtractionRunObservation::DocumentSelectionProgress(
                progress,
            ));
    }

    /// Preserves one structured diagnostic value in the run observation stream.
    fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        self.observer
            .on_observation(ExtractionRunObservation::DocumentSelectionDiagnostic(
                diagnostic,
            ));
    }
}

/// Executes one Extraction run and returns its semantic outcome directly.
///
/// The owned request is consumed exactly once. Selection diagnostics and
/// document-local failures are emitted through the observer and do not make the
/// run fallible or stop later documents. The final observation carries the same
/// outcome returned by this operation, and nothing is observed afterward.
pub(crate) fn run(
    request: ExtractionRunRequest,
    observer: &mut impl ExtractionRunObserver,
) -> ExtractionRunOutcome {
    let ExtractionRunRequest {
        inputs,
        recursive,
        output,
        epub_filter,
        document_extraction,
        conversion_requested,
        gif_destination,
    } = request;
    let cover_only = document_extraction.is_epub_cover_extraction_configured();
    let selected_documents = {
        let mut selection_observer = DocumentSelectionObservationAdapter::new(observer);
        document_selection::select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive,
                output: output.as_deref(),
                epub_filter: &epub_filter,
            },
            &mut selection_observer,
        )
    };

    if selected_documents.is_empty() {
        let outcome = ExtractionRunOutcome::NoDocuments;
        observer.on_observation(ExtractionRunObservation::Terminal(outcome.clone()));
        return outcome;
    }

    observer.on_observation(ExtractionRunObservation::ExtractionStarted {
        total: selected_documents.len(),
        cover_only,
    });
    let mut aggregation = RunAggregation::new(conversion_requested, gif_destination);

    for selected_document in selected_documents {
        // The run retains only observer-facing facts before transferring the
        // selection-owned handoff into Document extraction exactly once.
        let path = selected_document.get_path().to_path_buf();
        let display_name = selected_document.get_display_name().to_string();
        observer.on_observation(ExtractionRunObservation::DocumentStarted {
            path: path.clone(),
            display_name,
        });

        let (facts, error) = match document_extraction.extract(selected_document) {
            DocumentExtractionOutcome::Completed(facts) => (facts, None),
            DocumentExtractionOutcome::Failed { facts, error } => (facts, Some(error)),
        };
        for warning in facts.get_warnings() {
            observer.on_observation(ExtractionRunObservation::DocumentWarning {
                path: path.clone(),
                message: warning.get_message().to_string(),
            });
        }
        aggregation.record_document_result(&facts);
        if let Some(error) = error {
            observer.on_observation(ExtractionRunObservation::DocumentError {
                path: path.clone(),
                message: error.to_string(),
            });
        }

        observer.on_observation(ExtractionRunObservation::DocumentFinished { path: path.clone() });
    }

    let outcome = aggregation.into_outcome(cover_only);
    observer.on_observation(ExtractionRunObservation::Terminal(outcome.clone()));
    outcome
}

impl RunAggregation {
    /// Starts aggregation with only the facts applicable to the requested workflow.
    fn new(conversion_requested: bool, gif_destination: Option<PathBuf>) -> Self {
        Self {
            emitted_images: 0,
            documents_with_output: 0,
            has_normal_image_output: false,
            conversion: conversion_requested.then(ConversionAggregation::default),
            gif_routing: gif_destination.map(|destination| GifRoutingAggregation {
                routed_gifs: 0,
                destination,
            }),
        }
    }

    /// Records Document extraction facts retained by one completed or failed outcome.
    fn record_document_result(&mut self, facts: &DocumentExtractionFacts) {
        self.emitted_images += facts.get_emitted_images();
        if let Some(conversion) = &mut self.conversion {
            conversion.converted_images += facts.get_converted_images();
            conversion.skipped_conversions += facts.get_skipped_conversions();
        } else {
            debug_assert_eq!(facts.get_converted_images(), 0);
            debug_assert_eq!(facts.get_skipped_conversions(), 0);
        }
        if let Some(gif_routing) = &mut self.gif_routing {
            gif_routing.routed_gifs += facts.get_gifs_routed();
        } else {
            debug_assert_eq!(facts.get_gifs_routed(), 0);
        }

        if facts.get_emitted_images() > 0 {
            self.documents_with_output += 1;
            self.has_normal_image_output |= facts.is_normal_image_output_present();
        }
    }

    /// Consumes aggregate counters into one closed, state-valid semantic outcome.
    fn into_outcome(self, cover_only: bool) -> ExtractionRunOutcome {
        // EPUB fallback and DOCX output are normal images even during a cover-only run.
        // Classify output as covers only when every emitted file used required-cover purpose.
        let output_kind = if cover_only && !self.has_normal_image_output {
            ExtractionOutputKind::Covers
        } else {
            ExtractionOutputKind::Images
        };
        let Some(emitted_images) = NonZeroUsize::new(self.emitted_images) else {
            return ExtractionRunOutcome::NoOutput(output_kind);
        };
        let documents_with_output = NonZeroUsize::new(self.documents_with_output)
            .expect("emitted output must belong to at least one document");
        let conversion = self
            .conversion
            .map(|facts| ConversionFacts::new(facts.converted_images, facts.skipped_conversions));
        let gif_routing = self.gif_routing.and_then(|facts| {
            NonZeroUsize::new(facts.routed_gifs)
                .map(|routed_gifs| GifRoutingFacts::new(routed_gifs, facts.destination))
        });

        ExtractionRunOutcome::try_produced(
            output_kind,
            emitted_images,
            documents_with_output,
            conversion,
            gif_routing,
        )
        .expect("run aggregation must produce consistent semantic totals")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Args;
    use crate::document_selection::{
        DocumentSelectionDiagnostic, DocumentSelectionPhaseStatus, DocumentSelectionProgress,
        DocumentSelectionScanScope,
    };
    use crate::extraction_run_intake;
    use clap::Parser;
    use image::DynamicImage;
    use std::fs;
    use std::io::{Cursor, Write};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

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

    /// Prepares one production request and asserts that no pre-run notice applies.
    fn prepare_request(arguments: Vec<String>) -> ExtractionRunRequest {
        let args = Args::try_parse_from(arguments).expect("test arguments should parse");
        let prepared =
            extraction_run_intake::prepare(args).expect("Extraction run intake should succeed");
        assert!(prepared.notices.is_empty());
        prepared.request
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

    /// Writes an EPUB fixture with declaration-derived identity and an optional image.
    fn write_epub(path: &Path, creator: &str, title: &str, image: Option<(&str, &[u8], bool)>) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("mimetype", options)
            .expect("mimetype entry should start");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype should be writable");
        zip.start_file("META-INF/container.xml", options)
            .expect("container entry should start");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .expect("container should be writable");
        let manifest_item = match image {
            Some((name, _, true)) => format!(
                r#"<item id="image" href="images/{name}" media-type="image/jpeg" properties="cover-image"/>"#
            ),
            Some((name, _, false)) => {
                format!(r#"<item id="image" href="images/{name}" media-type="image/jpeg"/>"#)
            }
            None => String::new(),
        };
        let opf = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:creator>{creator}</dc:creator>
  </metadata>
  <manifest>
    {manifest_item}
  </manifest>
  <spine></spine>
</package>"#
        );
        zip.start_file("OEBPS/content.opf", options)
            .expect("OPF entry should start");
        zip.write_all(opf.as_bytes())
            .expect("OPF should be writable");
        if let Some((name, data, _)) = image {
            zip.start_file(format!("OEBPS/images/{name}"), options)
                .expect("image entry should start");
            zip.write_all(data)
                .expect("image payload should be writable");
        }
        zip.finish().expect("test EPUB should finish");
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
        let temp_dir = temp_test_dir("no-documents");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input = temp_dir.to_string_lossy().into_owned();
        let args =
            Args::try_parse_from(["test", input.as_str()]).expect("test arguments should parse");
        let prepared =
            extraction_run_intake::prepare(args).expect("Extraction run intake should succeed");
        let mut observer = RecordingRunObserver::default();

        let outcome = run(prepared.request, &mut observer);

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
    fn selection_diagnostic_and_completion_precede_extraction_in_one_observation_stream() {
        let temp_dir = temp_test_dir("selection-extraction-boundary");
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
        let temp_dir = temp_test_dir("image-no-output");
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
        let temp_dir = temp_test_dir("cover-no-output");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("empty.epub");
        let output_dir = temp_dir.join("output");
        write_epub(&input_path, "Test Creator", "No Cover", None);

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
        let temp_dir = temp_test_dir("normal-output");
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
        let temp_dir = temp_test_dir("normal-fallback-output");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("fallback.epub");
        let output_dir = temp_dir.join("output");
        write_epub(
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
        let temp_dir = temp_test_dir("zero-conversion-totals");
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
        let temp_dir = temp_test_dir("gif-routing");
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
        let temp_dir = temp_test_dir("document-fact-aggregation");
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
        let temp_dir = temp_test_dir("epub-identity-across-policies");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("filename.epub");
        write_epub(
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
        let warning_messages: Vec<_> = warning_indices
            .iter()
            .map(|index| match &observer.observations[*index] {
                ExtractionRunObservation::DocumentWarning { message, .. } => message.as_str(),
                _ => unreachable!("warning indices must reference warning observations"),
            })
            .collect();
        assert!(warning_messages[0].contains("first.png"));
        assert!(warning_messages[1].contains("second.png"));
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
}
