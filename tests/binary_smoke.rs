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
/// A successful exit means no document failed, per ADR-0014, not that anything was
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

/// Verifies the run summary reaches standard output when standard error is not a terminal.
///
/// `output()` pipes both streams, so the binary runs exactly as it does under a
/// redirect, a scheduled task or CI. The progress library hides its display there, and
/// a summary drawn onto that display used to vanish, leaving a successful run with no
/// output at all (audit H3, ADR-0026). This is the one place the real hidden display
/// can be observed: an in-process capture substitutes its own draw target.
#[test]
fn compiled_binary_prints_its_summary_when_stderr_is_not_a_terminal() {
    let temp_dir = temp_test_dir("binary-smoke", "redirected-summary");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let docx_path = temp_dir.join("sample.docx");
    write_png_docx(&docx_path);

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&docx_path)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("extractor binary should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "extractor failed\nstderr: {stderr}"
    );
    // Standard output carries the summary and nothing else, so a script can read the
    // result from one place. Comparing lines rather than bytes keeps this independent
    // of the platform's line ending.
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["Extracted 1 image(s) from 1 document(s)"],
        "the summary should be the whole of standard output\nstderr: {stderr}"
    );
    assert_eq!(
        stderr, "",
        "a clean run should write nothing to standard error"
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

/// Verifies an unrecognized `--formats` token stops the binary before any extraction.
///
/// A typo such as `pgn` used to be ignored with a warning, and when it was the only
/// token the filter widened to every format and the run exited 0 (audit M3,
/// ADR-0027). It is now a usage error, so this pins the three things only the process
/// shows: the usage exit status 2, the message on standard error naming the token
/// and the valid names, and no output directory, because nothing ran.
#[test]
fn compiled_binary_rejects_an_unrecognized_format() {
    let temp_dir = temp_test_dir("binary-smoke", "unrecognized-format");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let docx_path = temp_dir.join("sample.docx");
    write_png_docx(&docx_path);

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&docx_path)
        .arg("--formats")
        .arg("pgn")
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("extractor binary should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "an unrecognized format should be a usage error
stderr: {stderr}"
    );
    assert!(
        stderr.contains("invalid value 'pgn' for '--formats <FORMATS>'"),
        "the error should name the rejected token: {stderr}"
    );
    assert!(
        stderr.contains("[possible values: jpg, png, gif,"),
        "the error should list the valid format names: {stderr}"
    );
    assert_eq!(stdout, "", "a rejected invocation should print no output");
    assert!(
        !output_dir.exists(),
        "a rejected invocation should create no output directory"
    );
}
