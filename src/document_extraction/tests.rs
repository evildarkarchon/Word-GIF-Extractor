//! Tests for per-document extraction policy, dispatch, and outcomes.

use super::*;
use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
use crate::document_search_surface::FilesystemSearchSurface;
use crate::document_selection::{DocumentSelectionOptions, EpubFilter, select_documents};
use crate::epub_declarations::EpubFileDeclarations;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};
use crate::test_support::{
    SilentExtractionRunObserver, pipeline_accepting, temp_test_dir, write_docx, write_epub_fixture,
    write_epub_image, write_epub_with_resources,
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Constructs a production Conversion policy for operation-level fixtures.
fn conversion_policy(target: ConversionTarget) -> ConversionPolicy {
    ConversionPolicy::try_from(ConversionRequest {
        target,
        quality: None,
        lossless: false,
    })
    .expect("test conversion policy should be valid")
}

/// Returns stable warning bodies in their retained Document extraction order.
fn warning_messages(facts: &DocumentExtractionFacts) -> Vec<&str> {
    facts
        .get_warnings()
        .iter()
        .map(DocumentExtractionWarning::get_message)
        .collect()
}

/// Returns the document's normal-image and cover totals, which together say what its output was for.
///
/// `(n, 0)` with `n > 0` is output that included normal images, `(0, n)` is
/// covers only, and `(0, 0)` is nothing emitted (ADR-0017).
fn normal_images_and_covers(facts: &DocumentExtractionFacts) -> (usize, usize) {
    let tally = facts.get_tally();
    (tally.normal_images(), tally.covers())
}

/// Obtains one extraction handoff through the production Document selection operation.
fn select_one_document(input_path: &Path, output_dir: &Path) -> SelectedDocument {
    let mut observer = SilentExtractionRunObserver;
    let input_path = input_path.to_path_buf();
    select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&input_path),
            recursive: false,
            output: Some(output_dir),
            epub_filter: &EpubFilter::default(),
            epub_only: false,
        },
        &FilesystemSearchSurface,
        &EpubFileDeclarations,
        &mut observer,
    )
    .into_iter()
    .next()
    .expect("document fixture should be selected")
}

/// Verifies the cover intent and Applicable outcome facts Document extraction reports from its policies.
///
/// Pins both report methods, including the GIF destination value, in a cover-only
/// run with conversion and GIF routing and in a run with no image flags. These
/// assertions were checked only through the Extraction run tests' scripted adapter,
/// which forwarded both facts from a wrapped real Document extraction; this test
/// replaces that run-level forwarding check (ADR-0018). Nothing is written to disk.
#[test]
fn reports_cover_intent_and_applicable_outcome_facts_from_its_policies() {
    let cover_only_extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePipeline::new(ImageWritePolicy::new(
            ImageFormat::all_set(),
            Some(conversion_policy(ConversionTarget::Jpg)),
            Some(PathBuf::from("gifs")),
        )),
    );

    assert!(cover_only_extraction.is_epub_cover_extraction_configured());
    let applicable = cover_only_extraction.applicable_outcome_facts();
    assert!(applicable.is_conversion_applicable());
    assert_eq!(
        applicable.into_gif_destination(),
        Some(PathBuf::from("gifs"))
    );

    let normal_images_extraction =
        DocumentExtraction::new(None, pipeline_accepting(ImageFormat::all_set()));

    assert!(!normal_images_extraction.is_epub_cover_extraction_configured());
    let applicable = normal_images_extraction.applicable_outcome_facts();
    assert!(!applicable.is_conversion_applicable());
    assert_eq!(applicable.into_gif_destination(), None);
}

// `docx_uses_normal_images_when_policy_requests_an_epub_cover` lived here. It
// handed a cover policy to a DOCX and asserted the policy was dropped; that is
// now unconstructible, because a cover policy only reaches the EPUB arm. The
// user-visible half of its claim — a DOCX in a cover-only run still emits its
// normal images — moved up to `extraction_run::tests`, where a run can hold
// both document kinds at once.

#[test]
fn failed_extraction_retains_document_extraction_facts() {
    let temp_dir = temp_test_dir("document-extraction", "partial-failure");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("output");
    let blocked_gif_output = temp_dir.join("blocked-gifs");
    fs::write(&blocked_gif_output, b"not a directory")
        .expect("blocked GIF destination should be creatable");
    write_docx(
        &input_path,
        &[
            ("word/media/first.png", b"not actually a png"),
            ("word/media/second.gif", b"GIF89a"),
        ],
    );

    let extraction = DocumentExtraction::new(
        None,
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png, ImageFormat::Gif]),
            None,
            Some(blocked_gif_output),
        )),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Failed { facts, error } = extraction.extract(document) else {
        panic!("blocked GIF destination should fail Document extraction");
    };

    assert_eq!(normal_images_and_covers(&facts), (1, 0));
    assert_eq!(
        facts
            .get_warnings()
            .iter()
            .map(DocumentExtractionWarning::get_message)
            .collect::<Vec<_>>(),
        vec!["Magic detection failed for word/media/first.png; falling back to .png extension"]
    );
    assert!(
        error
            .to_string()
            .contains("Failed to create output directory")
    );
    assert!(output_dir.join("sample_1.png").exists());
}

#[test]
fn docx_warning_bodies_keep_source_format_base_name_detail_multiplicity_and_phase_order() {
    let temp_dir = temp_test_dir("document-extraction", "docx-warning-bodies");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("output");
    let corrupt_png = b"\x89PNG\r\n\x1A\nnot an image at all";
    write_docx(
        &input_path,
        &[
            ("word/media/extension.png", b"extension-only payload"),
            ("word/media/corrupt-one.png", corrupt_png),
            ("word/media/corrupt-two.png", corrupt_png),
            (
                "word/media/vector.svg",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
            ),
        ],
    );
    let extraction = DocumentExtraction::new(
        None,
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png, ImageFormat::Svg]),
            Some(conversion_policy(ConversionTarget::Jpg)),
            None,
        )),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("warning-producing DOCX extraction should complete");
    };

    let tally = facts.get_tally();
    assert_eq!(tally.emitted(), 4);
    assert_eq!(tally.converted(), 0);
    assert_eq!(tally.conversion_skipped(), 4);
    assert_eq!(
        warning_messages(&facts),
        vec![
            "Magic detection failed for word/media/extension.png; falling back to .png extension",
            "Skipping conversion for sample (png format not supported for conversion)",
            "Conversion failed for image in sample: Failed to decode image",
            "Conversion failed for image in sample: Failed to decode image",
            "Skipping conversion for sample (svg format not supported for conversion)",
        ]
    );
}

#[test]
fn epub_cover_warning_bodies_keep_declared_mime_and_filtered_format() {
    let temp_dir = temp_test_dir("document-extraction", "epub-cover-discovery-warning-bodies");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

    let unidentified_path = temp_dir.join("unidentified.epub");
    let unidentified_output = temp_dir.join("unidentified-output");
    write_epub_fixture(
        &unidentified_path,
        &[(
            "cover",
            "images/art.bin",
            "application/x-cover-art",
            Some("cover-image"),
        )],
        &[("OEBPS/images/art.bin", b"unidentified cover bytes")],
    );
    let unidentified_extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let unidentified = select_one_document(&unidentified_path, &unidentified_output);
    let DocumentExtractionOutcome::Completed(unidentified_facts) =
        unidentified_extraction.extract(unidentified)
    else {
        panic!("unidentified required cover should default and complete");
    };
    assert_eq!(
        warning_messages(&unidentified_facts),
        vec![
            "Cover image MIME 'application/x-cover-art' could not be identified; defaulting to .jpg extension."
        ]
    );
    assert_eq!(normal_images_and_covers(&unidentified_facts), (0, 1));

    let filtered_path = temp_dir.join("filtered.epub");
    let filtered_output = temp_dir.join("filtered-output");
    write_epub_fixture(
        &filtered_path,
        &[("cover", "images/art.png", "image/png", Some("cover-image"))],
        &[("OEBPS/images/art.png", b"\x89PNG\r\n\x1A\n")],
    );
    let filtered_extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let filtered = select_one_document(&filtered_path, &filtered_output);
    let DocumentExtractionOutcome::Completed(filtered_facts) =
        filtered_extraction.extract(filtered)
    else {
        panic!("filtered required cover should complete without emission");
    };
    assert_eq!(
        warning_messages(&filtered_facts),
        vec!["Cover image format 'png' not in allowed formats, skipping."]
    );
    assert_eq!(normal_images_and_covers(&filtered_facts), (0, 0));
}

#[test]
fn epub_cover_conversion_warning_bodies_keep_format_and_lower_error_detail() {
    let temp_dir = temp_test_dir(
        "document-extraction",
        "epub-cover-conversion-warning-bodies",
    );
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

    let unsupported_path = temp_dir.join("unsupported.epub");
    let unsupported_output = temp_dir.join("unsupported-output");
    write_epub_fixture(
        &unsupported_path,
        &[(
            "cover",
            "images/art.svg",
            "image/svg+xml",
            Some("cover-image"),
        )],
        &[(
            "OEBPS/images/art.svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
        )],
    );
    let unsupported_extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Svg]),
            Some(conversion_policy(ConversionTarget::Jpg)),
            None,
        )),
    );
    let unsupported = select_one_document(&unsupported_path, &unsupported_output);
    let DocumentExtractionOutcome::Completed(unsupported_facts) =
        unsupported_extraction.extract(unsupported)
    else {
        panic!("unsupported required-cover conversion should complete");
    };
    assert_eq!(
        warning_messages(&unsupported_facts),
        vec!["Cover image format 'svg' not supported for conversion, skipping cover."]
    );
    assert_eq!(normal_images_and_covers(&unsupported_facts), (0, 0));

    let failed_path = temp_dir.join("failed.epub");
    let failed_output = temp_dir.join("failed-output");
    write_epub_fixture(
        &failed_path,
        &[("cover", "images/art.png", "image/png", Some("cover-image"))],
        &[(
            "OEBPS/images/art.png",
            b"\x89PNG\r\n\x1A\nnot an image at all",
        )],
    );
    let failed_extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            Some(conversion_policy(ConversionTarget::Jpg)),
            None,
        )),
    );
    let failed = select_one_document(&failed_path, &failed_output);
    let DocumentExtractionOutcome::Completed(failed_facts) = failed_extraction.extract(failed)
    else {
        panic!("failed required-cover conversion should be non-fatal");
    };
    assert_eq!(
        warning_messages(&failed_facts),
        vec!["Cover conversion failed: Failed to decode image"]
    );
    assert_eq!(normal_images_and_covers(&failed_facts), (0, 0));
}

#[test]
fn epub_cover_retry_warning_bodies_precede_filename_retry_and_normal_fallback() {
    let temp_dir = temp_test_dir("document-extraction", "epub-cover-retry-warning-bodies");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    write_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/missing.png",
                "image/png",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/page.png", "image/png", None),
        ],
        &[("OEBPS/images/page.png", b"extension-only page")],
    );
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverThenNormalImages),
        pipeline_accepting([ImageFormat::Jpg, ImageFormat::Png]),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("unreadable cover candidates should allow normal-image fallback");
    };

    assert_eq!(normal_images_and_covers(&facts), (1, 0));
    assert_eq!(
        warning_messages(&facts),
        vec![
            "Could not read archive resource 'OEBPS/images/missing.png': EPUB resource not found: OEBPS/images/missing.png",
            "Could not read archive resource 'OEBPS/images/cover.jpg': EPUB resource not found: OEBPS/images/cover.jpg",
            "Magic detection failed for OEBPS/images/page.png; falling back to .png extension",
        ]
    );
}

#[test]
fn epub_cover_output_is_classified_as_covers_only() {
    let temp_dir = temp_test_dir("document-extraction", "epub-cover-purpose");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    write_epub_image(
        &input_path,
        "images/art.jpg",
        Some("cover-image"),
        b"\xFF\xD8\xFF",
    );
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
        panic!("valid EPUB cover extraction should complete");
    };

    assert_eq!(normal_images_and_covers(&result), (0, 1));
    assert!(output_dir.join("Test.jpg").exists());
}

#[test]
fn epub_cover_fallback_is_classified_as_normal_images() {
    let temp_dir = temp_test_dir("document-extraction", "epub-fallback-purpose");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    write_epub_image(&input_path, "images/interior.jpg", None, b"\xFF\xD8\xFF");
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverThenNormalImages),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
        panic!("EPUB cover fallback should complete");
    };

    assert_eq!(normal_images_and_covers(&result), (1, 0));
    assert!(output_dir.join("Test.jpg").exists());
}

#[test]
fn normal_policy_extracts_epub_images_through_document_extraction() {
    let temp_dir = temp_test_dir("document-extraction", "epub-normal-images");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    write_epub_image(
        &input_path,
        "images/interior.jpg",
        Some("cover-image"),
        b"\xFF\xD8\xFF",
    );
    let extraction = DocumentExtraction::new(None, pipeline_accepting([ImageFormat::Jpg]));
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
        panic!("normal EPUB extraction should complete");
    };

    assert_eq!(normal_images_and_covers(&result), (1, 0));
    assert!(output_dir.join("Test.jpg").exists());
}

#[test]
fn retained_epub_declarations_are_authoritative_during_extraction() {
    let temp_dir = temp_test_dir("document-extraction", "retained-epub-declarations");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    let selected_payload = b"\xFF\xD8\xFFselected";
    let replacement_payload = b"\xFF\xD8\xFFreplacement";
    let archive_resources = [
        ("OEBPS/images/selected.jpg", selected_payload.as_slice()),
        (
            "OEBPS/images/replacement.jpg",
            replacement_payload.as_slice(),
        ),
    ];
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_epub_with_resources(&input_path, "images/selected.jpg", None, &archive_resources);
    let mut observer = SilentExtractionRunObserver;
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&input_path),
            recursive: false,
            output: Some(&output_dir),
            epub_filter: &EpubFilter::default(),
            epub_only: false,
        },
        &FilesystemSearchSurface,
        &EpubFileDeclarations,
        &mut observer,
    );
    assert_eq!(selected.len(), 1);

    write_epub_with_resources(
        &input_path,
        "images/replacement.jpg",
        None,
        &archive_resources,
    );
    let extraction = DocumentExtraction::new(None, pipeline_accepting([ImageFormat::Jpg]));

    let document = selected
        .into_iter()
        .next()
        .expect("EPUB fixture should be selected");
    let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
        panic!("retained EPUB declarations should support extraction");
    };

    assert_eq!(normal_images_and_covers(&result), (1, 0));
    assert_eq!(
        fs::read(output_dir.join("Test.jpg")).expect("selected image should be readable"),
        selected_payload
    );
}

#[test]
fn selection_declaration_failure_is_retried_without_revising_selected_identity() {
    let temp_dir = temp_test_dir("document-extraction", "retry-epub-declarations");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    fs::write(&input_path, b"not an EPUB").expect("invalid EPUB should be writable");
    let mut observer = SilentExtractionRunObserver;
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&input_path),
            recursive: false,
            output: Some(&output_dir),
            epub_filter: &EpubFilter::default(),
            epub_only: false,
        },
        &FilesystemSearchSurface,
        &EpubFileDeclarations,
        &mut observer,
    );
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_display_name(), "sample.epub");

    write_epub_image(
        &input_path,
        "images/recovered.jpg",
        None,
        b"\xFF\xD8\xFFrecovered",
    );
    let extraction = DocumentExtraction::new(None, pipeline_accepting([ImageFormat::Jpg]));

    let document = selected
        .into_iter()
        .next()
        .expect("EPUB fixture should be selected");
    let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
        panic!("Document extraction should retry unavailable EPUB declarations");
    };

    assert_eq!(normal_images_and_covers(&result), (1, 0));
    assert_eq!(
        fs::read(output_dir.join("sample.jpg")).expect("recovered image should be readable"),
        b"\xFF\xD8\xFFrecovered"
    );
}

/// Verifies a fabricated error keeps its contextual source chain, as an extracted one does.
#[test]
fn fabricated_error_preserves_its_source_chain() {
    let error = DocumentExtractionError::fabricated(
        anyhow::anyhow!("archive vanished").context("Failed to open document"),
    );

    assert_eq!(error.to_string(), "Failed to open document");
    let chain = std::iter::successors(Some(&error as &dyn std::error::Error), |error| {
        error.source()
    })
    .map(ToString::to_string)
    .collect::<Vec<_>>();
    assert!(
        chain.iter().any(|message| message == "archive vanished"),
        "the underlying cause should stay reachable: {chain:?}"
    );
}
