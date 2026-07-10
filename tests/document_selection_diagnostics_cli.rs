use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns an isolated temporary directory for one CLI integration test.
fn temp_test_dir(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-selection-diagnostic-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

#[test]
fn warns_when_deduplication_uses_filename_after_metadata_failure() {
    let temp_dir = temp_test_dir("dedupe-fallback");
    let input_path = temp_dir.join("invalid.epub");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&input_path, b"not an epub").expect("invalid EPUB should be writable");

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&input_path)
        .output()
        .expect("extractor binary should run");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("during deduplication; using filename fallback"),
        "stderr did not contain the deduplication fallback warning: {stderr}"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
