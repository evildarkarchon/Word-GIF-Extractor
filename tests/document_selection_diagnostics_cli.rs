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

/// Creates a directory link used to exercise requested-root inspection through the CLI.
#[cfg(unix)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
    std::os::unix::fs::symlink(target, link).expect("test directory symlink should be creatable");
}

/// Creates a directory link without requiring Windows symbolic-link privileges.
#[cfg(windows)]
fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return;
    }

    let output = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("Windows junction command should run");
    assert!(
        output.status.success(),
        "test directory link should be creatable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Removes a directory link without following it into its target.
#[cfg(unix)]
fn remove_directory_link(link: &std::path::Path) {
    fs::remove_file(link).expect("test directory symlink should be removable");
}

/// Removes a Windows directory symlink or junction without following it.
#[cfg(windows)]
fn remove_directory_link(link: &std::path::Path) {
    fs::remove_dir(link).expect("test directory link should be removable");
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

#[test]
fn warns_for_broken_requested_link_before_no_documents_summary() {
    let temp_dir = temp_test_dir("broken-requested-link");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = temp_dir.join("broken-link");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&broken_link)
        .output()
        .expect("extractor binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["No documents found to process."]
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(stderr_lines.len(), 1, "unexpected stderr: {stderr}");
    let stable_prefix = format!(
        "Warning: Could not inspect {} during document discovery: ",
        broken_link.display()
    );
    let detail = stderr_lines[0]
        .strip_prefix(&stable_prefix)
        .unwrap_or_else(|| panic!("stderr did not use the stable discovery warning: {stderr}"));
    assert!(
        !detail.is_empty(),
        "discovery detail should remain non-empty"
    );

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies a nested non-recursive inspection failure renders once before normal completion.
#[test]
fn warns_once_for_broken_nested_link_during_non_recursive_discovery() {
    let temp_dir = temp_test_dir("broken-nested-link");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    fs::write(requested_directory.join("notes.txt"), [])
        .expect("unsupported entry should be writable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&requested_directory)
        .output()
        .expect("extractor binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["No documents found to process."]
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(stderr_lines.len(), 1, "unexpected stderr: {stderr}");
    let stable_prefix = format!(
        "Warning: Could not inspect {} during document discovery: ",
        broken_link.display()
    );
    let detail = stderr_lines[0]
        .strip_prefix(&stable_prefix)
        .unwrap_or_else(|| panic!("stderr did not use the stable discovery warning: {stderr}"));
    assert!(
        !detail.is_empty(),
        "discovery detail should remain non-empty"
    );

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
