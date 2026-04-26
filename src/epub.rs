//! EPUB file processing module

use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::common::{
    ExtractionConfig, ExtractionCounts, get_unique_output_path, is_safe_archive_path,
    sanitize_filename, write_image_to_file,
};
use crate::convert::{ConversionResult, try_convert};

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
    extension: String,
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
    allowed_extensions: &HashSet<&str>,
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
            allowed_extensions,
            input_path,
            cover_fallback,
            config,
        );
    }

    extract_all_images(
        &mut doc,
        output_base_dir,
        &base_name,
        allowed_extensions,
        input_path,
        config,
    )
}

/// Extracts all images from an EPUB file
fn extract_all_images(
    doc: &mut EpubDoc<std::io::BufReader<std::fs::File>>,
    output_base_dir: &Path,
    base_name: &str,
    allowed_extensions: &HashSet<&str>,
    _input_path: &Path,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
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
        return Ok(ExtractionCounts::default());
    }

    // create_dir_all is idempotent - succeeds if directory exists
    fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;

    let total_images = images.len();
    let mut counts = ExtractionCounts::default();
    let mut gif_dir_created = false;

    for (seq_index, image) in images.iter().enumerate() {
        // Get the image data - get_resource returns Option<(Vec<u8>, String)>
        let (data, _mime) = doc
            .get_resource(&image.id)
            .ok_or_else(|| anyhow::anyhow!("Failed to get resource '{}'", image.id))?;

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

        // Determine if this GIF is being routed (GIF routing takes priority per D-11)
        let is_routed_gif = is_gif && config.gif_output.is_some();

        // Attempt conversion if requested and not a routed GIF (per D-05: warn + write raw on failure)
        let (final_data, final_ext) = if let Some(format) = config.convert {
            if is_routed_gif {
                // GIF is being routed to gif_output -- write as-is, skip conversion
                (data, image.extension.clone())
            } else {
                match try_convert(
                    &data,
                    &image.extension,
                    format,
                    config.quality,
                    config.lossless,
                ) {
                    Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                        counts.converted += 1;
                        (converted_bytes, ext)
                    }
                    Ok(ConversionResult::Skipped(original_ext)) => {
                        eprintln!(
                            "Warning: Skipping conversion for {} ({} format not supported for conversion)",
                            base_name, original_ext
                        );
                        counts.skipped += 1;
                        (data, original_ext)
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Conversion failed for image in {}: {}",
                            base_name, e
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
            base_name,
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
    allowed_extensions: &HashSet<&str>,
    input_path: &Path,
    cover_fallback: bool,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    // Try to get the cover image using the epub crate's get_cover method
    let cover = doc.get_cover();

    match cover {
        Some((data, mime)) => {
            // Determine the extension from the MIME type
            let extension = mime_to_extension(&mime).unwrap_or_else(|| "jpg".to_string());

            // Check if this extension is in our allowed list
            if !allowed_extensions.contains(extension.as_str()) {
                eprintln!(
                    "Warning: Cover image format '{}' not in allowed formats, skipping.",
                    extension
                );
                return Ok(ExtractionCounts::default());
            }

            // Determine output directory: route GIF covers to gif_output if set
            let is_gif = extension == "gif";
            let is_routed_gif = is_gif && config.gif_output.is_some();
            let effective_output_dir = if let (true, Some(gif_dir)) = (is_gif, config.gif_output) {
                fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                gif_dir
            } else {
                output_base_dir
            };

            // Attempt conversion if requested and not a routed GIF (per D-04: skip entirely on failure)
            let (final_data, final_ext, was_converted) = if let Some(format) = config.convert {
                if is_routed_gif {
                    (data, extension.clone(), false)
                } else {
                    match try_convert(&data, &extension, format, config.quality, config.lossless) {
                        Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                            (converted_bytes, ext, true)
                        }
                        Ok(ConversionResult::Skipped(original_ext)) => {
                            eprintln!(
                                "Warning: Cover image format '{}' not supported for conversion, skipping cover.",
                                original_ext
                            );
                            return Ok(ExtractionCounts::default());
                        }
                        Err(e) => {
                            eprintln!("Warning: Cover conversion failed: {}", e);
                            return Ok(ExtractionCounts::default());
                        }
                    }
                }
            } else {
                (data, extension, false)
            };

            // create_dir_all is idempotent - succeeds if directory exists
            fs::create_dir_all(effective_output_dir)
                .context("Failed to create output directory")?;

            let output_path =
                get_unique_output_path(effective_output_dir, base_name, 0, 1, &final_ext)?;

            write_image_to_file(&output_path, &final_data)?;

            let mut counts = ExtractionCounts {
                extracted: 1,
                gifs_routed: 0,
                converted: 0,
                skipped: 0,
            };
            if is_routed_gif {
                counts.gifs_routed = 1;
            }
            if was_converted {
                counts.converted = 1;
            }
            Ok(counts)
        }
        None => {
            // Try fallback: look for a file named "cover" with JPEG extension
            if let Some((data, mime)) = find_cover_by_filename(doc) {
                let extension = mime_to_extension(&mime).unwrap_or_else(|| "jpg".to_string());

                // Check if this extension is in our allowed list
                if !allowed_extensions.contains(extension.as_str()) {
                    eprintln!(
                        "Warning: Cover image format '{}' not in allowed formats, skipping.",
                        extension
                    );
                    return Ok(ExtractionCounts::default());
                }

                // Determine output directory: route GIF covers to gif_output if set
                let is_gif = extension == "gif";
                let is_routed_gif = is_gif && config.gif_output.is_some();
                let effective_output_dir = if let (true, Some(gif_dir)) =
                    (is_gif, config.gif_output)
                {
                    fs::create_dir_all(gif_dir).context("Failed to create GIF output directory")?;
                    gif_dir
                } else {
                    output_base_dir
                };

                // Attempt conversion if requested and not a routed GIF (per D-04: skip entirely on failure)
                let (final_data, final_ext, was_converted) = if let Some(format) = config.convert {
                    if is_routed_gif {
                        (data, extension.clone(), false)
                    } else {
                        match try_convert(
                            &data,
                            &extension,
                            format,
                            config.quality,
                            config.lossless,
                        ) {
                            Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                                (converted_bytes, ext, true)
                            }
                            Ok(ConversionResult::Skipped(original_ext)) => {
                                eprintln!(
                                    "Warning: Cover image format '{}' not supported for conversion, skipping cover.",
                                    original_ext
                                );
                                return Ok(ExtractionCounts::default());
                            }
                            Err(e) => {
                                eprintln!("Warning: Cover conversion failed: {}", e);
                                return Ok(ExtractionCounts::default());
                            }
                        }
                    }
                } else {
                    (data, extension, false)
                };

                fs::create_dir_all(effective_output_dir)
                    .context("Failed to create output directory")?;

                let output_path =
                    get_unique_output_path(effective_output_dir, base_name, 0, 1, &final_ext)?;

                write_image_to_file(&output_path, &final_data)?;

                let mut counts = ExtractionCounts {
                    extracted: 1,
                    gifs_routed: 0,
                    converted: 0,
                    skipped: 0,
                };
                if is_routed_gif {
                    counts.gifs_routed = 1;
                }
                if was_converted {
                    counts.converted = 1;
                }
                Ok(counts)
            } else if cover_fallback {
                // No cover found via metadata or filename, fall back to extracting all images
                extract_all_images(
                    doc,
                    output_base_dir,
                    base_name,
                    allowed_extensions,
                    input_path,
                    config,
                )
            } else {
                // No cover image found
                Ok(ExtractionCounts::default())
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
