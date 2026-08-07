//! Conversion policy behaviour for a source image that already matches the target.

mod support;

use std::fs;
use std::path::Path;

use anyhow::Result;
use word_image_extractor::Capture;

use support::{run_captured, temp_test_dir, test_jpeg, write_docx};

/// Runs one extraction that converts to JPEG, with an optional quality override.
fn run_jpeg_conversion(
    input: &Path,
    output_dir: &Path,
    quality: Option<u8>,
) -> (Result<()>, Capture) {
    // The three owned values are bound before the argument list so they outlive the
    // borrows in it.
    let input = input.to_string_lossy();
    let output_dir = output_dir.to_string_lossy();
    let quality = quality.map(|quality| quality.to_string());

    let mut arguments = vec![
        input.as_ref(),
        "--output",
        output_dir.as_ref(),
        "--formats",
        "jpg",
        "--convert",
        "jpg",
    ];
    if let Some(quality) = &quality {
        arguments.extend(["--quality", quality.as_str()]);
    }

    run_captured(&arguments)
}

#[test]
fn matching_jpeg_is_preserved_when_quality_is_implicit() {
    let temp_dir = temp_test_dir("conversion-policy", "implicit");
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be creatable");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let input_path = input_dir.join("sample.docx");
    let original = test_jpeg();
    write_docx(&input_path, &[("word/media/image1.jpg", &original)]);

    let (result, capture) = run_jpeg_conversion(&input_path, &output_dir, None);

    result.expect("intake should accept a JPEG conversion with no quality named");
    assert_eq!(capture.stderr(), "", "unexpected standard error");
    assert_eq!(
        fs::read(output_dir.join("sample.jpg")).expect("output JPEG should be readable"),
        original
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn matching_jpeg_is_reencoded_when_quality_is_explicit() {
    let temp_dir = temp_test_dir("conversion-policy", "explicit");
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be creatable");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let input_path = input_dir.join("sample.docx");
    let original = test_jpeg();
    write_docx(&input_path, &[("word/media/image1.jpg", &original)]);

    let (result, capture) = run_jpeg_conversion(&input_path, &output_dir, Some(70));

    result.expect("intake should accept a JPEG conversion at an explicit quality");
    assert_eq!(capture.stderr(), "", "unexpected standard error");
    let converted =
        fs::read(output_dir.join("sample.jpg")).expect("output JPEG should be readable");
    assert_ne!(converted, original);
    image::load_from_memory(&converted).expect("converted JPEG should remain decodable");

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies a policy `clap` accepts but intake refuses arrives as the returned error.
///
/// This is the other half of the seam, and the one place the split matters. A quality
/// override against PNG parses cleanly — the two flags are individually valid and `clap`
/// has no rule tying them together — so the refusal happens in Extraction run intake,
/// after parsing and before anything renders. Its wording travels as the returned error
/// and never reaches the destination, because printing a returned error is the process
/// exit path's job, so the capture stays empty on both streams.
#[test]
fn quality_against_png_is_refused_through_the_returned_error() {
    let (result, capture) = run_captured(&["--convert", "png", "--quality", "90"]);

    let error = result.expect_err("a quality override against PNG should fail intake");
    assert!(
        error
            .to_string()
            .contains("--quality cannot be used with --convert png"),
        "error was: {error}"
    );
    assert_eq!(capture.stdout(), "", "intake wording must not be captured");
    assert_eq!(capture.stderr(), "", "intake wording must not be captured");
}
