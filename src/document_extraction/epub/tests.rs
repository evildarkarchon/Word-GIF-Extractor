//! Tests for EPUB file processing.

use super::*;
use crate::document_extraction::DocumentExtractionPolicy;
use crate::document_search_surface::FilesystemSearchSurface;
use crate::document_selection::{
    DocumentSelectionOptions, EpubFilter, SelectedDocument, SelectedEpub, select_documents,
};
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
use crate::test_support::{
    SilentExtractionRunObserver, temp_test_dir, write_epub_with_one_image,
    write_stored_epub_fixture,
};
use std::collections::HashSet;
use std::fs;

const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

/// Obtains one owned EPUB handoff through the production Document selection operation.
fn select_epub(input_path: &Path, output_dir: &Path) -> SelectedEpub {
    let mut observer = SilentExtractionRunObserver;
    let input_path = input_path.to_path_buf();
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&input_path),
            recursive: false,
            output: Some(output_dir),
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    )
    .into_iter()
    .next()
    .expect("EPUB fixture should be selected");

    match selected {
        SelectedDocument::Epub(document) => document,
        SelectedDocument::Docx(_) => panic!("EPUB fixture should retain its selected kind"),
    }
}

/// Returns archive acquisition warning sources in their observed order.
fn acquisition_failure_sources(warnings: &[ImageWriteWarning]) -> Vec<&str> {
    warnings
        .iter()
        .filter_map(|warning| match warning {
            ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, .. } => {
                Some(source_name.as_str())
            }
            _ => None,
        })
        .collect()
}

/// Corrupts one uniquely identifiable stored payload without changing ZIP metadata.
fn corrupt_stored_payload(path: &Path, payload: &[u8]) {
    let mut archive_bytes = fs::read(path).expect("test EPUB should be readable");
    let payload_start = archive_bytes
        .windows(payload.len())
        .position(|window| window == payload)
        .expect("stored test payload should occur in the EPUB");
    assert_eq!(
        archive_bytes
            .windows(payload.len())
            .filter(|window| *window == payload)
            .count(),
        1,
        "test payload must uniquely identify one archive entry"
    );
    archive_bytes[payload_start] ^= 0xff;
    fs::write(path, archive_bytes).expect("corrupted test EPUB should be writable");
}

#[test]
fn declared_cover_resource_is_acquired_and_emitted_as_one_file() {
    let temp_dir = temp_test_dir("epub", "declared-cover-end-to-end");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let cover = b"\xFF\xD8\xFFdeclared-cover";
    // The declared cover deliberately resolves after another resource, so it is not
    // the plan's first entry: EPUB cover extraction addresses candidates by position,
    // and an off-by-one there would acquire the wrong payload rather than fail.
    write_stored_epub_fixture(
        &input_path,
        &[
            ("page", "images/a.png", "image/png", None),
            (
                "metadata-cover",
                "images/b.jpg",
                "image/jpeg",
                Some("cover-image"),
            ),
        ],
        &[
            ("OEBPS/images/a.png", MINIMAL_PNG),
            ("OEBPS/images/b.jpg", cover),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: false,
        },
        &pipeline,
    )
    .expect("a declared cover should be acquired and emitted");

    assert_eq!(result.counts.extracted, 1);
    assert!(!result.has_normal_image_output());
    assert!(result.warnings.is_empty());
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.jpg")).unwrap(),
        cover
    );
    assert_eq!(
        fs::read_dir(&output_dir)
            .expect("output directory should remain readable")
            .count(),
        1
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn extracts_epub_resource_by_magic_before_declared_extension_and_mime() {
    let temp_dir = temp_test_dir("epub", "magic-before-labels");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");

    write_epub_with_one_image(
        &input_path,
        "images/mislabeled.jpg",
        "image/jpeg",
        MINIMAL_PNG,
    );

    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("EPUB extraction should succeed");

    assert_eq!(result.counts.extracted, 1);
    assert!(output_dir.join("Tester - Magic Test.png").exists());
    assert!(!output_dir.join("Tester - Magic Test.jpg").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn extracts_epub_resource_by_magic_without_declared_image_hints() {
    let temp_dir = temp_test_dir("epub", "magic-without-hints");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");

    write_epub_with_one_image(
        &input_path,
        "images/mislabeled.bin",
        "application/octet-stream",
        MINIMAL_PNG,
    );

    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("EPUB extraction should succeed");

    assert_eq!(result.counts.extracted, 1);
    assert!(output_dir.join("Tester - Magic Test.png").exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn missing_manifest_resource_warns_and_later_image_is_extracted() {
    let temp_dir = temp_test_dir("epub", "missing-resource-continues");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            ("missing", "images/a.png", "image/png", None),
            ("valid", "images/b.png", "image/png", None),
        ],
        &[("OEBPS/images/b.png", MINIMAL_PNG)],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("a missing resource should not abort the EPUB");

    assert_eq!(result.counts.extracted, 1);
    assert!(matches!(
        &result.warnings[..],
        [ImageWriteWarning::ArchiveImageAcquisitionFailed {
            source_name,
            detail,
        }] if source_name == "OEBPS/images/a.png"
            && detail == "EPUB resource not found: OEBPS/images/a.png"
    ));
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
        MINIMAL_PNG
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn epub_batch_output_uses_resolved_path_order() {
    let temp_dir = temp_test_dir("epub", "resolved-path-order");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let mut first_by_path = MINIMAL_PNG.to_vec();
    first_by_path.push(1);
    let mut second_by_path = MINIMAL_PNG.to_vec();
    second_by_path.push(2);
    write_stored_epub_fixture(
        &input_path,
        &[
            ("z", "images/z.png", "image/png", None),
            ("a", "images/a.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/z.png", &second_by_path),
            ("OEBPS/images/a.png", &first_by_path),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("EPUB extraction should succeed");

    assert_eq!(result.counts.extracted, 2);
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test_1.png")).unwrap(),
        first_by_path
    );
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test_2.png")).unwrap(),
        second_by_path
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn percent_decoded_resource_sorts_by_resolved_zip_path() {
    let temp_dir = temp_test_dir("epub", "percent-decoded-sort-order");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let mut first_by_resolved_path = MINIMAL_PNG.to_vec();
    first_by_resolved_path.push(1);
    let mut second_by_resolved_path = MINIMAL_PNG.to_vec();
    second_by_resolved_path.push(2);
    write_stored_epub_fixture(
        &input_path,
        &[
            ("escaped-z", "images/%7A.png", "image/png", None),
            ("plain-a", "images/a.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/z.png", &second_by_resolved_path),
            ("OEBPS/images/a.png", &first_by_resolved_path),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("EPUB extraction should succeed");

    assert_eq!(result.counts.extracted, 2);
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test_1.png")).unwrap(),
        first_by_resolved_path
    );
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test_2.png")).unwrap(),
        second_by_resolved_path
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn percent_decoded_manifest_path_falls_back_to_matching_zip_entry() {
    let temp_dir = temp_test_dir("epub", "percent-decoded-resource");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[("image", "images/cover%20art.png", "image/png", None)],
        &[("OEBPS/images/cover art.png", MINIMAL_PNG)],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("percent-decoded lookup should preserve EPUB crate behavior");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
        MINIMAL_PNG
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn exact_manifest_path_wins_before_percent_decoded_alias() {
    let temp_dir = temp_test_dir("epub", "exact-resource-wins");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let mut exact_payload = MINIMAL_PNG.to_vec();
    exact_payload.push(1);
    let mut decoded_payload = MINIMAL_PNG.to_vec();
    decoded_payload.push(2);
    write_stored_epub_fixture(
        &input_path,
        &[("image", "images/cover%20art.png", "image/png", None)],
        &[
            ("OEBPS/images/cover%20art.png", &exact_payload),
            ("OEBPS/images/cover art.png", &decoded_payload),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect("exact ZIP lookup should take precedence");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
        exact_payload
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn archive_open_failure_after_selection_is_a_fatal_extraction_error() {
    let temp_dir = temp_test_dir("epub", "archive-open-failure");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_epub_with_one_image(&input_path, "images/image.png", "image/png", MINIMAL_PNG);
    let selected = select_epub(&input_path, &output_dir);
    fs::remove_file(&input_path).expect("selected EPUB should be removable");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let failure = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect_err("archive-open failure should be fatal");

    assert!(
        failure
            .error
            .to_string()
            .contains("Failed to open input file"),
        "unexpected failure: {}",
        failure.error
    );
    assert_eq!(failure.partial.counts.extracted, 0);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn archive_parse_failure_after_selection_is_a_fatal_extraction_error() {
    let temp_dir = temp_test_dir("epub", "archive-parse-failure");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_epub_with_one_image(&input_path, "images/image.png", "image/png", MINIMAL_PNG);
    let selected = select_epub(&input_path, &output_dir);
    fs::write(&input_path, b"not a ZIP archive").expect("selected EPUB should be replaceable");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Png]),
        None,
        None,
    ));

    let failure = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
        .expect_err("archive-parse failure should be fatal");

    assert!(
        failure
            .error
            .to_string()
            .contains("Failed to read zip archive"),
        "unexpected failure: {}",
        failure.error
    );
    assert_eq!(failure.partial.counts.extracted, 0);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Proves that several distinct manifest spellings resolve to one Archive resource identity.
///
/// `images/cover%2Ejpg`, `images/cover.jpg` and `images/cover%2ejpg` are three declared
/// references to a single archive payload, so exactly one of them is ever acquired and
/// the rest are excluded from normal fallback. EPUB cover extraction's own tests assume
/// aliasing candidates share an identity; only a real archive can establish that they do.
#[test]
fn aliasing_manifest_spellings_resolve_to_one_archive_identity() {
    let temp_dir = temp_test_dir("epub", "aliasing-cover-identity");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let corrupt_cover = b"uniquely corrupt cover payload";
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/cover%2Ejpg",
                "image/jpeg",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("non-cover-alias", "images/cover%2ejpg", "image/jpeg", None),
            ("page", "images/page.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/cover.jpg", corrupt_cover),
            ("OEBPS/images/page.png", MINIMAL_PNG),
        ],
    );
    corrupt_stored_payload(&input_path, corrupt_cover);
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect("one failed resolved cover should allow batch fallback");

    // One acquisition warning for three spellings is the observable form of one identity:
    // the two aliases were neither attempted again nor revisited by normal fallback.
    assert_eq!(
        acquisition_failure_sources(&result.warnings),
        vec!["OEBPS/images/cover%2Ejpg"]
    );
    assert_eq!(result.counts.extracted, 1);
    assert!(result.has_normal_image_output());
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
        MINIMAL_PNG
    );
    assert_eq!(
        fs::read_dir(&output_dir)
            .expect("normal output directory should remain readable")
            .count(),
        1
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
