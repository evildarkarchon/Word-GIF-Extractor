//! Tests for collision-safe Image file emission.

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns a unique local filesystem directory for one emission test.
fn temp_test_dir(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-emission-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

#[test]
fn write_failure_removes_the_reserved_partial_file() {
    let temp_dir = temp_test_dir("partial-cleanup");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let output_path = temp_dir.join("sample.png");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .expect("test output should be reservable");

    let result = complete_reserved_file(&output_path, file, |mut file| {
        file.write_all(b"partial")
            .expect("partial test bytes should be writable");
        Err(FileCompletionError::write(io::Error::other(
            "injected write failure",
        )))
    });

    assert!(result.is_err());
    assert!(!output_path.exists());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn cleanup_failure_is_reported_with_the_original_write_failure() {
    let temp_dir = temp_test_dir("cleanup-failure");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let output_path = temp_dir.join("sample.png");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .expect("test output should be reservable");
    let replacement_path = output_path.clone();

    let error = complete_reserved_file(&output_path, file, move |mut file| {
        file.write_all(b"partial")
            .expect("partial test bytes should be writable");
        drop(file);
        fs::remove_file(&replacement_path).expect("reserved test file should be removable");
        fs::create_dir(&replacement_path)
            .expect("replacement directory should make file cleanup fail");
        fs::write(replacement_path.join("child"), b"occupied")
            .expect("replacement directory should be non-empty");
        Err(FileCompletionError::write(io::Error::other(
            "injected write failure",
        )))
    })
    .expect_err("injected write and cleanup failures should be returned");
    let error_report = format!("{error:#}");

    assert!(error_report.contains("injected write failure"));
    assert!(error_report.contains("failed to remove partial output file"));
    assert!(error_report.contains(&output_path.display().to_string()));

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
