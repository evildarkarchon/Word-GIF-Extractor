//! Tests for DOCX file processing.

use super::*;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
use crate::test_support::{temp_test_dir, write_docx, write_extension_fallback_docx};
use std::collections::HashSet;
use std::fs;

#[test]
fn returns_extension_fallback_warning_fact() {
    let temp_dir = temp_test_dir("docx", "extension-fallback-warning");
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
    let temp_dir = temp_test_dir("docx", "zip-order");
    let input_path = temp_dir.join("sample.docx");
    let output_dir = temp_dir.join("out");
    fs::create_dir_all(&output_dir).expect("output directory should be creatable");
    let mut first = b"\x89PNG\r\n\x1A\n".to_vec();
    first.push(1);
    let mut second = b"GIF89a".to_vec();
    second.push(2);
    write_docx(
        &input_path,
        &[
            ("word/media/z.png", first.as_slice()),
            ("word/document.xml", b"<document/>".as_slice()),
            ("word/media/a.gif", second.as_slice()),
        ],
    );

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
