//! Tests for DOCX file processing.

use super::*;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

fn temp_test_dir(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-docx-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

fn write_extension_fallback_docx(path: &Path) {
    let file = fs::File::create(path).expect("test DOCX should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("word/media/image1.png", SimpleFileOptions::default())
        .expect("zip entry should start");
    zip.write_all(b"not actually a png")
        .expect("zip entry payload should be writable");
    zip.finish().expect("zip archive should finish");
}

#[test]
fn returns_extension_fallback_warning_fact() {
    let temp_dir = temp_test_dir("extension-fallback-warning");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    write_extension_fallback_docx(&input_path);

    let result = process_file(
        &input_path,
        &output_dir,
        "sample",
        &ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        )),
    )
    .expect("DOCX extraction should succeed");

    assert_eq!(result.counts.extracted, 1);
    assert_eq!(
        result.warnings,
        vec![ImageWriteWarning::ExtensionFallback {
            source_name: "word/media/image1.png".to_string(),
            format: ImageFormat::Png,
        }]
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn preserves_zip_order_for_numbered_outputs() {
    let temp_dir = temp_test_dir("zip-order");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let mut first = b"\x89PNG\r\n\x1A\n".to_vec();
    first.push(1);
    let mut second = b"GIF89a".to_vec();
    second.push(2);
    let file = fs::File::create(&input_path).expect("test DOCX should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    for (name, data) in [
        ("word/media/z.png", first.as_slice()),
        ("word/document.xml", b"<document/>".as_slice()),
        ("word/media/a.gif", second.as_slice()),
    ] {
        zip.start_file(name, SimpleFileOptions::default())
            .expect("zip entry should start");
        zip.write_all(data)
            .expect("zip entry payload should be writable");
    }
    zip.finish().expect("zip archive should finish");

    let result = process_file(
        &input_path,
        &output_dir,
        "sample",
        &ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png, ImageFormat::Gif]),
            None,
            None,
        )),
    )
    .expect("DOCX extraction should succeed");

    assert_eq!(result.counts.extracted, 2);
    assert_eq!(fs::read(output_dir.join("sample_1.png")).unwrap(), first);
    assert_eq!(fs::read(output_dir.join("sample_2.gif")).unwrap(), second);

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
