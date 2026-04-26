//! DOCX file processing module

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::common::{
    ExtractionConfig, ExtractionCounts, get_unique_output_path, is_safe_archive_path,
    is_supported_extension, write_image_to_file,
};
use crate::convert::{ConversionResult, try_convert};
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
            images.push((data, format));
        }
    }

    if images.is_empty() {
        return Ok(ExtractionCounts::default());
    }

    // create_dir_all is idempotent - succeeds if directory exists
    fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;

    let total_images = images.len();
    let mut counts = ExtractionCounts::default();
    let mut gif_dir_created = false;

    for (seq_index, (data, extension)) in images.into_iter().enumerate() {
        // Determine output directory: route GIFs to gif_output if set
        let is_gif = extension == "gif";
        let effective_output_dir = if let (true, Some(gif_dir)) = (is_gif, config.gif_output) {
            if !gif_dir_created {
                fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                gif_dir_created = true;
            }
            gif_dir
        } else {
            output_base_dir
        };

        // Determine if this GIF is being routed (GIF routing takes priority per D-10)
        let is_routed_gif = is_gif && config.gif_output.is_some();

        // Attempt conversion if requested and not a routed GIF
        let (final_data, final_ext) = if let Some(format) = config.convert {
            if is_routed_gif {
                // GIF is being routed to gif_output -- write as-is, skip conversion
                (data, extension.clone())
            } else {
                match try_convert(&data, &extension, format, config.quality, config.lossless) {
                    Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                        counts.converted += 1;
                        (converted_bytes, ext)
                    }
                    Ok(ConversionResult::Skipped(original_ext)) => {
                        eprintln!(
                            "Warning: Skipping conversion for {} ({} format not supported for conversion)",
                            doc_name, original_ext
                        );
                        counts.skipped += 1;
                        (data, original_ext)
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Conversion failed for image in {}: {}",
                            doc_name, e
                        );
                        counts.skipped += 1;
                        (data, extension.clone())
                    }
                }
            }
        } else {
            (data, extension.clone())
        };

        let output_path = get_unique_output_path(
            effective_output_dir,
            &doc_name,
            seq_index,
            total_images,
            &final_ext,
        )?;

        write_image_to_file(&output_path, &final_data)?;

        counts.extracted += 1;
        if is_routed_gif {
            counts.gifs_routed += 1;
        }
    }

    Ok(counts)
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
