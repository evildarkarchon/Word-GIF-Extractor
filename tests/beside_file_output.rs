//! Where extracted images land when no output directory is requested.
//!
//! This file holds exactly one test on purpose. The test moves the process working
//! directory, which every test in a binary shares, and each file under `tests/` is its
//! own binary — so one test per file is what keeps that move from racing anything.

mod support;

use std::fs;

use support::{
    has_file_with_extension, run_captured, temp_test_dir, with_current_dir, write_png_docx,
};

#[test]
fn extracts_beside_input_when_output_omitted() {
    let temp_dir = temp_test_dir("beside-output", "omitted-output");
    let cwd = temp_dir.join("cwd");
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&cwd).expect("cwd directory should be created");
    fs::create_dir_all(&input_dir).expect("input directory should be created");

    let docx_path = input_dir.join("sample.docx");
    write_png_docx(&docx_path);

    // "Beside the input" is only a claim about anything if the working directory is
    // somewhere else, so the run happens from a third directory that is neither.
    let (result, capture) = with_current_dir(&cwd, |_| {
        run_captured(&[docx_path.to_string_lossy().as_ref(), "--formats", "png"])
    });

    result.expect("intake should accept a named input and a format filter");
    assert!(
        has_file_with_extension(&input_dir, "png"),
        "expected PNG beside input in {}",
        input_dir.display()
    );
    assert!(
        !has_file_with_extension(&cwd, "png"),
        "did not expect PNG in the working directory {}",
        cwd.display()
    );
    // A produced outcome ends on the progress display rather than on either text
    // stream, so a run that says nothing is a run that extracted without complaint.
    assert_eq!(capture.stdout(), "", "unexpected standard output");
    assert_eq!(capture.stderr(), "", "unexpected standard error");
    assert!(
        capture.writes() > 0,
        "the run summary should have been drawn on the progress display"
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
