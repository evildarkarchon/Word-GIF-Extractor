//! A document reached more than once in one run, as the whole run handles it.
//!
//! Document selection treats a path as one candidate (ADR-0019). The selection tests
//! pin that against the in-memory search surface; this file pins the defect where it
//! was visible to a user: a DOCX named directly and also reached through its
//! directory was extracted twice, and because Image file emission never overwrites,
//! the second pass wrote a collision-suffixed copy of every image beside the first.

mod support;

use std::fs;

use support::{run_captured, temp_test_dir, write_png_docx};

/// Verifies a DOCX named directly and through its directory writes its image once.
///
/// The directory is listed first so the traversal sighting is the earlier one, which
/// is the order that used to produce `report_1.png` beside `report.png`.
#[test]
fn writes_each_image_once_when_a_docx_is_named_and_reached_through_its_directory() {
    let temp_dir = temp_test_dir("repeat-sighting", "docx-and-directory");
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be created");

    let docx_path = input_dir.join("report.docx");
    write_png_docx(&docx_path);

    let (result, capture) = run_captured(&[
        input_dir.to_string_lossy().as_ref(),
        docx_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
    ]);

    result.expect("intake should accept a directory and a file inside it");
    let mut written: Vec<_> = fs::read_dir(&output_dir)
        .expect("output directory should exist after extraction")
        .map(|entry| {
            entry
                .expect("output entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    written.sort();
    assert_eq!(
        written,
        ["report.png"],
        "each image should be written once, with no collision-suffixed copy"
    );
    assert_eq!(capture.stderr(), "", "unexpected standard error");
}
