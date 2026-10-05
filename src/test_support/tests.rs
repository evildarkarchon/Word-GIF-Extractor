//! Tests for the shared test fixtures whose behaviour other tests rely on.

use super::*;

use std::panic::{self, AssertUnwindSafe};

/// Verifies a passing test's directory is removed with everything in it.
#[test]
fn temp_test_path_removes_a_populated_directory_when_dropped() {
    let temp_dir = temp_test_dir("test-support", "removes-directory");
    let path = temp_dir.to_path_buf();
    fs::create_dir_all(temp_dir.join("nested")).expect("nested directory should be creatable");
    fs::write(temp_dir.join("nested").join("file.bin"), b"payload")
        .expect("nested file should be writable");

    drop(temp_dir);

    assert!(
        !path.exists(),
        "a passing test's directory should be removed"
    );
}

/// Verifies a passing test's single file is removed.
#[test]
fn temp_test_path_removes_a_file_when_dropped() {
    let temp_epub = temp_epub_path("test-support", "removes-file");
    let path = temp_epub.to_path_buf();
    fs::write(&temp_epub, b"not an epub").expect("temporary file should be writable");

    drop(temp_epub);

    assert!(!path.exists(), "a passing test's file should be removed");
}

/// Verifies a path nothing was created at drops without complaint.
#[test]
fn temp_test_path_accepts_a_path_that_was_never_created() {
    let temp_dir = temp_test_dir("test-support", "never-created");
    assert!(!temp_dir.exists());

    drop(temp_dir);
}

/// Verifies a failing test keeps what it left behind for inspection.
///
/// The panic is caught so this test can look afterwards; the guard sees the same
/// unwinding a failing assertion would cause. The kept directory is removed by
/// hand at the end, since keeping it is the behaviour under test.
#[test]
fn temp_test_path_keeps_the_directory_of_a_failing_test() {
    let temp_dir = temp_test_dir("test-support", "keeps-on-failure");
    let path = temp_dir.to_path_buf();
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

    let unwound = panic::catch_unwind(AssertUnwindSafe(move || {
        let _held = temp_dir;
        panic!("a failing assertion inside the test");
    }));

    assert!(unwound.is_err());
    assert!(path.exists(), "a failing test's directory should be kept");
    fs::remove_dir_all(&path).expect("kept directory should be removable by hand");
}
