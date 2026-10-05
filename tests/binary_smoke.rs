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
/// A successful exit means no document failed, per ADR-0010, not that anything was
/// produced: a run that found no documents or no images also exits zero, because
/// finding nothing is an answer rather than a failure. That is why the emitted file is
/// asserted separately. The failure side of the contract is pinned by
/// [`compiled_binary_exits_with_failure_when_a_document_fails`].
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
}

/// Verifies a run whose document fails to extract exits with the failure status.
///
/// The document is selected by its extension and then cannot be opened as an archive,
/// so the run reports it as an error and carries on to its summary. Only the process
/// exit status says whether the run as a whole succeeded, which is why this lives in
/// the one file that drives the compiled binary.
#[test]
fn compiled_binary_exits_with_failure_when_a_document_fails() {
    let temp_dir = temp_test_dir("binary-smoke", "failed-document-exit");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let docx_path = temp_dir.join("broken.docx");
    fs::write(&docx_path, b"not a zip archive").expect("broken DOCX should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&docx_path)
        .arg("--output")
        .arg(temp_dir.join("output"))
        .output()
        .expect("extractor binary should run");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Error processing"),
        "the failed document should be reported as an error: {stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "a run with a failed document should exit with status 1\nstderr: {stderr}"
    );
}
