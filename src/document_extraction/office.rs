//! Office document processing module.

use anyhow::Context;
use std::borrow::Cow;
use std::fs;
use std::path::Path;
use zip::ZipArchive;

use crate::image_write_pipeline::{ArchiveImageSource, ImageWriteOutcome, ImageWritePipeline};
use crate::office_package::OfficePackage;
use crate::output_placement::OutputPlacement;

/// Processes a single Office document, extracting the images in its package's
/// media folder that the requested Image formats accept.
///
/// Only entries inside [`OfficePackage::media_folder`] are offered to the Image
/// write pipeline, in ZIP order. Everything else in the archive, such as a
/// `docProps/thumbnail.jpeg` preview, is not a document image, so it is neither
/// written nor numbered nor warned about (ADR-0029). The configured pipeline owns
/// bounded source acquisition, image acceptance and output policy. Individual entry
/// failures become warnings; opening the document or its ZIP archive remains fatal.
///
/// # Errors
///
/// Returns an error when the input or ZIP archive cannot be opened, or when
/// collision-safe output emission cannot create or complete a file.
pub(super) fn process_file(
    input_path: &Path,
    package: OfficePackage,
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
            // decode. Beyond the eligibility check below, it is only a diagnostic
            // name, so an undecodable one falls back to the index exactly as a
            // missing one does; reading the entry
            // itself still reports any real failure through `unreadable`.
            let decoded_name = archive.name_for_index(index).and_then(Result::ok);

            // An entry whose name cannot be read cannot be shown to lie outside the
            // media folder, so it is still offered: passing over it would lose an
            // image on a guess, which ADR-0020 forbids. Its bytes then decide, as
            // they do for every entry inside the folder.
            if decoded_name
                .as_deref()
                .is_some_and(|name| !package.holds_document_image(name))
            {
                continue;
            }

            let source_name =
                decoded_name.map_or_else(|| format!("archive entry #{index}"), Cow::into_owned);
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
