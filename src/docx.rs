//! DOCX file processing module

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::common::{get_unique_output_path, is_safe_archive_path, write_image_to_file, ImageToExtract};

/// Processes a single .docx file, extracting images matching the allowed extensions.
/// Returns the number of images extracted.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
) -> Result<usize> {
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
        return Ok(0);
    }

    // create_dir_all is idempotent - succeeds if directory exists
    fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;

    let total_images = images.len();
    println!(
        "Found {} image files in {}.",
        total_images,
        input_path.display()
    );

    for (seq_index, image) in images.iter().enumerate() {
        let mut file = archive.by_index(image.index)?;

        let output_path = get_unique_output_path(
            output_base_dir,
            &doc_name,
            seq_index,
            total_images,
            &image.extension,
        )?;

        println!("Extracting to: {}", output_path.display());

        // Read archive entry into memory and use shared write function
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .context("Failed to read image from archive")?;

        write_image_to_file(&output_path, &data)?;
    }

    Ok(total_images)
}
