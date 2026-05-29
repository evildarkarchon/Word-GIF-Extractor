use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

fn temp_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-beside-output-{}-{nanos}",
        std::process::id()
    ))
}

fn write_minimal_docx(path: &Path) {
    let file = fs::File::create(path).expect("test archive should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("word/media/image1.png", SimpleFileOptions::default())
        .expect("zip entry should start");
    zip.write_all(MINIMAL_PNG)
        .expect("zip entry payload should be writable");
    zip.finish().expect("zip archive should finish");
}

fn has_png_files(dir: &Path) -> bool {
    fs::read_dir(dir)
        .expect("directory should be readable")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "png")
        })
}

#[test]
fn extracts_beside_input_when_output_omitted() {
    let temp_dir = temp_test_dir();
    let cwd = temp_dir.join("cwd");
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&cwd).expect("cwd directory should be created");
    fs::create_dir_all(&input_dir).expect("input directory should be created");

    let docx_path = input_dir.join("sample.docx");
    write_minimal_docx(&docx_path);

    let output = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"))
        .current_dir(&cwd)
        .arg(&docx_path)
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

    assert!(
        has_png_files(&input_dir),
        "expected PNG beside input in {}",
        input_dir.display()
    );
    assert!(
        !has_png_files(&cwd),
        "did not expect PNG in process cwd {}",
        cwd.display()
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
