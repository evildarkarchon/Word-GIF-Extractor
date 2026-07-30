//! Pre-run notice wording, ordering, and which stream each one lands on.
//!
//! This file holds exactly one test on purpose. The defaulted-input notice only exists
//! when no input is named, which makes the test a statement about the process working
//! directory — shared by every test in a binary, and each file under `tests/` is its own
//! binary, so one test per file is what keeps that move from racing anything.

mod support;

use std::fs;

use support::{run_captured, temp_test_dir, with_current_dir};

/// Verifies pre-run notices retain their exact wording, order, and output streams.
///
/// The two notices are the reason the destination keeps standard output and standard
/// error apart: defaulting the input is normal output, and ignoring a format token is a
/// warning, so a single merged stream would hide the difference.
#[test]
fn renders_ordered_pre_run_notices_on_existing_streams() {
    let temp_dir = temp_test_dir("pre-run-notice", "ordered-streams");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

    let (result, capture, working_directory) = with_current_dir(&temp_dir, |working_directory| {
        let (result, capture) = run_captured(&["--formats", "first,png,second"]);
        (result, capture, working_directory.to_path_buf())
    });

    result.expect("intake should accept a bare format filter with no input named");
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
    let stderr = capture.stderr();
    assert_eq!(
        stderr.lines().collect::<Vec<_>>(),
        vec![
            "Warning: Unrecognized format 'first' ignored",
            "Warning: Unrecognized format 'second' ignored",
        ]
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
