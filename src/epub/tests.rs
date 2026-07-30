//! Tests for EPUB file processing.

use super::*;
use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
use crate::document_extraction::DocumentExtractionPolicy;
use crate::document_selection::{
    DocumentSelectionOptions, EpubFilter, SelectedDocument, SelectedEpub, select_documents,
};
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
use crate::test_support::{
    SilentDocumentSelectionObserver, temp_test_dir, write_epub_with_one_image,
    write_stored_epub_fixture,
};
use std::collections::HashSet;
use std::fs;
use std::io::Cursor;

const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

/// Obtains one owned EPUB handoff through the production Document selection operation.
fn select_epub(input_path: &Path, output_dir: &Path) -> SelectedEpub {
    let mut observer = SilentDocumentSelectionObserver;
    let input_path = input_path.to_path_buf();
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&input_path),
            recursive: false,
            output: Some(output_dir),
            epub_filter: &EpubFilter::default(),
        },
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

/// Builds the production Image write pipeline with one validated Conversion policy.
fn pipeline_with_conversion(
    allowed_formats: HashSet<ImageFormat>,
    target: ConversionTarget,
) -> ImageWritePipeline {
    let conversion = ConversionPolicy::try_from(ConversionRequest {
        target,
        quality: None,
        lossless: false,
    })
    .expect("test Conversion policy should be valid");
    ImageWritePipeline::new(ImageWritePolicy::new(
        allowed_formats,
        Some(conversion),
        None,
    ))
}

/// Encodes one valid pixel so conversion-focused EPUB tests use production decoders.
fn encoded_test_png() -> Vec<u8> {
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .expect("test PNG should encode");
    encoded.into_inner()
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
fn filename_cover_heuristics_are_observable_through_epub_extraction() {
    for (extension, should_match) in [
        ("jpg", true),
        ("jpeg", true),
        ("jpe", true),
        ("jfif", true),
        ("png", false),
        ("gif", false),
        ("webp", false),
        ("bmp", false),
    ] {
        let temp_dir = temp_test_dir("epub", &format!("filename-cover-{extension}"));
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let manifest_path = format!("images/CoVeR.{extension}");
        let archive_path = format!("OEBPS/{manifest_path}");
        write_stored_epub_fixture(
            &input_path,
            &[(
                "filename-candidate",
                manifest_path.as_str(),
                "application/octet-stream",
                None,
            )],
            &[(archive_path.as_str(), b"\xFF\xD8\xFFcover")],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
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
        .expect("filename cover discovery should complete normally");

        assert_eq!(
            result.counts.extracted,
            usize::from(should_match),
            "unexpected filename-cover decision for .{extension}"
        );
        assert_eq!(
            output_dir.join("Tester - Magic Test.jpg").exists(),
            should_match,
            "unexpected filename-cover output for .{extension}"
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}

#[test]
fn filename_cover_uses_first_deterministic_jpeg_family_candidate() {
    let temp_dir = temp_test_dir("epub", "deterministic-filename-cover");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let first_cover = b"\xFF\xD8\xFFfirst";
    let later_cover = b"\xFF\xD8\xFFlater";
    write_stored_epub_fixture(
        &input_path,
        &[
            ("later", "z/cover.jpg", "image/jpeg", None),
            ("first", "a/cover.jpeg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/z/cover.jpg", later_cover),
            ("OEBPS/a/cover.jpeg", first_cover),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg]),
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
    .expect("the first deterministic filename candidate should be emitted");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.jpg")).unwrap(),
        first_cover
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

#[test]
fn filtered_metadata_cover_is_terminal_before_filename_and_normal_fallback() {
    let temp_dir = temp_test_dir("epub", "filtered-cover-terminal");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/art.png",
                "image/png",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/page.jpg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/images/art.png", MINIMAL_PNG),
            ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
            ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg]),
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
    .expect("a filtered cover should complete without another candidate or fallback");

    assert_eq!(result.counts.extracted, 0);
    assert!(!result.has_normal_image_output());
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::UnsupportedCoverFormat {
            format: ImageFormat::Png,
        }]
    );
    assert!(
        fs::read_dir(&output_dir)
            .expect("output directory should remain readable")
            .next()
            .is_none()
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn unsupported_cover_conversion_is_terminal_before_filename_and_normal_fallback() {
    let temp_dir = temp_test_dir("epub", "unsupported-cover-conversion-terminal");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/art.svg",
                "image/svg+xml",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/page.jpg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/images/art.svg", b"<svg/>"),
            ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
            ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
        ],
    );
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Svg, ImageFormat::Jpg]),
        ConversionTarget::Png,
    );
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect("an unsupported cover conversion should be a terminal decision");

    assert_eq!(result.counts.extracted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(!result.has_normal_image_output());
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::CoverConversionSkipped {
            format: ImageFormat::Svg,
        }]
    );
    assert!(
        fs::read_dir(&output_dir)
            .expect("output directory should remain readable")
            .next()
            .is_none()
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn failed_cover_conversion_is_terminal_before_filename_and_normal_fallback() {
    let temp_dir = temp_test_dir("epub", "failed-cover-conversion-terminal");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/art.png",
                "image/png",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/page.jpg", "image/jpeg", None),
        ],
        &[
            ("OEBPS/images/art.png", b"\x89PNG\r\n\x1A\ninvalid"),
            ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
            ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
        ],
    );
    let pipeline = pipeline_with_conversion(
        HashSet::from([ImageFormat::Png, ImageFormat::Jpg]),
        ConversionTarget::Jpg,
    );
    let selected = select_epub(&input_path, &output_dir);

    let result = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect("a failed cover conversion should be a terminal decision");

    assert_eq!(result.counts.extracted, 0);
    assert_eq!(result.counts.skipped, 0);
    assert!(!result.has_normal_image_output());
    assert!(matches!(
        &result.warnings[..],
        [ImageWriteWarning::CoverConversionFailed { detail }]
            if detail == "Failed to decode image"
    ));
    assert!(
        fs::read_dir(&output_dir)
            .expect("output directory should remain readable")
            .next()
            .is_none()
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn unreadable_metadata_cover_warns_then_filename_cover_succeeds() {
    let temp_dir = temp_test_dir("epub", "unreadable-cover-fallback");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let cover = b"\xFF\xD8\xFFcover";
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/missing.png",
                "image/png",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
        ],
        &[("OEBPS/images/cover.jpg", cover)],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg]),
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
    .expect("an unreadable metadata cover should allow filename fallback");

    assert_eq!(result.counts.extracted, 1);
    assert!(!result.has_normal_image_output());
    assert!(matches!(
        &result.warnings[..],
        [ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, .. }]
            if source_name == "OEBPS/images/missing.png"
    ));
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.jpg")).unwrap(),
        cover
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn cover_retry_warnings_precede_normal_fallback_warning() {
    let temp_dir = temp_test_dir("epub", "unreadable-covers-batch-fallback");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let extension_only_page = b"extension-only-page";
    write_stored_epub_fixture(
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
        &[("OEBPS/images/page.png", extension_only_page)],
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
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect("unreadable cover candidates should allow batch fallback");

    assert_eq!(result.counts.extracted, 1);
    assert!(result.has_normal_image_output());
    assert!(matches!(
        &result.warnings[..],
        [
            ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name: metadata_source,
                ..
            },
            ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name: filename_source,
                ..
            },
            ImageWriteWarning::ExtensionFallback {
                source_name: fallback_source,
                format: ImageFormat::Png,
            },
        ] if metadata_source == "OEBPS/images/missing.png"
            && filename_source == "OEBPS/images/cover.jpg"
            && fallback_source == "OEBPS/images/page.png"
    ));
    assert_eq!(
        fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
        extension_only_page
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn cover_retries_precede_partial_normal_fallback_facts() {
    let temp_dir = temp_test_dir("epub", "cover-retries-partial-batch-fallback");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    let blocked_gif_output = temp_dir.join("blocked-gifs");
    let convertible_png = encoded_test_png();
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    fs::write(&blocked_gif_output, b"not a directory")
        .expect("blocked GIF destination should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/missing.png",
                "image/png",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/first.png", "image/png", None),
            ("animation", "images/second.gif", "image/gif", None),
        ],
        &[
            ("OEBPS/images/first.png", &convertible_png),
            ("OEBPS/images/second.gif", b"GIF89a"),
        ],
    );
    let conversion = ConversionPolicy::try_from(ConversionRequest {
        target: ConversionTarget::Jpg,
        quality: None,
        lossless: false,
    })
    .expect("test Conversion policy should be valid");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg, ImageFormat::Png, ImageFormat::Gif]),
        Some(conversion),
        Some(blocked_gif_output),
    ));
    let selected = select_epub(&input_path, &output_dir);

    let failure = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect_err("fallback emission failure should retain earlier normal output");

    assert_eq!(failure.partial.counts.extracted, 1);
    assert_eq!(failure.partial.counts.gifs_routed, 0);
    assert_eq!(failure.partial.counts.converted, 1);
    assert_eq!(failure.partial.counts.skipped, 0);
    assert!(failure.partial.has_normal_image_output());
    assert_eq!(
        acquisition_failure_sources(&failure.partial.warnings),
        vec!["OEBPS/images/missing.png", "OEBPS/images/cover.jpg"]
    );
    let emitted = fs::read_dir(&output_dir)
        .expect("normal output directory should remain readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("normal output entries should remain readable");
    assert_eq!(emitted.len(), 1);
    assert_eq!(
        emitted[0]
            .path()
            .extension()
            .and_then(|value| value.to_str()),
        Some("jpg")
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn routed_gif_fact_precedes_later_normal_fallback_failure() {
    let temp_dir = temp_test_dir("epub", "routed-gif-before-normal-failure");
    let input_path = temp_dir.join("sample.epub");
    let blocked_output = temp_dir.join("blocked-normal-output");
    let gif_output = temp_dir.join("gifs");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&blocked_output, b"not a directory")
        .expect("blocked normal destination should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/missing.png",
                "image/png",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("animation", "images/a.gif", "image/gif", None),
            ("page", "images/b.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/a.gif", b"GIF89a"),
            ("OEBPS/images/b.png", MINIMAL_PNG),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg, ImageFormat::Png, ImageFormat::Gif]),
        None,
        Some(gif_output.clone()),
    ));
    let selected = select_epub(&input_path, &blocked_output);

    let failure = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect_err("later normal emission failure should retain the routed GIF");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.counts.extracted, 1);
    assert_eq!(failure.partial.counts.gifs_routed, 1);
    assert_eq!(failure.partial.counts.converted, 0);
    assert!(failure.partial.has_normal_image_output());
    assert_eq!(
        acquisition_failure_sources(&failure.partial.warnings),
        vec!["OEBPS/images/missing.png", "OEBPS/images/cover.jpg"]
    );
    let routed = fs::read_dir(&gif_output)
        .expect("GIF output directory should remain readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("routed GIF entries should remain readable");
    assert_eq!(routed.len(), 1);
    assert_eq!(fs::read(routed[0].path()).unwrap(), b"GIF89a");

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn resolved_archive_identity_prevents_duplicate_cover_attempts() {
    let temp_dir = temp_test_dir("epub", "resolved-cover-identity");
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

    assert_eq!(result.counts.extracted, 1);
    assert!(result.has_normal_image_output());
    assert_eq!(
        result
            .warnings
            .iter()
            .filter(|warning| matches!(
                warning,
                ImageWriteWarning::ArchiveImageAcquisitionFailed { .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        acquisition_failure_sources(&result.warnings),
        vec!["OEBPS/images/cover%2Ejpg"]
    );
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

#[test]
fn cover_emission_failure_aborts_the_document() {
    let temp_dir = temp_test_dir("epub", "cover-emission-failure");
    let input_path = temp_dir.join("sample.epub");
    let blocked_output = temp_dir.join("not-a-directory");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&blocked_output, b"occupied").expect("blocking file should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/missing.png",
                "image/png",
                Some("cover-image"),
            ),
            (
                "filename-cover",
                "images/cover.jpg",
                "application/octet-stream",
                None,
            ),
            ("page", "images/page.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/cover.jpg", b"unidentified-cover"),
            ("OEBPS/images/page.png", MINIMAL_PNG),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
        None,
        None,
    ));
    let selected = select_epub(&input_path, &blocked_output);

    let error = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect_err("Image file emission failure must abort cover extraction");

    assert!(format!("{:#}", error.error).contains("Failed to create output directory"));
    assert_eq!(error.partial.counts.extracted, 0);
    assert!(matches!(
        &error.partial.warnings[..],
        [
            ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, .. },
            ImageWriteWarning::CoverDefaultToJpeg { mime },
        ] if source_name == "OEBPS/images/missing.png"
            && mime == "application/octet-stream"
    ));

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn fatal_metadata_cover_emission_stops_filename_candidate_and_normal_fallback() {
    let temp_dir = temp_test_dir("epub", "fatal-cover-stops-later-work");
    let input_path = temp_dir.join("sample.epub");
    let output_dir = temp_dir.join("out");
    let blocked_gif_output = temp_dir.join("blocked-gifs");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    fs::write(&blocked_gif_output, b"not a directory")
        .expect("blocked GIF destination should be creatable");
    write_stored_epub_fixture(
        &input_path,
        &[
            (
                "metadata-cover",
                "images/art.gif",
                "image/gif",
                Some("cover-image"),
            ),
            ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ("page", "images/page.png", "image/png", None),
        ],
        &[
            ("OEBPS/images/art.gif", b"GIF89a"),
            ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
            ("OEBPS/images/page.png", MINIMAL_PNG),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Gif, ImageFormat::Jpg, ImageFormat::Png]),
        None,
        Some(blocked_gif_output),
    ));
    let selected = select_epub(&input_path, &output_dir);

    let failure = extract(
        selected,
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: true,
        },
        &pipeline,
    )
    .expect_err("fatal metadata cover emission should abort the EPUB");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.counts.extracted, 0);
    assert_eq!(failure.partial.counts.gifs_routed, 0);
    assert!(!failure.partial.has_normal_image_output());
    assert!(failure.partial.warnings.is_empty());
    assert!(
        fs::read_dir(&output_dir)
            .expect("normal output directory should remain readable")
            .next()
            .is_none(),
        "neither the filename cover nor normal fallback may run after a fatal cover"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
