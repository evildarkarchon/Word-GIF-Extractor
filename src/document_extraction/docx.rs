//! DOCX file processing module

use anyhow::Context;
use std::fs;
use std::path::Path;
use zip::ZipArchive;

use crate::image_write_pipeline::{
    ArchiveImageSource, ImageWriteOutcome, ImageWritePipeline, ImageWriteRequest,
};

/// Processes a single .docx file, extracting images accepted by the requested Image formats.
/// Uses the selected document base name for output files.
/// The configured Image write pipeline owns bounded source acquisition, image
/// acceptance, and output policy. Individual entry failures become warnings;
/// opening the document or its ZIP archive remains fatal.
///
/// # Errors
///
/// Returns an error when the input or ZIP archive cannot be opened, or when
/// collision-safe output emission cannot create or complete a file.
pub(super) fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let file = fs::File::open(input_path)
        .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;

    pipeline.write_from(
        ImageWriteRequest::normal_images(output_base_dir, base_name),
        |visitor| {
            for index in 0..archive.len() {
                let source_name = archive
                    .name_for_index(index)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("archive entry #{index}"));
                let source = ArchiveImageSource::named(source_name);

                match archive.by_index(index) {
                    Ok(mut entry) => visitor.visit(source, &mut entry)?,
                    Err(error) => visitor.unreadable(source, error),
                }
            }
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests;
