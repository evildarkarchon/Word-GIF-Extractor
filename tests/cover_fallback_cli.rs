//! What `--cover-fallback` does for an EPUB whose only cover is filtered out.

mod support;

use std::fs;
use std::process::ExitCode;

use support::{MINIMAL_PNG, run_captured, sorted_file_names, temp_test_dir, test_jpeg, write_epub};

/// Verifies that a cover filtered out by `--formats` is no usable cover, so a
/// cover-only run with fallback writes the EPUB's JPEG normal images instead.
///
/// The EPUB declares a PNG cover and has no `cover.jpg`, so the filtered cover is the
/// only cover candidate. The run warns why the cover was passed over, reports normal
/// images rather than covers in its summary, and exits successfully (ADR-0023).
#[test]
fn filtered_only_cover_falls_back_to_jpeg_normal_images() {
    let temp_dir = temp_test_dir("cover-fallback", "filtered-only-cover");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let document_path = temp_dir.join("book.epub");
    let first_page = test_jpeg();
    let mut second_page = test_jpeg();
    // A trailing byte after the JPEG's end marker keeps it a JPEG but tells the two
    // written pages apart.
    second_page.push(0);
    write_epub(
        &document_path,
        "Fallback",
        &[
            ("cover", "images/art.png", "image/png", Some("cover-image")),
            ("first", "images/first.jpg", "image/jpeg", None),
            ("second", "images/second.jpg", "image/jpeg", None),
        ],
        &[
            // The PNG's magic bytes identify it, so `--formats jpg` filters it out
            // as a cover.
            ("OEBPS/images/art.png", MINIMAL_PNG),
            ("OEBPS/images/first.jpg", &first_page),
            ("OEBPS/images/second.jpg", &second_page),
        ],
    );

    let (result, capture) = run_captured(&[
        document_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
        "--cover-only",
        "--cover-fallback",
        "--formats",
        "jpg",
    ]);

    let exit_code = result.expect("intake should accept a cover-only run with fallback");
    assert_eq!(exit_code, ExitCode::SUCCESS);

    assert_eq!(
        sorted_file_names(&output_dir),
        ["Fallback_1.jpg", "Fallback_2.jpg"]
    );
    assert_eq!(
        fs::read(output_dir.join("Fallback_1.jpg")).expect("first page should be readable"),
        first_page
    );
    assert_eq!(
        fs::read(output_dir.join("Fallback_2.jpg")).expect("second page should be readable"),
        second_page
    );

    assert_eq!(
        capture.stderr(),
        "Warning: Cover image format 'png' not in allowed formats, skipping.\n"
    );
    let progress = capture.progress_text();
    assert!(
        progress.contains("Extracted 2 image(s) from 1 document(s)"),
        "the summary should report normal images, not covers: {progress}"
    );
}
