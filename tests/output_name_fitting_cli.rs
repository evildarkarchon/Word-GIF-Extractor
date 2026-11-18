//! Output names a filesystem would refuse, as a whole run handles them.
//!
//! The 2026-11-14 audit (L5) found that an EPUB whose creator and title make a name
//! past the filesystem's 255-unit cap failed with a create error, and that a title
//! such as `NUL` names a Windows device. Image file emission's tests pin the fitting
//! rule itself. This file pins what a user sees: the run succeeds and writes a
//! usable file (ADR-0032).

mod support;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use support::{run_captured, sorted_file_names, temp_test_dir, write_declared_epub};

/// Runs the extractor over `input` into `output`, asserting the run succeeded.
fn run_into(input: &Path, output: &Path) {
    let input = input.to_string_lossy();
    let output = output.to_string_lossy();

    let (result, capture) = run_captured(&[&input, "--output", &output]);

    let exit = result.expect("intake should accept the run");
    assert_eq!(exit, ExitCode::SUCCESS, "stderr: {}", capture.stderr());
}

/// Verifies an EPUB whose creator and title are too long for one name is extracted.
#[test]
fn extracts_an_epub_whose_title_is_too_long_for_one_file_name() {
    let temp_dir = temp_test_dir("output-name-fitting", "long-title");
    let input = temp_dir.join("long.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&temp_dir).expect("input directory should be created");
    let title = "A Subtitle That Keeps Going ".repeat(12);
    write_declared_epub(&input, None, Some("Jane Doe"), Some(&title));

    run_into(&input, &output_dir);

    let names = sorted_file_names(&output_dir);
    assert_eq!(names.len(), 1, "{names:?}");
    // ASCII only, so its length is the same in UTF-16 units and UTF-8 bytes.
    assert!(names[0].len() <= 255, "{} units", names[0].len());
    let stem = names[0].strip_suffix(".png").expect("extension is kept");
    assert!(format!("Jane Doe - {title}").starts_with(stem), "{stem:?}");
}

/// Verifies an EPUB titled after a Windows device is written under a usable name.
#[test]
fn extracts_an_epub_titled_after_a_windows_device() {
    let temp_dir = temp_test_dir("output-name-fitting", "device-title");
    let input = temp_dir.join("device.epub");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&temp_dir).expect("input directory should be created");
    write_declared_epub(&input, None, None, Some("NUL"));

    run_into(&input, &output_dir);

    assert_eq!(sorted_file_names(&output_dir), ["_NUL.png"]);
}
