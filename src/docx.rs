//! DOCX file processing module

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::common::{
    ExtractionCounts, ImageToExtract, get_unique_output_path, is_safe_archive_path,
    write_image_to_file,
};

/// Processes a single .docx file, extracting images matching the allowed extensions.
/// Returns extraction counts including GIF routing information.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    gif_output: Option<&Path>,
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
        let effective_output_dir = if let (true, Some(gif_dir)) = (is_gif, gif_output) {
            if !gif_dir_created {
                fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                gif_dir_created = true;
            }
            gif_dir
        } else {
            output_base_dir
        };

        let output_path = get_unique_output_path(
            effective_output_dir,
            &doc_name,
            seq_index,
            total_images,
            &image.extension,
        )?;

        // Read archive entry into memory and use shared write function
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .context("Failed to read image from archive")?;

        write_image_to_file(&output_path, &data)?;

        counts.extracted += 1;
        if is_gif && gif_output.is_some() {
            counts.gifs_routed += 1;
        }
    }

    Ok(counts)
}
