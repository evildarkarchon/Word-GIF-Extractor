//! DOCX file processing module

use anyhow::{Context, Result};
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::image_write_pipeline::{
    ArchiveImageSource, ImageWritePipeline, ImageWriteRequest, ImageWriteResult,
};

/// Processes a single .docx file, extracting images accepted by the requested Image formats.
/// Uses the selected document base name for output files.
/// The configured Image write pipeline owns image acceptance and output policy.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> Result<ImageWriteResult> {
    let file = fs::File::open(input_path)
        .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;

    let mut sources = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let archive_name = file.name().to_string();

        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .context("Failed to read image from archive")?;

        sources.push(ArchiveImageSource::named(data, archive_name));
    }

    pipeline.write(ImageWriteRequest::normal_images(
        output_base_dir,
        base_name,
        sources,
    ))
}

#[cfg(test)]
mod tests {
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
}
