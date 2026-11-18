//! How a Document extraction warning reaches the terminal during a whole run.

mod support;

use std::fs;
use std::process::ExitCode;

use support::{run_captured, temp_test_dir, write_extension_fallback_docx};

/// Verifies a whole run prefixes a Document extraction warning once and names its document.
///
/// This drives the run end to end rather than the presentation helper alone, so it also
/// proves the run's document path reaches the rendered line. Entry names repeat across
/// documents, so a batch's warnings could not be traced without it.
#[test]
fn renders_document_extraction_warning_with_one_prefix_and_the_document_path() {
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

    // A warning is not a failure: a run whose only problem was printed as a
    // warning exits 0 (ADR-0014). This is that rule pinned through a whole run.
    assert_eq!(
        result.expect("intake should accept a named input and an output directory"),
        ExitCode::SUCCESS
    );
    assert!(output_dir.join("warned.png").exists());
    // The warning goes to standard error and the summary to standard output, so the
    // summary is the only line standard output carries.
    let stdout = capture.stdout();
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["Extracted 1 image(s) from 1 document(s)"],
        "unexpected standard output: {stdout}"
    );

    let stderr = capture.stderr();
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(stderr_lines.len(), 1, "unexpected standard error: {stderr}");
    // Stripping one prefix and then the document path must leave a non-empty body that
    // still carries no prefix of its own, which is the presentation contract without this
    // test owning the Document extraction wording.
    let body = stderr_lines[0]
        .strip_prefix("Warning: ")
        .unwrap_or_else(|| panic!("standard error did not use the warning prefix: {stderr}"))
        .strip_prefix(&format!("{}: ", document_path.display()))
        .unwrap_or_else(|| panic!("the warning should name its document: {stderr}"));
    assert!(!body.is_empty(), "warning body should remain non-empty");
    assert!(
        !body.contains("Warning:"),
        "presentation should add exactly one prefix: {stderr}"
    );
}
