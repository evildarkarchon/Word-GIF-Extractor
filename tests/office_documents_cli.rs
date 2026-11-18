//! Office documents of every suite, and which of their archive entries are images.
//!
//! Audit finding M5: the Office adapter used to offer every ZIP entry to the Image
//! write pipeline, so Word's `docProps/thumbnail.jpeg` preview was written as image 1
//! and renumbered the document's real images. Only the package's media folder holds
//! document images now (ADR-0029), and the same rule is what lets the adapter read
//! PowerPoint, Excel and OpenDocument files too.

mod support;

use std::fs;
use std::process::ExitCode;

use support::{
    MINIMAL_PNG, run_captured, sorted_file_names, temp_test_dir, test_jpeg, write_docx,
    write_office_document,
};

/// The audit's reproduction: `thumb.docx` holds a preview thumbnail ahead of its only
/// image in ZIP order, and must write `thumb.png` alone.
#[test]
fn a_word_preview_thumbnail_is_not_extracted_and_does_not_renumber_the_image() {
    let temp_dir = temp_test_dir("office-documents", "word-thumbnail");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be created");
    let input = temp_dir.join("thumb.docx");
    let thumbnail = test_jpeg();
    write_docx(
        &input,
        &[
            ("docProps/thumbnail.jpeg", thumbnail.as_slice()),
            ("word/document.xml", b"<w:document/>".as_slice()),
            ("word/media/image1.png", MINIMAL_PNG),
        ],
    );

    let (result, capture) = run_captured(&[
        input.to_string_lossy().as_ref(),
        "-o",
        output_dir.to_string_lossy().as_ref(),
    ]);

    assert_eq!(
        result.expect("intake should accept a named document and output directory"),
        ExitCode::SUCCESS
    );
    assert_eq!(sorted_file_names(&output_dir), ["thumb.png"]);
    assert_eq!(fs::read(output_dir.join("thumb.png")).unwrap(), MINIMAL_PNG);
    assert_eq!(capture.stderr(), "", "unexpected standard error");
}

/// One document from each other suite, found by a directory search, each read from
/// its own media folder and each with a thumbnail of its own that is left behind.
#[test]
fn presentations_workbooks_drawings_and_opendocument_files_are_extracted() {
    let temp_dir = temp_test_dir("office-documents", "every-suite");
    let input_dir = temp_dir.join("documents");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    let thumbnail = test_jpeg();

    write_office_document(
        &input_dir.join("slides.pptx"),
        &[
            ("docProps/thumbnail.jpeg", thumbnail.as_slice()),
            ("ppt/media/image1.png", MINIMAL_PNG),
            ("ppt/media/image2.jpeg", thumbnail.as_slice()),
        ],
    );
    write_office_document(
        &input_dir.join("budget.xlsx"),
        &[
            ("docProps/thumbnail.jpeg", thumbnail.as_slice()),
            ("xl/media/image1.png", MINIMAL_PNG),
        ],
    );
    write_office_document(
        &input_dir.join("plan.vsdx"),
        &[
            ("docProps/thumbnail.jpeg", thumbnail.as_slice()),
            ("visio/media/image1.png", MINIMAL_PNG),
        ],
    );
    write_office_document(
        &input_dir.join("notes.odt"),
        &[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.text".as_slice(),
            ),
            ("Thumbnails/thumbnail.png", MINIMAL_PNG),
            ("Pictures/1000000000000001000000014D2C9B3A.png", MINIMAL_PNG),
        ],
    );

    let (result, capture) = run_captured(&[
        input_dir.to_string_lossy().as_ref(),
        "-o",
        output_dir.to_string_lossy().as_ref(),
    ]);

    assert_eq!(
        result.expect("intake should accept a directory and output directory"),
        ExitCode::SUCCESS
    );
    // The presentation's two media images are numbered from 1; a thumbnail counted
    // first would have made them `slides_2` and `slides_3`.
    assert_eq!(
        sorted_file_names(&output_dir),
        [
            "budget.png",
            "notes.png",
            "plan.png",
            "slides_1.png",
            "slides_2.jpg",
        ]
    );
    assert_eq!(capture.stderr(), "", "unexpected standard error");
    assert!(
        capture
            .stdout()
            .contains("Extracted 5 image(s) from 4 document(s)"),
        "unexpected standard output: {}",
        capture.stdout()
    );
}
