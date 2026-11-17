//! EPUB deduplication as a whole run handles it.
//!
//! Document selection's tests pin the duplicate key against declared EPUB
//! declarations; this file pins the three cases the 2026-11-14 audit (H2) reproduced
//! against real archives, where a distinct book was dropped without a word, and the
//! two user-visible halves of the fix: a skipped duplicate is named, and
//! `--keep-duplicates` turns deduplication off.

mod support;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use support::{run_captured, sorted_file_names, temp_test_dir, write_declared_epub};

/// Runs the extractor over `input` into `output` with any extra flags, returning
/// standard error after asserting the run succeeded.
fn run_into(input: &Path, output: &Path, extra: &[&str]) -> String {
    let mut arguments = vec![
        input.to_string_lossy().into_owned(),
        "--output".to_string(),
        output.to_string_lossy().into_owned(),
    ];
    arguments.extend(extra.iter().map(|flag| flag.to_string()));
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();

    let (result, capture) = run_captured(&arguments);

    let exit = result.expect("intake should accept the run");
    assert_eq!(exit, ExitCode::SUCCESS, "the run should succeed");
    capture.stderr()
}

/// Verifies every issue of a serial is extracted, though all share a creator and title.
#[test]
fn extracts_every_issue_of_a_serial_that_shares_a_creator_and_title() {
    let temp_dir = temp_test_dir("epub-deduplication", "serial");
    let input_dir = temp_dir.join("serial");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    for issue in ["weekly-2026-01", "weekly-2026-02", "weekly-2026-03"] {
        write_declared_epub(
            &input_dir.join(format!("{issue}.epub")),
            Some(&format!("urn:uuid:{issue}")),
            Some("The Weekly"),
            Some("The Weekly"),
        );
    }

    let stderr = run_into(&input_dir, &output_dir, &[]);

    assert_eq!(
        sorted_file_names(&output_dir),
        [
            "The Weekly - The Weekly.png",
            "The Weekly - The Weekly_1.png",
            "The Weekly - The Weekly_2.png",
        ]
    );
    assert_eq!(stderr, "", "no issue should be reported as a duplicate");
}

/// Verifies two books that declare only the same creator are both extracted.
#[test]
fn extracts_every_book_that_declares_only_the_same_creator() {
    let temp_dir = temp_test_dir("epub-deduplication", "creator-only");
    let input_dir = temp_dir.join("creator_only");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    for name in ["first", "second"] {
        write_declared_epub(
            &input_dir.join(format!("{name}.epub")),
            None,
            Some("Jane Doe"),
            None,
        );
    }

    let stderr = run_into(&input_dir, &output_dir, &[]);

    assert_eq!(
        sorted_file_names(&output_dir),
        ["Jane Doe.png", "Jane Doe_1.png"]
    );
    assert_eq!(stderr, "");
}

/// Verifies two undeclared EPUBs with one file name in different directories are both
/// extracted, each beside its own source.
#[test]
fn extracts_undeclared_epubs_that_share_a_file_name_in_different_directories() {
    let temp_dir = temp_test_dir("epub-deduplication", "same-name");
    let first_dir = temp_dir.join("first");
    let second_dir = temp_dir.join("second");
    fs::create_dir_all(&first_dir).expect("first directory should be created");
    fs::create_dir_all(&second_dir).expect("second directory should be created");
    write_declared_epub(&first_dir.join("book.epub"), None, None, None);
    write_declared_epub(&second_dir.join("book.epub"), None, None, None);

    let (result, capture) = run_captured(&[
        first_dir.join("book.epub").to_string_lossy().as_ref(),
        second_dir.join("book.epub").to_string_lossy().as_ref(),
    ]);

    result.expect("intake should accept two named EPUBs");
    assert_eq!(sorted_file_names(&first_dir), ["book.epub", "book.png"]);
    assert_eq!(sorted_file_names(&second_dir), ["book.epub", "book.png"]);
    assert_eq!(capture.stderr(), "");
}

/// Verifies a true copy is skipped with a warning naming both files, and that
/// `--keep-duplicates` extracts it after all.
#[test]
fn names_a_skipped_copy_and_keeps_it_when_asked() {
    let temp_dir = temp_test_dir("epub-deduplication", "copy");
    let input_dir = temp_dir.join("library");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    let original = input_dir.join("a-original.epub");
    let copy = input_dir.join("b-copy.epub");
    for path in [&original, &copy] {
        write_declared_epub(
            path,
            Some("urn:isbn:9780000000000"),
            Some("Stephen King"),
            Some("The Shining"),
        );
    }

    let deduplicated_dir = temp_dir.join("deduplicated");
    let stderr = run_into(&input_dir, &deduplicated_dir, &[]);

    assert_eq!(
        sorted_file_names(&deduplicated_dir),
        ["Stephen King - The Shining.png"]
    );
    assert_eq!(
        stderr.lines().collect::<Vec<_>>(),
        [format!(
            "Warning: Skipped {}: it declares the same book as {} (use --keep-duplicates to extract both)",
            copy.display(),
            original.display()
        )]
    );

    let kept_dir = temp_dir.join("kept");
    let stderr = run_into(&input_dir, &kept_dir, &["--keep-duplicates"]);

    assert_eq!(
        sorted_file_names(&kept_dir),
        [
            "Stephen King - The Shining.png",
            "Stephen King - The Shining_1.png",
        ]
    );
    assert_eq!(stderr, "");
}
