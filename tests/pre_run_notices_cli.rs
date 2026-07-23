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
        "word-image-extractor-pre-run-notice-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

/// Verifies pre-run notices retain their exact wording, order, and output streams.
#[test]
fn renders_ordered_pre_run_notices_on_existing_streams() {
    let temp_dir = temp_test_dir("ordered-streams");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .current_dir(&temp_dir)
        .args(["--formats", "first,png,second"])
        .output()
        .expect("extractor binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            format!(
                "No input path specified, using current directory: {}",
                temp_dir.display()
            ),
            "No documents found to process.".to_string(),
        ]
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert_eq!(
        stderr.lines().collect::<Vec<_>>(),
        vec![
            "Warning: Unrecognized format 'first' ignored",
            "Warning: Unrecognized format 'second' ignored",
        ]
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
