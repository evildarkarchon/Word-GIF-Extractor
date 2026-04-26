use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

/// Creates a temporary directory unique to this test process.
fn temp_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-mislabeled-docx-{}-{nanos}",
        std::process::id()
    ))
}

/// Writes a minimal DOCX-like ZIP archive with a PNG payload using a `.bin` entry name.
fn write_mislabeled_docx(path: &Path) {
    let file = fs::File::create(path).expect("test archive should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("word/media/image1.bin", SimpleFileOptions::default())
        .expect("zip entry should start");
    zip.write_all(MINIMAL_PNG)
        .expect("zip entry payload should be writable");
    zip.finish().expect("zip archive should finish");
}

#[test]
fn extracts_mislabeled_png_when_filtering_for_png() {
    let temp_dir = temp_test_dir();
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be created");
    fs::create_dir_all(&output_dir).expect("output directory should be created");

    let docx_path = input_dir.join("sample.docx");
    write_mislabeled_docx(&docx_path);

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .arg(&docx_path)
        .arg("--output")
        .arg(&output_dir)
        .arg("--formats")
        .arg("png")
        .output()
        .expect("extractor binary should run");

    assert!(
        output.status.success(),
        "extractor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let png_outputs: Vec<_> = fs::read_dir(&output_dir)
        .expect("output directory should be readable")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "png")
        })
        .collect();
    assert!(
        !png_outputs.is_empty(),
        "expected at least one PNG output in {}",
        output_dir.display()
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
