//! Tests for per-document extraction policy, dispatch, and outcomes.

use super::*;
use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
use crate::document_search_surface::FilesystemSearchSurface;
use crate::document_selection::{DocumentSelectionOptions, EpubFilter, select_documents};
use crate::epub_declarations::EpubFileDeclarations;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};
use crate::test_support::{
    SilentExtractionRunObserver, file_names_in, pipeline_accepting, temp_test_dir, write_docx,
    write_epub_fixture, write_epub_image, write_epub_with_resources,
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

/// A payload whose magic bytes identify it as a PNG but which cannot be decoded, so
/// it passes Archive image discovery and then fails conversion.
const CORRUPT_PNG: &[u8] = b"\x89PNG\r\n\x1A\nnot an image at all";

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
fn docx_warning_bodies_keep_source_format_detail_multiplicity_and_phase_order() {
    let temp_dir = temp_test_dir("document-extraction", "docx-warning-bodies");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("output");
    write_docx(
        &input_path,
        &[
            ("word/media/extension.png", b"extension-only payload"),
            ("word/media/corrupt-one.png", CORRUPT_PNG),
            ("word/media/corrupt-two.png", CORRUPT_PNG),
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
            "Skipping conversion for word/media/extension.png (png format not supported for conversion)",
            "Conversion failed for word/media/corrupt-one.png: Failed to decode image",
            "Conversion failed for word/media/corrupt-two.png: Failed to decode image",
            "Skipping conversion for word/media/vector.svg (svg format not supported for conversion)",
        ]
    );
}

/// Verifies that two EPUB normal images that both fail conversion give two warnings
/// that tell the images apart, because each names its source image rather than the
/// document both share.
#[test]
fn epub_normal_image_conversion_warnings_name_each_source_image() {
    let temp_dir = temp_test_dir("document-extraction", "epub-conversion-warning-sources");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    write_epub_fixture(
        &input_path,
        &[
            ("first", "images/first.png", "image/png", None),
            ("second", "images/second.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/first.png", CORRUPT_PNG),
            ("OEBPS/images/second.png", CORRUPT_PNG),
        ],
    );
    let extraction = DocumentExtraction::new(
        None,
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            Some(conversion_policy(ConversionTarget::Jpg)),
            None,
        )),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("EPUB extraction with failing conversions should complete");
    };

    assert_eq!(facts.get_tally().conversion_skipped(), 2);
    assert_eq!(
        warning_messages(&facts),
        vec![
            "Conversion failed for OEBPS/images/first.png: Failed to decode image",
            "Conversion failed for OEBPS/images/second.png: Failed to decode image",
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

/// Verifies that a declared cover filtered out by the allowed formats writes nothing
/// and so hands over to the filename cover, which is written as the EPUB's one cover.
///
/// The filtered cover's warning is kept and comes before the filename cover's facts,
/// because a usable cover means a written cover (ADR-0023).
#[test]
fn epub_filtered_declared_cover_hands_over_to_the_filename_cover() {
    let temp_dir = temp_test_dir("document-extraction", "epub-filtered-cover-hands-over");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    let filename_cover: &[u8] = b"\xFF\xD8\xFFfilename cover";
    write_epub_fixture(
        &input_path,
        &[
            ("cover", "images/art.png", "image/png", Some("cover-image")),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/images/art.png", b"\x89PNG\r\n\x1A\n"),
            ("OEBPS/images/cover.jpg", filename_cover),
        ],
    );
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("a filtered declared cover should hand over and complete");
    };

    assert_eq!(
        warning_messages(&facts),
        vec!["Cover image format 'png' not in allowed formats, skipping."]
    );
    assert_eq!(normal_images_and_covers(&facts), (0, 1));
    assert_eq!(file_names_in(&output_dir), ["Test.jpg"]);
    assert_eq!(
        fs::read(output_dir.join("Test.jpg")).expect("written cover should be readable"),
        filename_cover
    );
}

/// Writes an EPUB whose two cover candidates are both filtered out under a JPEG-only
/// run, beside one JPEG normal image.
///
/// Both candidates are magic-less payloads at `.jpg` paths declared `image/png`. A
/// required cover never takes evidence from its path, so each is a PNG cover and is
/// filtered; as a normal image the `.jpg` extension would identify it and it would be
/// written. That difference is what makes the fallback's identity exclusion visible.
fn write_epub_with_two_filtered_covers(input_path: &Path) {
    write_epub_fixture(
        input_path,
        &[
            ("cover", "images/art.jpg", "image/png", Some("cover-image")),
            ("filename-cover", "images/cover.jpg", "image/png", None),
            ("page", "images/page.jpg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/images/art.jpg", b"declared cover without magic"),
            ("OEBPS/images/cover.jpg", b"filename cover without magic"),
            ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
        ],
    );
}

/// The warning each filtered PNG cover candidate leaves behind.
const FILTERED_PNG_COVER_WARNING: &str =
    "Cover image format 'png' not in allowed formats, skipping.";

/// Verifies that when every cover candidate writes nothing, `CoverThenNormalImages`
/// falls back to normal images, excluding every attempted cover payload.
///
/// Both covers are filtered, so neither was a usable cover (ADR-0023). Writing either
/// again as a normal image would add a file and an extension-fallback warning.
#[test]
fn epub_cover_fallback_runs_when_no_candidate_writes_a_cover() {
    let temp_dir = temp_test_dir("document-extraction", "epub-no-written-cover-falls-back");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    write_epub_with_two_filtered_covers(&input_path);
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverThenNormalImages),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("filtered cover candidates should fall back and complete");
    };

    assert_eq!(
        warning_messages(&facts),
        vec![FILTERED_PNG_COVER_WARNING, FILTERED_PNG_COVER_WARNING]
    );
    assert_eq!(normal_images_and_covers(&facts), (1, 0));
    assert_eq!(file_names_in(&output_dir), ["Test.jpg"]);
    assert_eq!(
        fs::read(output_dir.join("Test.jpg")).expect("written page should be readable"),
        b"\xFF\xD8\xFFpage"
    );
}

/// Verifies that when every cover candidate writes nothing under `CoverOnly`, nothing
/// is written and each passed-over candidate's warning explains why.
#[test]
fn epub_cover_only_writes_nothing_when_no_candidate_writes_a_cover() {
    let temp_dir = temp_test_dir("document-extraction", "epub-no-written-cover-cover-only");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    // Created up front so an empty listing is evidence nothing was written.
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_epub_with_two_filtered_covers(&input_path);
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        pipeline_accepting([ImageFormat::Jpg]),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("filtered cover candidates should complete without output");
    };

    assert_eq!(
        warning_messages(&facts),
        vec![FILTERED_PNG_COVER_WARNING, FILTERED_PNG_COVER_WARNING]
    );
    assert_eq!(normal_images_and_covers(&facts), (0, 0));
    assert!(file_names_in(&output_dir).is_empty());
}

/// Verifies that a declared GIF cover routed to the GIF destination is a written
/// cover, so it ends the search before the filename candidate and fallback.
///
/// Routing writes nothing into the output directory; counting only files there would
/// have handed over to `cover.jpg` and produced a second cover.
#[test]
fn epub_routed_gif_cover_ends_the_search_before_the_filename_candidate_and_fallback() {
    let temp_dir = temp_test_dir("document-extraction", "epub-routed-gif-cover-is-written");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("output");
    let gif_dir = temp_dir.join("gifs");
    // Created up front so an empty listing is evidence nothing was written there.
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_epub_fixture(
        &input_path,
        &[
            ("cover", "images/art.gif", "image/gif", Some("cover-image")),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/page.jpg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/images/art.gif", b"GIF89a"),
            ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename cover"),
            ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
        ],
    );
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverThenNormalImages),
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Gif, ImageFormat::Jpg]),
            None,
            Some(gif_dir.clone()),
        )),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("a routed GIF cover should complete");
    };

    assert!(warning_messages(&facts).is_empty());
    assert_eq!(normal_images_and_covers(&facts), (0, 1));
    assert_eq!(facts.get_tally().gifs_routed(), 1);
    assert_eq!(file_names_in(&gif_dir), ["Test.gif"]);
    assert!(
        file_names_in(&output_dir).is_empty(),
        "neither the filename cover nor the fallback page should be written"
    );
}

/// Verifies that a cover whose conversion is unsupported is written in its original
/// format and bytes, counted as one conversion-skipped cover, with the same
/// conversion-skipped warning a normal image gets, naming the cover's source image.
///
/// Rewritten from the old rule, under which an unsupported cover conversion wrote
/// nothing and warned "…skipping cover." (ADR-0023).
#[test]
fn epub_unconvertible_cover_is_written_in_its_original_format_as_a_conversion_skipped_cover() {
    let temp_dir = temp_test_dir("document-extraction", "epub-unconvertible-cover");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("unsupported.epub");
    let output_dir = temp_dir.join("output");
    let svg: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    write_epub_fixture(
        &input_path,
        &[(
            "cover",
            "images/art.svg",
            "image/svg+xml",
            Some("cover-image"),
        )],
        &[("OEBPS/images/art.svg", svg)],
    );
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Svg]),
            Some(conversion_policy(ConversionTarget::Jpg)),
            None,
        )),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("unsupported required-cover conversion should complete");
    };

    assert_eq!(
        warning_messages(&facts),
        vec![
            "Skipping conversion for OEBPS/images/art.svg (svg format not supported for conversion)"
        ]
    );
    assert_eq!(normal_images_and_covers(&facts), (0, 1));
    assert_eq!(facts.get_tally().conversion_skipped(), 1);
    assert_eq!(facts.get_tally().converted(), 0);
    assert_eq!(file_names_in(&output_dir), ["Test.svg"]);
    assert_eq!(
        fs::read(output_dir.join("Test.svg")).expect("written cover should be readable"),
        svg
    );
}

/// Verifies that a cover whose payload cannot be decoded is written in its original
/// bytes, counted as one conversion-skipped cover, and warned about with the same
/// conversion-failed warning a normal image gets, naming the cover's source image.
///
/// Rewritten from the old rule, under which a failed cover conversion wrote nothing
/// and warned "Cover conversion failed: …" (ADR-0023).
#[test]
fn epub_undecodable_cover_is_written_in_its_original_bytes_and_its_warning_names_the_source() {
    let temp_dir = temp_test_dir("document-extraction", "epub-undecodable-cover");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let input_path = temp_dir.join("failed.epub");
    let output_dir = temp_dir.join("output");
    write_epub_fixture(
        &input_path,
        &[("cover", "images/art.png", "image/png", Some("cover-image"))],
        &[("OEBPS/images/art.png", CORRUPT_PNG)],
    );
    let extraction = DocumentExtraction::new(
        Some(EpubCoverPolicy::CoverOnly),
        ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            Some(conversion_policy(ConversionTarget::Jpg)),
            None,
        )),
    );
    let document = select_one_document(&input_path, &output_dir);

    let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
        panic!("failed required-cover conversion should be non-fatal");
    };

    assert_eq!(
        warning_messages(&facts),
        vec!["Conversion failed for OEBPS/images/art.png: Failed to decode image"]
    );
    assert_eq!(normal_images_and_covers(&facts), (0, 1));
    assert_eq!(facts.get_tally().conversion_skipped(), 1);
    assert_eq!(file_names_in(&output_dir), ["Test.png"]);
    assert_eq!(
        fs::read(output_dir.join("Test.png")).expect("written cover should be readable"),
        CORRUPT_PNG
    );
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
