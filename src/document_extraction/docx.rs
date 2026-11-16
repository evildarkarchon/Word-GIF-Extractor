//! DOCX file processing module

use anyhow::Context;
use std::borrow::Cow;
use std::fs;
use std::path::Path;
use zip::ZipArchive;

use crate::image_write_pipeline::{ArchiveImageSource, ImageWriteOutcome, ImageWritePipeline};
use crate::output_placement::OutputPlacement;

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
    placement: &OutputPlacement,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let file = fs::File::open(input_path)
        .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;

    pipeline.write_from(placement, |visitor| {
        for index in 0..archive.len() {
            // `zip` decodes entry names lazily, so a name can be present yet fail to
            // decode. It is only a diagnostic name here, so an undecodable one falls
            // back to the index exactly as a missing one does; reading the entry
            // itself still reports any real failure through `unreadable`.
            let source_name = archive
                .name_for_index(index)
                .and_then(Result::ok)
                .map_or_else(|| format!("archive entry #{index}"), Cow::into_owned);
            let source = ArchiveImageSource::named(source_name);

            match archive.by_index(index) {
                Ok(mut entry) => visitor.visit(source, &mut entry)?,
                Err(error) => visitor.unreadable(source, error),
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
