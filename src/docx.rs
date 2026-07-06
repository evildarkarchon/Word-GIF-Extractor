//! DOCX file processing module

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::common::{ExtractionConfig, ExtractionCounts, is_safe_archive_path};
use crate::image_format::{FormatConfidence, ImageFormat};
use crate::image_writer::{ImageToWrite, WriteMode, write_images};

#[derive(Debug)]
struct CandidateEntry {
    index: usize,
}

/// Processes a single .docx file, extracting images matching the allowed extensions.
/// When `convert` is specified, images are converted to the target format before writing.
/// GIF routing takes priority over conversion: GIFs routed to `gif_output` are written as-is.
/// Returns extraction counts including conversion and GIF routing information.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_formats: &HashSet<ImageFormat>,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    let doc_name = input_path
        .file_stem()
        .context("Invalid filename")?
        .to_string_lossy()
        .to_string();

    let file = fs::File::open(input_path)
        .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;

    let mut candidates: Vec<CandidateEntry> = Vec::new();

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name();

        // Defense-in-depth: skip entries with path traversal patterns
        if !is_safe_archive_path(name) {
            continue;
        }

        candidates.push(CandidateEntry { index: i });
    }

    let mut images = Vec::new();
    for candidate in candidates {
        let mut file = archive.by_index(candidate.index)?;
        let archive_name = file.name().to_string();

        // Read archive entry into memory before deciding whether the format filter applies.
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .context("Failed to read image from archive")?;

        let Some(identified) = ImageFormat::identify(&data, &archive_name) else {
            continue;
        };
        let format = identified.format;

        if identified.confidence == FormatConfidence::ExtensionFallback {
            eprintln!(
                "Warning: Magic detection failed for {}; falling back to .{} extension",
                archive_name,
                format.extension()
            );
        }

        if allowed_formats.contains(&format) {
            images.push(ImageToWrite { data, format });
        }
    }

    write_images(
        output_base_dir,
        &doc_name,
        images,
        config,
        WriteMode::BatchImages,
    )
}
