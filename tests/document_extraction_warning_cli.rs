use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

/// Returns an isolated temporary directory for one CLI integration test.
fn temp_test_dir(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-extraction-warning-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

/// Writes a DOCX whose single image defeats magic detection and warns on fallback.
fn write_warning_docx(path: &Path) {
    let file = fs::File::create(path).expect("test DOCX should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("word/media/only.png", SimpleFileOptions::default())
        .expect("ZIP entry should start");
    zip.write_all(b"not actually a png")
        .expect("ZIP entry payload should be writable");
    zip.finish().expect("test DOCX should finish");
}

/// Verifies the shipped binary prefixes a Document extraction warning exactly once.
///
/// This exercises the real observer write rather than the presentation helper, so
/// it also proves the run's document path never reaches the rendered line.
#[test]
fn renders_document_extraction_warning_with_one_prefix_and_no_document_path() {
    let temp_dir = temp_test_dir("single-prefix");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let document_path = temp_dir.join("warned.docx");
    write_warning_docx(&document_path);

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&document_path)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("extractor binary should run");

    assert!(
        output.status.success(),
        "extractor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The run summary belongs to the progress bar, which indicatif hides when
    // stderr is a pipe, so the emitted file is what stays observable here.
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(output_dir.join("warned.png").exists());

    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(stderr_lines.len(), 1, "unexpected stderr: {stderr}");
    // Stripping one prefix must leave a non-empty body that still carries no
    // prefix of its own, which is the presentation contract without this test
    // owning the Document extraction wording.
    let body = stderr_lines[0]
        .strip_prefix("Warning: ")
        .unwrap_or_else(|| panic!("stderr did not use the warning prefix: {stderr}"));
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
