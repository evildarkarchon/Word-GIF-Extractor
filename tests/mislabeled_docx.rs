//! Format filtering against an image whose entry name does not name its format.

mod support;

use std::fs;

use support::{has_file_with_extension, run_captured, temp_test_dir, write_mislabeled_png_docx};

#[test]
fn extracts_mislabeled_png_when_filtering_for_png() {
    let temp_dir = temp_test_dir("mislabeled-docx", "png-filter");
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    fs::create_dir_all(&output_dir).expect("output directory should be created");

    let docx_path = input_dir.join("sample.docx");
    write_mislabeled_png_docx(&docx_path);

    let (result, capture) = run_captured(&[
        docx_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
        "--formats",
        "png",
    ]);

    result.expect("intake should accept a named input, output directory and format filter");
    assert!(
        has_file_with_extension(&output_dir, "png"),
        "expected at least one PNG output in {}",
        output_dir.display()
    );
    // Identifying the payload from its magic bytes is the normal path, not a fallback,
    // so nothing is warned about.
    assert_eq!(capture.stderr(), "", "unexpected standard error");

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
