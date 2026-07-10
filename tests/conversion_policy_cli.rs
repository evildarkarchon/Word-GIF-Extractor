use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, Rgb, RgbImage};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

/// Creates a temporary directory unique to this test process.
fn temp_test_dir(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-conversion-policy-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

/// Encodes a small valid JPEG whose bytes can be compared after extraction.
fn test_jpeg() -> Vec<u8> {
    let mut image = RgbImage::new(8, 8);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = Rgb([(x * 31) as u8, (y * 29) as u8, ((x + y) * 17) as u8]);
    }

    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 95)
        .encode_image(&DynamicImage::ImageRgb8(image))
        .expect("test JPEG should encode");
    bytes
}

/// Writes a minimal DOCX-like ZIP archive containing one JPEG resource.
fn write_docx_with_jpeg(path: &Path, jpeg: &[u8]) {
    let file = fs::File::create(path).expect("test DOCX should be creatable");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("word/media/image1.jpg", SimpleFileOptions::default())
        .expect("JPEG archive entry should start");
    archive
        .write_all(jpeg)
        .expect("JPEG archive entry should be writable");
    archive.finish().expect("test DOCX should finish");
}

/// Runs the extractor with a JPEG conversion request and optional quality.
fn run_jpeg_conversion(input: &Path, output_dir: &Path, quality: Option<u8>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_word-image-extractor"));
    command
        .arg(input)
        .arg("--output")
        .arg(output_dir)
        .arg("--formats")
        .arg("jpg")
        .arg("--convert")
        .arg("jpg");
    if let Some(quality) = quality {
        command.arg("--quality").arg(quality.to_string());
    }
    command.output().expect("extractor binary should run")
}

#[test]
fn matching_jpeg_is_preserved_when_quality_is_implicit() {
    let temp_dir = temp_test_dir("implicit");
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be creatable");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let input_path = input_dir.join("sample.docx");
    let original = test_jpeg();
    write_docx_with_jpeg(&input_path, &original);

    let output = run_jpeg_conversion(&input_path, &output_dir, None);

    assert!(
        output.status.success(),
        "extractor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(output_dir.join("sample.jpg")).expect("output JPEG should be readable"),
        original
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn matching_jpeg_is_reencoded_when_quality_is_explicit() {
    let temp_dir = temp_test_dir("explicit");
    let input_dir = temp_dir.join("input");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&input_dir).expect("input directory should be creatable");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let input_path = input_dir.join("sample.docx");
    let original = test_jpeg();
    write_docx_with_jpeg(&input_path, &original);

    let output = run_jpeg_conversion(&input_path, &output_dir, Some(70));

    assert!(
        output.status.success(),
        "extractor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let converted =
        fs::read(output_dir.join("sample.jpg")).expect("output JPEG should be readable");
    assert_ne!(converted, original);
    image::load_from_memory(&converted).expect("converted JPEG should remain decodable");

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
