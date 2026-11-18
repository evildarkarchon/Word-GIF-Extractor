//! Document selection diagnostics as a whole run renders them.

mod support;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use word_image_extractor::Capture;

use support::{
    create_directory_link, remove_directory_link, run_captured, sorted_file_names, temp_test_dir,
    write_png_docx,
};

/// Asserts the run warned about `path` once and otherwise printed only `stdout_lines`.
///
/// The detail after the stable prefix is the operating system's, so it is only required
/// to be present: what these tests own is that exactly one line is written, that it
/// carries the stable wording, and that it reaches standard error while the summary
/// reaches standard output. `stdout_lines` is passed in because a recursive run also
/// prints its discovery conclusion ahead of the summary.
fn assert_single_discovery_warning(capture: &Capture, path: &Path, stdout_lines: &[&str]) {
    let stdout = capture.stdout();
    assert_eq!(stdout.lines().collect::<Vec<_>>(), stdout_lines);

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
fn warns_when_deduplication_uses_filename_after_declaration_failure() {
    let temp_dir = temp_test_dir("selection-diagnostic", "dedupe-fallback");
    let input_path = temp_dir.join("invalid.epub");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&input_path, b"not an epub").expect("invalid EPUB should be writable");

    let (result, capture) = run_captured(&[input_path.to_string_lossy().as_ref()]);

    // The selection warning does not exclude the file: it is still selected, and
    // extraction then fails to open it. That failure, not the warning, is what makes
    // the run exit 1, so its `Error processing` line is asserted beside the status.
    // ADR-0014 ties the two together: non-zero exactly when such a line was printed.
    assert_eq!(
        result.expect("intake should accept a named EPUB input"),
        ExitCode::FAILURE
    );
    let stderr = capture.stderr();
    assert!(
        stderr.contains(&format!("Error processing {}: ", input_path.display())),
        "the failed extraction should explain the failure status: {stderr}"
    );
    assert!(
        stderr.contains("during deduplication; using filename fallback"),
        "standard error did not contain the deduplication fallback warning: {stderr}"
    );
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

    // The requested path itself cannot be inspected. That is reported as a warning
    // and leaves no documents, which ADR-0014 counts as success. Audit finding M4
    // made a missing or unsupported named input an error; ADR-0028 left an
    // uninspectable one a warning, so this status did not change with it.
    assert_eq!(
        result.expect("intake should accept a named directory input"),
        ExitCode::SUCCESS
    );
    assert_single_discovery_warning(&capture, &broken_link, &["No documents found to process."]);

    remove_directory_link(&broken_link);
}

/// Verifies rejected named inputs fail the run without stopping the inputs beside them.
///
/// Audit finding M4. Like a failed document, a rejected input is partial failure: the
/// readable document is still extracted and the summary still printed, and then the
/// run exits 1 because two error lines were printed (ADR-0028).
#[test]
fn rejected_named_inputs_fail_the_run_and_the_rest_still_extract() {
    let temp_dir = temp_test_dir("selection-diagnostic", "rejected-named-inputs");
    let output_dir = temp_dir.join("output");
    let missing_path = temp_dir.join("nope.docx");
    let unsupported_path = temp_dir.join("report.doc");
    let document_path = temp_dir.join("sample.docx");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&unsupported_path, b"legacy Word binary")
        .expect("unsupported file should be writable");
    write_png_docx(&document_path);

    let (result, capture) = run_captured(&[
        missing_path.to_string_lossy().as_ref(),
        unsupported_path.to_string_lossy().as_ref(),
        document_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
    ]);

    assert_eq!(
        result.expect("intake should accept named inputs whatever they are"),
        ExitCode::FAILURE
    );
    assert_eq!(
        capture.stderr().lines().collect::<Vec<_>>(),
        [
            format!(
                "Error: Input path does not exist: {}",
                missing_path.display()
            ),
            format!(
                "Error: Input is not a supported document (such as .docx, .pptx, .xlsx, .odt or .epub) or a directory: {}",
                unsupported_path.display()
            ),
        ]
    );
    assert_eq!(
        capture.stdout(),
        "Extracted 1 image(s) from 1 document(s)\n"
    );
    assert_eq!(sorted_file_names(&output_dir), ["sample.png"]);
}

/// Verifies a title or author filter extracts no Office document, named or found.
///
/// Audit finding L6: `--author nobody ./docs` used to extract every DOCX in the folder,
/// because the filter applied to EPUBs alone. A filter asks for particular books, so
/// an Office document is not eligible (ADR-0033). The named DOCX is reported as skipped,
/// the one inside the directory is dropped without a line, nothing is written, and the
/// run still exits 0, because skipping an ineligible input is a decision, not an error.
#[test]
fn an_epub_filter_extracts_no_office_document() {
    let temp_dir = temp_test_dir("selection-diagnostic", "filter-skips-office");
    let docs_dir = temp_dir.join("docs");
    let output_dir = temp_dir.join("output");
    let named_path = temp_dir.join("named.docx");
    fs::create_dir_all(&docs_dir).expect("documents directory should be creatable");
    write_png_docx(&docs_dir.join("found.docx"));
    write_png_docx(&named_path);

    let (result, capture) = run_captured(&[
        "--author",
        "nobody",
        docs_dir.to_string_lossy().as_ref(),
        named_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
    ]);

    assert_eq!(
        result.expect("intake should accept an author filter"),
        ExitCode::SUCCESS
    );
    assert_eq!(
        capture.stderr().lines().collect::<Vec<_>>(),
        [format!(
            "Warning: Skipped {}: this run extracts EPUB documents only",
            named_path.display()
        )]
    );
    assert_eq!(
        capture.stdout().lines().collect::<Vec<_>>(),
        ["No documents found to process."]
    );
    assert!(
        !output_dir.exists(),
        "a run with no eligible document should write nothing"
    );
}

/// Verifies a nested non-recursive inspection failure renders once before normal completion.
///
/// This is also the contrast case for
/// [`warns_once_for_broken_nested_link_during_recursive_discovery`]: non-recursive
/// discovery raises no scan spinner, so the run below has no progress display at all and
/// draws nothing.
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

    // The broken link is an entry found inside the requested directory, not a named
    // input, so its warning leaves the exit status at 0 (ADR-0014), and would under
    // audit finding M4's proposal too.
    assert_eq!(
        result.expect("intake should accept a named directory input"),
        ExitCode::SUCCESS
    );
    assert_single_discovery_warning(&capture, &broken_link, &["No documents found to process."]);
    assert_eq!(
        capture.progress_text(),
        "",
        "a non-recursive run raises no progress display for the warning to suspend"
    );

    remove_directory_link(&broken_link);
}

/// Verifies a recursive warning renders once and leaves normal run completion intact.
///
/// Recursive discovery runs behind a live scan spinner, and this is where that matters:
/// presentation suspends the spinner around the warning, so the display clears its drawn
/// lines and redraws them afterwards rather than painting over the message. The capture
/// records every sink in one order, so that is asserted from outside exactly: the warning
/// line sits between a clear and a redraw, and it still arrived as one intact line.
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

    // As in the non-recursive case, a broken entry found by the search is a
    // warning, not a failure, so the run exits 0 (ADR-0014).
    assert_eq!(
        result.expect("intake should accept a named directory input with --recursive"),
        ExitCode::SUCCESS
    );
    assert_single_discovery_warning(
        &capture,
        &broken_link,
        &["Found 0 document(s)", "No documents found to process."],
    );
    let stderr = capture.stderr();
    let warning = stderr
        .lines()
        .next()
        .expect("the discovery warning was asserted above");
    assert!(
        capture.suspended_around(warning),
        "the scan spinner should have cleared before the warning and redrawn after it"
    );

    remove_directory_link(&broken_link);
}
