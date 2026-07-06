//! EPUB file processing module

use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use std::collections::HashSet;
use std::path::Path;

use crate::common::{ExtractionConfig, ExtractionCounts, is_safe_archive_path, sanitize_filename};
use crate::image_format::ImageFormat;
use crate::image_writer::{ImageToWrite, WriteMode, write_images};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

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

    /// Returns a human-readable description of the filter for progress messages
    pub fn description(&self) -> String {
        match (&self.author, &self.title) {
            (Some(author), Some(title)) => format!("author '{}' and title '{}'", author, title),
            (Some(author), None) => format!("author '{}'", author),
            (None, Some(title)) => format!("title '{}'", title),
            (None, None) => String::new(),
        }
    }
}

/// Checks if EPUB metadata matches the filter (case-insensitive substring match)
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

/// Checks if an EPUB file matches the given filter without extracting any images.
/// Returns true if the file matches, false otherwise.
/// Returns an error if the file cannot be opened or read.
pub fn check_filter_match(input_path: &Path, filter: &EpubFilter) -> Result<bool> {
    let doc =
        EpubDoc::new(input_path).map_err(|e| anyhow::anyhow!("Failed to open EPUB file: {}", e))?;

    let title = doc.mdata("title").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone());

    Ok(matches_filter(title.as_deref(), author.as_deref(), filter))
}

/// Gets the metadata (author, title) from an EPUB file.
/// Returns a tuple of (author, title) where either may be None if not present.
/// Used for deduplication and display purposes.
pub fn get_metadata(input_path: &Path) -> Result<(Option<String>, Option<String>)> {
    let doc =
        EpubDoc::new(input_path).map_err(|e| anyhow::anyhow!("Failed to open EPUB file: {}", e))?;

    let title = doc.mdata("title").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone());

    Ok((author, title))
}

/// Gets the computed base name for an EPUB file based on its metadata.
/// This is used for progress bar display in cover-only mode.
/// Returns the sanitized "Author - Title" name, or falls back to the filename.
pub fn get_base_name(input_path: &Path) -> Result<String> {
    let fallback_name = input_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let (author, title) = get_metadata(input_path)?;

    Ok(format_epub_base_name(
        author.as_deref(),
        title.as_deref(),
        &fallback_name,
    ))
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
    format: ImageFormat,
}

/// Processes a single .epub file, extracting images matching the allowed extensions.
/// Uses author and title metadata for naming, falling back to filename.
/// If cover_only is true, only extracts the cover image.
/// If cover_fallback is true and cover_only is true but no cover is found, extracts all images.
/// If a filter is provided, only processes files matching the filter criteria.
/// Returns extraction counts including GIF routing information.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_formats: &HashSet<ImageFormat>,
    cover_only: bool,
    cover_fallback: bool,
    filter: &EpubFilter,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
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
        return Ok(ExtractionCounts::default());
    }

    let base_name = format_epub_base_name(author.as_deref(), title.as_deref(), &fallback_name);

    if cover_only {
        return extract_cover_only(
            &mut doc,
            output_base_dir,
            &base_name,
            allowed_formats,
            cover_fallback,
            config,
        );
    }

    extract_all_images(
        &mut doc,
        output_base_dir,
        &base_name,
        allowed_formats,
        config,
    )
}

/// Extracts all images from an EPUB file
fn extract_all_images(
    doc: &mut EpubDoc<std::io::BufReader<std::fs::File>>,
    output_base_dir: &Path,
    base_name: &str,
    allowed_formats: &HashSet<ImageFormat>,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    // Collect images from resources
    // resources is HashMap<String, ResourceItem> where ResourceItem has path and mime fields
    let mut images: Vec<EpubImage> = Vec::new();

    // Clone the resource keys and extract info to avoid borrow issues
    let resources: Vec<(String, ImageFormat)> = doc
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

            // Try to get image format from path first, then from MIME.
            let format = item
                .path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(ImageFormat::from_extension)
                .or_else(|| {
                    ImageFormat::identify_mime(&item.mime).map(|identified| identified.format)
                });

            format.map(|format| (id.clone(), format))
        })
        .collect::<Vec<(String, ImageFormat)>>();

    for (id, format) in resources {
        // Check if this format is in our allowed list.
        if allowed_formats.contains(&format) {
            images.push(EpubImage { id, format });
        }
    }

    if images.is_empty() {
        return Ok(ExtractionCounts::default());
    }

    let mut images_to_write = Vec::with_capacity(images.len());
    for image in images {
        // Get the image data - get_resource returns Option<(Vec<u8>, String)>
        let (data, _mime) = doc
            .get_resource(&image.id)
            .ok_or_else(|| anyhow::anyhow!("Failed to get resource '{}'", image.id))?;

        images_to_write.push(ImageToWrite {
            data,
            format: image.format,
        });
    }

    write_images(
        output_base_dir,
        base_name,
        images_to_write,
        config,
        WriteMode::BatchImages,
    )
}

/// Searches for a cover image by filename when metadata-based detection fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the image data and MIME type if found.
fn find_cover_by_filename(
    doc: &mut EpubDoc<std::io::BufReader<std::fs::File>>,
) -> Option<(Vec<u8>, String)> {
    // First, find the resource ID of a file named "cover" with a JPEG extension
    let cover_id = doc.resources.iter().find_map(|(id, item)| {
        let path = &item.path;
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());

        if file_stem.as_deref() == Some("cover")
            && let Some(ext) = &extension
            && JPEG_EXTENSIONS.contains(&ext.as_str())
        {
            return Some(id.clone());
        }
        None
    });

    // If we found a matching resource, get its data
    cover_id.and_then(|id| doc.get_resource(&id))
}

/// Extracts only the cover image from an EPUB file
/// If cover_fallback is true and no cover is found, extracts all images instead
fn extract_cover_only(
    doc: &mut EpubDoc<std::io::BufReader<std::fs::File>>,
    output_base_dir: &Path,
    base_name: &str,
    allowed_formats: &HashSet<ImageFormat>,
    cover_fallback: bool,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    // Try to get the cover image using the epub crate's get_cover method
    let cover = doc.get_cover();

    match cover {
        Some((data, mime)) => write_cover_image(
            data,
            &mime,
            output_base_dir,
            base_name,
            allowed_formats,
            config,
        ),
        None => {
            // Try fallback: look for a file named "cover" with JPEG extension
            if let Some((data, mime)) = find_cover_by_filename(doc) {
                write_cover_image(
                    data,
                    &mime,
                    output_base_dir,
                    base_name,
                    allowed_formats,
                    config,
                )
            } else if cover_fallback {
                // No cover found via metadata or filename, fall back to extracting all images
                extract_all_images(doc, output_base_dir, base_name, allowed_formats, config)
            } else {
                // No cover image found
                Ok(ExtractionCounts::default())
            }
        }
    }
}

/// Writes a single cover image with cover-only conversion semantics.
fn write_cover_image(
    data: Vec<u8>,
    mime: &str,
    output_base_dir: &Path,
    base_name: &str,
    allowed_formats: &HashSet<ImageFormat>,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    // Preserve the existing cover behavior: unknown cover MIME defaults to JPEG.
    let format = ImageFormat::identify_mime(mime)
        .map(|identified| identified.format)
        .unwrap_or(ImageFormat::Jpg);

    // Check if this format is in our allowed list.
    if !allowed_formats.contains(&format) {
        eprintln!(
            "Warning: Cover image format '{}' not in allowed formats, skipping.",
            format.extension()
        );
        return Ok(ExtractionCounts::default());
    }

    write_images(
        output_base_dir,
        base_name,
        vec![ImageToWrite { data, format }],
        config,
        WriteMode::RequiredCover,
    )
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
    fn test_jpeg_extensions_contains_common_extensions() {
        assert!(JPEG_EXTENSIONS.contains(&"jpg"));
        assert!(JPEG_EXTENSIONS.contains(&"jpeg"));
        assert!(JPEG_EXTENSIONS.contains(&"jpe"));
        assert!(JPEG_EXTENSIONS.contains(&"jfif"));
    }

    #[test]
    fn test_jpeg_extensions_does_not_contain_other_formats() {
        assert!(!JPEG_EXTENSIONS.contains(&"png"));
        assert!(!JPEG_EXTENSIONS.contains(&"gif"));
        assert!(!JPEG_EXTENSIONS.contains(&"webp"));
        assert!(!JPEG_EXTENSIONS.contains(&"bmp"));
    }
}
