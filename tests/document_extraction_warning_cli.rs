//! How a Document extraction warning reaches the terminal during a whole run.

mod support;

use std::fs;

use support::{run_captured, temp_test_dir, write_extension_fallback_docx};

/// Verifies a whole run prefixes a Document extraction warning exactly once.
///
/// This drives the run end to end rather than the presentation helper alone, so it also
/// proves the run's document path never reaches the rendered line.
#[test]
fn renders_document_extraction_warning_with_one_prefix_and_no_document_path() {
    let temp_dir = temp_test_dir("extraction-warning", "single-prefix");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let document_path = temp_dir.join("warned.docx");
    write_extension_fallback_docx(&document_path);

    let (result, capture) = run_captured(&[
        document_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
    ]);

    result.expect("intake should accept a named input and an output directory");
    assert!(output_dir.join("warned.png").exists());
    // The run summary belongs to the progress display, which is a sink of its own, so
    // neither text stream carries it and standard output stays empty.
    let stdout = capture.stdout();
    assert!(stdout.is_empty(), "unexpected standard output: {stdout}");
    assert!(
        capture.writes() > 0,
        "the run summary should have been drawn on the progress display"
    );

    let stderr = capture.stderr();
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(stderr_lines.len(), 1, "unexpected standard error: {stderr}");
    // Stripping one prefix must leave a non-empty body that still carries no prefix of
    // its own, which is the presentation contract without this test owning the Document
    // extraction wording.
    let body = stderr_lines[0]
        .strip_prefix("Warning: ")
        .unwrap_or_else(|| panic!("standard error did not use the warning prefix: {stderr}"));
    assert!(!body.is_empty(), "warning body should remain non-empty");
    assert!(
        !body.contains("Warning:"),
        "presentation should add exactly one prefix: {stderr}"
    );
    assert!(
        !stderr_lines[0].contains(&document_path.display().to_string()),
        "the document path is run context only and must not be rendered: {stderr}"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
