//! EPUB file processing module

use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::common::{
    get_unique_output_path, is_safe_archive_path, sanitize_filename, write_image_to_file,
};

/// Filter criteria for EPUB files
#[derive(Debug, Default)]
pub struct EpubFilter {
    pub title: Option<String>,
    pub author: Option<String>,
}

impl EpubFilter {
    /// Returns true if no filter criteria are set
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.author.is_none()
    }
}

/// Checks if EPUB metadata matches the filter (case-insensitive substring match)
/// Returns (matches, title, author) - title/author are returned for skip messaging
fn matches_filter(title: Option<&str>, author: Option<&str>, filter: &EpubFilter) -> bool {
    let title_matches = filter
        .title
        .as_ref()
        .is_none_or(|f| title.is_some_and(|t| t.to_lowercase().contains(&f.to_lowercase())));

    let author_matches = filter
        .author
        .as_ref()
        .is_none_or(|f| author.is_some_and(|a| a.to_lowercase().contains(&f.to_lowercase())));

    title_matches && author_matches
}

/// Formats a filename based on EPUB metadata (author and title)
/// Falls back to the provided fallback name if metadata is missing
fn format_epub_base_name(author: Option<&str>, title: Option<&str>, fallback: &str) -> String {
    let author = author.map(|s| s.trim()).filter(|s| !s.is_empty());
    let title = title.map(|s| s.trim()).filter(|s| !s.is_empty());

    let raw_name = match (author, title) {
        (Some(a), Some(t)) => format!("{} - {}", a, t),
        (None, Some(t)) => t.to_string(),
        (Some(a), None) => a.to_string(),
        (None, None) => fallback.to_string(),
    };

    sanitize_filename(&raw_name)
}

/// Struct to hold image data extracted from EPUB
struct EpubImage {
    id: String,
    extension: String,
}

/// Processes a single .epub file, extracting images matching the allowed extensions.
/// Uses author and title metadata for naming, falling back to filename.
/// If cover_only is true, only extracts the cover image.
/// If cover_fallback is true and cover_only is true but no cover is found, extracts all images.
/// If a filter is provided, only processes files matching the filter criteria.
/// Returns the number of images extracted.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    filter: &EpubFilter,
) -> Result<usize> {
    let fallback_name = input_path
        .file_stem()
        .context("Invalid filename")?
        .to_string_lossy()
        .to_string();

    let mut doc =
        EpubDoc::new(input_path).map_err(|e| anyhow::anyhow!("Failed to open EPUB file: {}", e))?;

    // Extract metadata - mdata() returns Option<MetadataItem> with .value field
    let title = doc.mdata("title").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone()); // 'creator' is the Dublin Core element for author

    // Check filter if any criteria are set - silently skip non-matching files
    if !filter.is_empty() && !matches_filter(title.as_deref(), author.as_deref(), filter) {
        return Ok(0);
    }

    let base_name = format_epub_base_name(author.as_deref(), title.as_deref(), &fallback_name);

    // Print metadata info
    if let Some(ref t) = title {
        println!("EPUB Title: {}", t);
    }
    if let Some(ref a) = author {
        println!("EPUB Author: {}", a);
    }

    if cover_only {
        return extract_cover_only(
            &mut doc,
            output_base_dir,
            &base_name,
            allowed_extensions,
            input_path,
            cover_fallback,
        );
    }

    extract_all_images(
        &mut doc,
        output_base_dir,
        &base_name,
        allowed_extensions,
        input_path,
    )
}

/// Extracts all images from an EPUB file
fn extract_all_images(
    doc: &mut EpubDoc<std::io::BufReader<std::fs::File>>,
    output_base_dir: &Path,
    base_name: &str,
    allowed_extensions: &HashSet<&str>,
    input_path: &Path,
) -> Result<usize> {
    // Collect images from resources
    // resources is HashMap<String, ResourceItem> where ResourceItem has path and mime fields
    let mut images: Vec<EpubImage> = Vec::new();

    // Clone the resource keys and extract info to avoid borrow issues
    let resources: Vec<(String, String)> = doc
        .resources
        .iter()
        .filter_map(|(id, item)| {
            // Defense-in-depth: validate resource paths
            let path_str = item.path.to_string_lossy();
            if !is_safe_archive_path(&path_str) {
                return None;
            }

            // Only keep images
            if !item.mime.starts_with("image/") {
                return None;
            }

            // Try to get extension from path first, then from mime
            let ext = item
                .path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_lowercase())
                .or_else(|| mime_to_extension(&item.mime));

            ext.map(|e| (id.clone(), e))
        })
        .collect::<Vec<(String, String)>>();

    for (id, extension) in resources {
        // Check if this extension is in our allowed list
        if allowed_extensions.contains(extension.as_str()) {
            images.push(EpubImage { id, extension });
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
        // Get the image data - get_resource returns Option<(Vec<u8>, String)>
        let (data, _mime) = doc
            .get_resource(&image.id)
            .ok_or_else(|| anyhow::anyhow!("Failed to get resource '{}'", image.id))?;

        let output_path = get_unique_output_path(
            output_base_dir,
            base_name,
            seq_index,
            total_images,
            &image.extension,
        )?;

        println!("Extracting to: {}", output_path.display());

        write_image_to_file(&output_path, &data)?;
    }

    Ok(total_images)
}

/// Extracts only the cover image from an EPUB file
/// If cover_fallback is true and no cover is found, extracts all images instead
fn extract_cover_only(
    doc: &mut EpubDoc<std::io::BufReader<std::fs::File>>,
    output_base_dir: &Path,
    base_name: &str,
    allowed_extensions: &HashSet<&str>,
    input_path: &Path,
    cover_fallback: bool,
) -> Result<usize> {
    // Try to get the cover image using the epub crate's get_cover method
    let cover = doc.get_cover();

    match cover {
        Some((data, mime)) => {
            // Determine the extension from the MIME type
            let extension = mime_to_extension(&mime).unwrap_or_else(|| "jpg".to_string());

            // Check if this extension is in our allowed list
            if !allowed_extensions.contains(extension.as_str()) {
                println!(
                    "Cover image format '{}' not in allowed formats, skipping.",
                    extension
                );
                return Ok(0);
            }

            // create_dir_all is idempotent - succeeds if directory exists
            fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;

            // Use just the base name (author/title) for cover-only mode
            let output_path = get_unique_output_path(output_base_dir, base_name, 0, 1, &extension)?;

            println!(
                "Extracting cover from {} to: {}",
                input_path.display(),
                output_path.display()
            );

            write_image_to_file(&output_path, &data)?;

            Ok(1)
        }
        None => {
            if cover_fallback {
                println!(
                    "No cover image found in {}, falling back to extracting all images.",
                    input_path.display()
                );
                extract_all_images(
                    doc,
                    output_base_dir,
                    base_name,
                    allowed_extensions,
                    input_path,
                )
            } else {
                println!("No cover image found in {}", input_path.display());
                Ok(0)
            }
        }
    }
}

/// Converts a MIME type to a file extension
fn mime_to_extension(mime: &str) -> Option<String> {
    match mime {
        "image/jpeg" => Some("jpg".to_string()),
        "image/png" => Some("png".to_string()),
        "image/gif" => Some("gif".to_string()),
        "image/bmp" => Some("bmp".to_string()),
        "image/webp" => Some("webp".to_string()),
        "image/svg+xml" => Some("svg".to_string()),
        "image/tiff" => Some("tiff".to_string()),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico".to_string()),
        "image/x-emf" | "image/emf" => Some("emf".to_string()),
        "image/x-wmf" | "image/wmf" => Some("wmf".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_epub_base_name_both() {
        let result = format_epub_base_name(Some("Stephen King"), Some("The Shining"), "fallback");
        assert_eq!(result, "Stephen King - The Shining");
    }

    #[test]
    fn test_format_epub_base_name_title_only() {
        let result = format_epub_base_name(None, Some("The Shining"), "fallback");
        assert_eq!(result, "The Shining");
    }

    #[test]
    fn test_format_epub_base_name_author_only() {
        let result = format_epub_base_name(Some("Stephen King"), None, "fallback");
        assert_eq!(result, "Stephen King");
    }

    #[test]
    fn test_format_epub_base_name_neither() {
        let result = format_epub_base_name(None, None, "fallback");
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_format_epub_base_name_empty_strings() {
        let result = format_epub_base_name(Some("  "), Some(""), "fallback");
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_format_epub_base_name_sanitizes() {
        let result = format_epub_base_name(Some("Author/Name"), Some("Title:Subtitle"), "fallback");
        assert_eq!(result, "Author_Name - Title_Subtitle");
    }

    #[test]
    fn test_mime_to_extension() {
        assert_eq!(mime_to_extension("image/jpeg"), Some("jpg".to_string()));
        assert_eq!(mime_to_extension("image/png"), Some("png".to_string()));
        assert_eq!(mime_to_extension("image/gif"), Some("gif".to_string()));
        assert_eq!(mime_to_extension("image/unknown"), None);
    }
}
