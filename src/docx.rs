//! DOCX file processing module

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::common::{
    ExtractionConfig, ExtractionCounts, ImageToExtract, get_unique_output_path,
    is_safe_archive_path, write_image_to_file,
};
use crate::convert::{ConversionResult, try_convert};

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

    let mut images: Vec<ImageToExtract> = Vec::new();

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name();

        // Defense-in-depth: skip entries with path traversal patterns
        if !is_safe_archive_path(name) {
            continue;
        }

        // Check if file has an extension and if it's in our allowed list
        if let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if allowed_extensions.contains(ext_lower.as_str()) {
                images.push(ImageToExtract {
                    index: i,
                    extension: ext_lower,
                });
            }
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

    for (seq_index, image) in images.iter().enumerate() {
        let mut file = archive.by_index(image.index)?;

        // Determine output directory: route GIFs to gif_output if set
        let is_gif = image.extension == "gif";
        let effective_output_dir = if let (true, Some(gif_dir)) = (is_gif, config.gif_output) {
            if !gif_dir_created {
                fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                gif_dir_created = true;
            }
            gif_dir
        } else {
            output_base_dir
        };

        // Read archive entry into memory
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .context("Failed to read image from archive")?;

        // Determine if this GIF is being routed (GIF routing takes priority per D-10)
        let is_routed_gif = is_gif && config.gif_output.is_some();

        // Attempt conversion if requested and not a routed GIF
        let (final_data, final_ext) = if let Some(format) = config.convert {
            if is_routed_gif {
                // GIF is being routed to gif_output -- write as-is, skip conversion
                (data, image.extension.clone())
            } else {
                match try_convert(&data, &image.extension, format, config.quality, config.lossless) {
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
                        (data, image.extension.clone())
                    }
                }
            }
        } else {
            (data, image.extension.clone())
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
