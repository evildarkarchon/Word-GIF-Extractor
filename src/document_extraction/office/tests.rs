//! Tests for Office document processing.

use super::*;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::ImageWriteWarning;
use crate::test_support::{
    MINIMAL_PNG, file_names_in, pipeline_accepting, temp_test_dir, write_docx,
    write_extension_fallback_docx,
};
use std::fs;

#[test]
fn returns_extension_fallback_warning_fact() {
    let temp_dir = temp_test_dir("docx", "extension-fallback-warning");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_extension_fallback_docx(&input_path);

    let result = process_file(
        &input_path,
        OfficePackage::Word,
        &OutputPlacement::new(&output_dir, "sample"),
        &pipeline_accepting([ImageFormat::Png]),
    )
    .expect("DOCX extraction should succeed");

    assert_eq!(result.tally.emitted(), 1);
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::ExtensionFallback {
            source_name: "word/media/image1.png".to_string(),
            format: ImageFormat::Png,
        }]
    );
}

#[test]
fn preserves_zip_order_for_numbered_outputs() {
    let temp_dir = temp_test_dir("docx", "zip-order");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let mut first = b"\x89PNG\r\n\x1A\n".to_vec();
    first.push(1);
    let mut second = b"GIF89a".to_vec();
    second.push(2);
    write_docx(
        &input_path,
        &[
            ("word/media/z.png", first.as_slice()),
            ("word/document.xml", b"<document/>".as_slice()),
            ("word/media/a.gif", second.as_slice()),
        ],
    );

    let result = process_file(
        &input_path,
        OfficePackage::Word,
        &OutputPlacement::new(&output_dir, "sample"),
        &pipeline_accepting([ImageFormat::Png, ImageFormat::Gif]),
    )
    .expect("DOCX extraction should succeed");

    assert_eq!(result.tally.emitted(), 2);
    assert_eq!(fs::read(output_dir.join("sample_1.png")).unwrap(), first);
    assert_eq!(fs::read(output_dir.join("sample_2.gif")).unwrap(), second);
}

/// Extracts a DOCX whose only entry is a valid PNG stored under `entry_name`, then
/// asserts the image was emitted under the Output placement's base name and that
/// nothing was written anywhere but the output directory.
///
/// The entry name is format evidence and warning identity only, so an unusual one
/// must neither suppress the image nor steer where it is written.
fn assert_entry_is_emitted_under_base_name(test_name: &str, entry_name: &str) {
    let temp_dir = temp_test_dir("docx", test_name);
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_docx(&input_path, &[(entry_name, MINIMAL_PNG)]);

    let result = process_file(
        &input_path,
        OfficePackage::Word,
        &OutputPlacement::new(&output_dir, "sample"),
        &pipeline_accepting([ImageFormat::Png]),
    )
    .expect("DOCX extraction should succeed");

    assert_eq!(result.tally.emitted(), 1, "{entry_name} should be emitted");
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(
        fs::read(output_dir.join("sample.png")).unwrap(),
        MINIMAL_PNG
    );
    assert_eq!(file_names_in(&output_dir), ["sample.png"]);
    assert_eq!(file_names_in(&temp_dir), ["out", "sample.docx"]);
}

#[test]
fn entry_name_with_parent_segments_is_emitted_under_base_name() {
    assert_entry_is_emitted_under_base_name(
        "parent-segment-entry-name",
        "word/media/../../secret.png",
    );
}

#[test]
fn entry_name_with_colon_is_emitted_under_base_name() {
    assert_entry_is_emitted_under_base_name("colon-entry-name", "word/media/image1.png:hidden");
}

/// A preview thumbnail whose JPEG signature the PNG-and-JPEG pipeline below would
/// accept, so only the media-folder rule can keep it out of the output.
const THUMBNAIL_JPEG: &[u8] = b"\xFF\xD8\xFF\xE0thumbnail";

/// Extracts an Office document of `package` holding `entries` with a PNG-and-JPEG
/// pipeline, returning the outcome and the names written to the output directory.
fn extract_office_document(
    test_name: &str,
    file_name: &str,
    package: OfficePackage,
    entries: &[(&str, &[u8])],
) -> (crate::image_write_pipeline::ImageWriteResult, Vec<String>) {
    let temp_dir = temp_test_dir("office", test_name);
    let input_path = temp_dir.join(file_name);
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_docx(&input_path, entries);

    let result = process_file(
        &input_path,
        package,
        &OutputPlacement::new(&output_dir, "sample"),
        &pipeline_accepting([ImageFormat::Png, ImageFormat::Jpg]),
    )
    .expect("Office document extraction should succeed");
    (result, file_names_in(&output_dir))
}

/// The audit's M5 reproduction: Word stores its preview thumbnail first in ZIP
/// order, and it used to become image 1 and turn `sample.png` into `sample_2.png`.
#[test]
fn passes_over_the_preview_thumbnail_so_a_single_image_keeps_the_singular_name() {
    let (result, written) = extract_office_document(
        "word-thumbnail",
        "sample.docx",
        OfficePackage::Word,
        &[
            ("docProps/thumbnail.jpeg", THUMBNAIL_JPEG),
            ("word/document.xml", b"<document/>".as_slice()),
            ("word/media/image1.png", MINIMAL_PNG),
        ],
    );

    assert_eq!(result.tally.emitted(), 1);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(written, ["sample.png"]);
}

/// An image stored anywhere but the media folder is not a document image, so it
/// is passed over without a warning, whatever its bytes are.
#[test]
fn passes_over_images_outside_the_media_folder_without_warning() {
    let (result, written) = extract_office_document(
        "outside-media",
        "sample.docx",
        OfficePackage::Word,
        &[
            ("customXml/item1.png", MINIMAL_PNG),
            ("word/embeddings/preview.png", MINIMAL_PNG),
        ],
    );

    assert_eq!(result.tally.emitted(), 0);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert!(written.is_empty(), "{written:?}");
}

/// A PowerPoint presentation reads `ppt/media/`, not Word's folder.
#[test]
fn reads_a_presentation_from_its_own_media_folder() {
    let (result, written) = extract_office_document(
        "powerpoint-media",
        "slides.pptx",
        OfficePackage::PowerPoint,
        &[
            ("docProps/thumbnail.jpeg", THUMBNAIL_JPEG),
            ("ppt/slides/slide1.xml", b"<slide/>".as_slice()),
            ("ppt/media/image1.png", MINIMAL_PNG),
        ],
    );

    assert_eq!(result.tally.emitted(), 1);
    assert_eq!(written, ["sample.png"]);
}

/// An OpenDocument file reads `Pictures/` and passes over `Thumbnails/`, which
/// LibreOffice writes into every file it saves.
#[test]
fn reads_an_opendocument_file_from_pictures_and_passes_over_its_thumbnail() {
    let (result, written) = extract_office_document(
        "opendocument-pictures",
        "notes.odt",
        OfficePackage::OpenDocument,
        &[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.text".as_slice(),
            ),
            ("Thumbnails/thumbnail.png", MINIMAL_PNG),
            ("content.xml", b"<office:document-content/>".as_slice()),
            ("Pictures/10000000000000010000000162A1B2C3.png", MINIMAL_PNG),
        ],
    );

    assert_eq!(result.tally.emitted(), 1);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(written, ["sample.png"]);
}
