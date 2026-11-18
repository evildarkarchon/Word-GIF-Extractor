//! The Image size limit and streamed writes, through a whole run over a real ZIP.
//!
//! The audit's M6 fixture was one entry of 512 MiB that deflated to 522 KB. That is
//! too slow to build in a test, so the limit is lowered to fit small fixtures
//! instead: what is under test is how the run treats an entry past the limit, not
//! the default value, which intake's tests pin.

mod support;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use support::{MINIMAL_PNG, run_captured, temp_test_dir, write_docx};

/// Returns a PNG-identified payload of `len` bytes whose tail is hard to compress,
/// so the archive entry is roughly its own size and the reader does real work.
fn png_payload(len: usize, seed: u32) -> Vec<u8> {
    let mut payload = MINIMAL_PNG.to_vec();
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    while payload.len() < len {
        // A linear congruential generator: deterministic, and noisy enough that
        // deflate cannot collapse the payload.
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        payload.push((state >> 24) as u8);
    }
    payload
}

/// Returns the sorted file names directly inside `directory`.
fn file_names_in(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .expect("output directory should be readable")
        .map(|entry| {
            entry
                .expect("directory entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Verifies an image past the limit is a warning, writes nothing, and does not
/// number the document's other image.
///
/// The oversize image is the second one, so it arrives while the first is held back
/// waiting for its name. The first must still be written under the singular name,
/// which is the order ADR-0030 chose streaming writes for.
#[test]
fn image_past_the_limit_is_skipped_with_a_warning_and_leaves_the_other_singular() {
    let temp_dir = temp_test_dir("image-size-limit", "skipped");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let document_path = temp_dir.join("limited.docx");
    write_docx(
        &document_path,
        &[
            ("word/media/image1.png", MINIMAL_PNG),
            ("word/media/image2.png", &png_payload(64 * 1024, 2)),
        ],
    );

    let (result, capture) = run_captured(&[
        document_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
        "--max-image-size",
        "16K",
    ]);

    // A skipped image is a warning, not a failure (ADR-0014).
    assert_eq!(
        result.expect("intake should accept a size limit"),
        ExitCode::SUCCESS
    );
    assert_eq!(file_names_in(&output_dir), ["limited.png"]);
    assert_eq!(
        fs::read(output_dir.join("limited.png")).unwrap(),
        MINIMAL_PNG
    );

    let stderr = capture.stderr();
    let stderr_lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(
        stderr_lines,
        [format!(
            "Warning: {}: Skipping word/media/image2.png: larger than the 16 KiB image size limit (raise it with --max-image-size)",
            document_path.display()
        )],
        "unexpected standard error: {stderr}"
    );
}

/// Verifies images streamed from a real ZIP reader land byte for byte under their
/// numbered names.
///
/// Every image after the first is copied from the archive entry to its file in
/// chunks rather than buffered, and the archive's own CRC check runs on that read,
/// so a payload that streamed wrongly would either differ here or fail to read.
#[test]
fn large_images_are_streamed_to_their_files_byte_for_byte() {
    let temp_dir = temp_test_dir("image-size-limit", "streamed");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let document_path = temp_dir.join("streamed.docx");
    let payloads = [
        png_payload(40_000, 1),
        png_payload(1_500_000, 2),
        png_payload(1_500_000, 3),
    ];
    write_docx(
        &document_path,
        &[
            ("word/media/image1.png", &payloads[0]),
            ("word/media/image2.png", &payloads[1]),
            ("word/media/image3.png", &payloads[2]),
        ],
    );

    let (result, capture) = run_captured(&[
        document_path.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
    ]);

    assert_eq!(
        result.expect("intake should accept a named input"),
        ExitCode::SUCCESS
    );
    assert_eq!(capture.stderr(), "", "unexpected standard error");
    assert_eq!(
        file_names_in(&output_dir),
        ["streamed_1.png", "streamed_2.png", "streamed_3.png"]
    );
    for (index, payload) in payloads.iter().enumerate() {
        let name = format!("streamed_{}.png", index + 1);
        assert!(
            fs::read(output_dir.join(&name)).unwrap() == *payload,
            "{name} should hold its entry's bytes exactly"
        );
    }
}
