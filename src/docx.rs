//! DOCX file processing module

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::common::{
    ExtractionConfig, ExtractionCounts, is_safe_archive_path, is_supported_extension,
};
use crate::image_writer::{ImageToWrite, WriteMode, write_images};
use crate::magic::detect_image_format;

#[derive(Debug)]
struct CandidateEntry {
    index: usize,
}

/// Identifies an image format using magic bytes first, then a supported extension fallback.
///
/// Returns the selected format and whether extension fallback was used. Unsupported fallback
/// extensions are ignored so generic entries like `.bin` do not create noisy warnings.
pub(crate) fn identify_image_format(data: &[u8], archive_name: &str) -> (Option<String>, bool) {
    if let Some(format) = detect_image_format(data) {
        return (Some(format.to_string()), false);
    }

    let Some(ext) = Path::new(archive_name).extension().and_then(|e| e.to_str()) else {
        return (None, false);
    };

    let ext_lower = ext.to_lowercase();
    if is_supported_extension(ext_lower.as_str()) {
        (Some(ext_lower), true)
    } else {
        (None, false)
    }
}

/// Processes a single .docx file, extracting images matching the allowed extensions.
/// When `convert` is specified, images are converted to the target format before writing.
/// GIF routing takes priority over conversion: GIFs routed to `gif_output` are written as-is.
/// Returns extraction counts including conversion and GIF routing information.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
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

        let (Some(format), used_fallback) = identify_image_format(&data, &archive_name) else {
            continue;
        };

        if used_fallback {
            eprintln!(
                "Warning: Magic detection failed for {}; falling back to .{} extension",
                archive_name, format
            );
        }

        if allowed_extensions.contains(format.as_str()) {
            images.push(ImageToWrite {
                data,
                extension: format,
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identify_image_format_prefers_magic_over_extension() {
        let (format, used_fallback) =
            identify_image_format(b"\x89PNG\r\n\x1A\n", "word/media/image.bin");
        assert_eq!(format.as_deref(), Some("png"));
        assert!(!used_fallback);
    }

    #[test]
    fn identify_image_format_falls_back_to_supported_extension() {
        let (format, used_fallback) = identify_image_format(b"unknown", "word/media/image.PNG");
        assert_eq!(format.as_deref(), Some("png"));
        assert!(used_fallback);
    }

    #[test]
    fn identify_image_format_ignores_unsupported_extension() {
        let (format, used_fallback) = identify_image_format(b"unknown", "word/media/image.bin");
        assert_eq!(format, None);
        assert!(!used_fallback);
    }
}
