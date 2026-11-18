//! Pre-run notice wording, ordering, and which stream each one lands on.
//!
//! This file holds exactly one test on purpose. The defaulted-input notice only exists
//! when no input is named, which makes the test a statement about the process working
//! directory — shared by every test in a binary, and each file under `tests/` is its own
//! binary, so one test per file is what keeps that move from racing anything.

mod support;

use std::fs;
use std::process::ExitCode;

use support::{run_captured, temp_test_dir, with_current_dir};

/// Verifies the defaulted-input notice keeps its exact wording and its stream.
///
/// Defaulting the input is normal output, so it lands on standard output ahead of the
/// run's own lines, and a run that has nothing to warn about leaves standard error
/// empty. The ignored-format warning that once shared this test is gone: an
/// unrecognized `--formats` token is now a parse error (ADR-0027), pinned by
/// `binary_smoke.rs` because only the process exit path shows it.
#[test]
fn renders_the_defaulted_input_notice_on_standard_output() {
    let temp_dir = temp_test_dir("pre-run-notice", "ordered-streams");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

    let (result, capture, working_directory) = with_current_dir(&temp_dir, |working_directory| {
        let (result, capture) = run_captured(&["--formats", "png"]);
        (result, capture, working_directory.to_path_buf())
    });

    // An empty working directory finds no documents, which is not a failure, so the
    // run exits 0 (ADR-0014).
    assert_eq!(
        result.expect("intake should accept a bare format filter with no input named"),
        ExitCode::SUCCESS
    );
    let stdout = capture.stdout();
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            format!(
                "No input path specified, using current directory: {}",
                working_directory.display()
            ),
            "No documents found to process.".to_string(),
        ]
    );
    assert_eq!(capture.stderr(), "");
}
