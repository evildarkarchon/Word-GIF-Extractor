//! The one integration test that still drives the compiled binary.
//!
//! Every other integration test calls the library entry point in-process with a
//! capturing destination, because that is faster and can assert things a terminal never
//! prints. Two things are invisible from there and only from here: the argument wiring
//! `main` performs, and the process exit path. So one subprocess test remains, and it
//! asserts only what needs a subprocess to observe.

mod support;

use std::fs;
use std::process::Command;

use support::{temp_test_dir, write_png_docx};

/// Verifies the shipped binary wires its arguments through and exits successfully.
///
/// The exit assertion is weaker than it looks, knowingly so. `main` discards the
/// Extraction run outcome, so the binary exits zero for a run that found no documents,
/// produced no output, or failed every document it opened — a successful exit therefore
/// says nothing about whether the run achieved anything, which is why the emitted file is
/// asserted separately. That is a known defect, deliberately out of scope for the test
/// conversion this file belongs to; the assertion pins current behaviour rather than
/// fixing it. Whoever gives the outcomes exit codes should expect to strengthen it.
#[test]
fn compiled_binary_extracts_and_exits_successfully() {
    let temp_dir = temp_test_dir("binary-smoke", "successful-exit");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let docx_path = temp_dir.join("sample.docx");
    write_png_docx(&docx_path);

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&docx_path)
        .arg("--output")
        .arg(&output_dir)
        .arg("--formats")
        .arg("png")
        .output()
        .expect("extractor binary should run");

    assert!(
        output.status.success(),
        "extractor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output_dir.join("sample.png").exists(),
        "expected the extracted PNG in {}",
        output_dir.display()
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
