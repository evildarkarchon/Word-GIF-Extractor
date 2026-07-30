//! Document selection diagnostics as a whole run renders them.

mod support;

use std::fs;
use std::path::Path;

use word_image_extractor::Capture;

use support::{create_directory_link, remove_directory_link, run_captured, temp_test_dir};

/// Asserts the run said nothing but the discovery warning for `path`, and said it once.
///
/// The detail after the stable prefix is the operating system's, so it is only required
/// to be present: what these tests own is that exactly one line is written, that it
/// carries the stable wording, and that it reaches standard error while the summary
/// reaches standard output.
fn assert_single_discovery_warning(capture: &Capture, path: &Path) {
    let stdout = capture.stdout();
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        ["No documents found to process."]
    );

    let stderr = capture.stderr();
    // Progress drawing goes to a sink of its own, so a control sequence reaching here
    // would mean presentation put one in the warning itself rather than a redraw
    // landing on top of it. The captured stream needs no line-ending normalisation
    // first: the destination terminates its lines with `\n` on every platform.
    assert!(
        !stderr.contains(['\r', '\u{1b}']),
        "the rendered warning contained terminal control sequences: {stderr:?}"
    );
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(stderr_lines.len(), 1, "unexpected standard error: {stderr}");
    let stable_prefix = format!(
        "Warning: Could not inspect {} during document discovery: ",
        path.display()
    );
    let detail = stderr_lines[0]
        .strip_prefix(&stable_prefix)
        .unwrap_or_else(|| {
            panic!("standard error did not use the stable discovery warning: {stderr}")
        });
    assert!(
        !detail.is_empty(),
        "discovery detail should remain non-empty"
    );
}

#[test]
fn warns_when_deduplication_uses_filename_after_metadata_failure() {
    let temp_dir = temp_test_dir("selection-diagnostic", "dedupe-fallback");
    let input_path = temp_dir.join("invalid.epub");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&input_path, b"not an epub").expect("invalid EPUB should be writable");

    let (result, capture) = run_captured(&[input_path.to_string_lossy().as_ref()]);

    result.expect("intake should accept a named EPUB input");
    let stderr = capture.stderr();
    assert!(
        stderr.contains("during deduplication; using filename fallback"),
        "standard error did not contain the deduplication fallback warning: {stderr}"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn warns_for_broken_requested_link_before_no_documents_summary() {
    let temp_dir = temp_test_dir("selection-diagnostic", "broken-requested-link");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = temp_dir.join("broken-link");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");

    let (result, capture) = run_captured(&[broken_link.to_string_lossy().as_ref()]);

    result.expect("intake should accept a named directory input");
    assert_single_discovery_warning(&capture, &broken_link);

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies a nested non-recursive inspection failure renders once before normal completion.
///
/// This is also the contrast case for
/// [`warns_once_for_broken_nested_link_during_recursive_discovery`]: non-recursive
/// discovery raises no scan spinner, so the run below has no progress display at all and
/// its counters stay at zero.
#[test]
fn warns_once_for_broken_nested_link_during_non_recursive_discovery() {
    let temp_dir = temp_test_dir("selection-diagnostic", "broken-nested-link");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    fs::write(requested_directory.join("notes.txt"), [])
        .expect("unsupported entry should be writable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");

    let (result, capture) = run_captured(&[requested_directory.to_string_lossy().as_ref()]);

    result.expect("intake should accept a named directory input");
    assert_single_discovery_warning(&capture, &broken_link);
    assert_eq!(
        (capture.clear_lines(), capture.writes()),
        (0, 0),
        "a non-recursive run raises no progress display for the warning to suspend"
    );

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies a recursive warning renders once and leaves normal run completion intact.
///
/// Recursive discovery runs behind a live scan spinner, and this is where that matters:
/// presentation suspends the spinner around the warning, so the display clears its drawn
/// lines and redraws them afterwards rather than painting over the message. Both
/// counters are cumulative lower bounds — attributing them to the one suspend is only
/// possible from inside the crate, where
/// `recursive_discovery_diagnostic_suspends_active_scan_spinner` does exactly that. What
/// this test adds is the other half of the same claim from outside: a display was live
/// and drawing, and the warning still arrived as one intact line.
#[test]
fn warns_once_for_broken_nested_link_during_recursive_discovery() {
    let temp_dir = temp_test_dir("selection-diagnostic", "recursive-broken-nested-link");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    fs::write(requested_directory.join("notes.txt"), [])
        .expect("unsupported entry should be writable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");

    let (result, capture) = run_captured(&[
        requested_directory.to_string_lossy().as_ref(),
        "--recursive",
    ]);

    result.expect("intake should accept a named directory input with --recursive");
    assert_single_discovery_warning(&capture, &broken_link);
    assert!(
        capture.clear_lines() > 0,
        "the scan spinner should have cleared its drawn lines around the warning"
    );
    assert!(
        capture.writes() > 0,
        "the scan spinner should have drawn to the progress display"
    );

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
