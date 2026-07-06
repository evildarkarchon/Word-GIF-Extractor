//! EPUB file processing module

use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use std::collections::HashSet;
use std::path::Path;

use crate::archive_image_discovery::{ArchiveImageSource, discover_images};
use crate::common::{DocumentExtractionResult, ExtractionConfig, sanitize_filename};
use crate::extraction_warning::combine_document_warnings;
use crate::image_format::ImageFormat;
use crate::image_writer::{WriteMode, write_images};

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

struct EpubResourceCandidate {
    id: String,
    source_name: String,
}

/// Processes a single .epub file, extracting images matching the allowed extensions.
/// Uses author and title metadata for naming, falling back to filename.
/// If cover_only is true, only extracts the cover image.
/// If cover_fallback is true and cover_only is true but no cover is found, extracts all images.
/// If a filter is provided, only processes files matching the filter criteria.
/// Returns extraction counts plus Archive image discovery warnings.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_formats: &HashSet<ImageFormat>,
    cover_only: bool,
    cover_fallback: bool,
    filter: &EpubFilter,
    config: &ExtractionConfig,
) -> Result<DocumentExtractionResult> {
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
        return Ok(DocumentExtractionResult::default());
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
) -> Result<DocumentExtractionResult> {
    // Clone the resource keys and extract info to avoid borrow issues
    // Send every manifest resource through Archive image discovery so byte-first
    // Image format identification can recover images with weak EPUB labels.
    let resources: Vec<EpubResourceCandidate> = doc
        .resources
        .iter()
        .map(|(id, item)| EpubResourceCandidate {
            id: id.clone(),
            source_name: item.path.to_string_lossy().to_string(),
        })
        .collect();

    let mut sources = Vec::new();
    for candidate in resources {
        // Get the image data - get_resource returns Option<(Vec<u8>, String)>
        let (data, mime) = doc
            .get_resource(&candidate.id)
            .ok_or_else(|| anyhow::anyhow!("Failed to get resource '{}'", candidate.id))?;

        sources.push(ArchiveImageSource::batch(
            data,
            candidate.source_name,
            Some(mime),
        ));
    }

    let discovered = discover_images(sources, allowed_formats);
    let write_result = write_images(
        output_base_dir,
        base_name,
        discovered.images,
        config,
        WriteMode::BatchImages,
    )?;

    Ok(DocumentExtractionResult::new(
        write_result.counts,
        combine_document_warnings(discovered.warnings, write_result.warnings),
    ))
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
) -> Result<DocumentExtractionResult> {
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
                Ok(DocumentExtractionResult::default())
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
) -> Result<DocumentExtractionResult> {
    let discovered = discover_images(
        vec![ArchiveImageSource::required_cover(data, mime.to_string())],
        allowed_formats,
    );

    let write_result = write_images(
        output_base_dir,
        base_name,
        discovered.images,
        config,
        WriteMode::RequiredCover,
    )?;

    Ok(DocumentExtractionResult::new(
        write_result.counts,
        combine_document_warnings(discovered.warnings, write_result.warnings),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-epub-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn write_minimal_epub(path: &Path, image_href: &str, image_mime: &str, image_data: &[u8]) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        zip.start_file("mimetype", options)
            .expect("mimetype entry should start");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype should be writable");

        zip.start_file("META-INF/container.xml", options)
            .expect("container entry should start");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .expect("container should be writable");

        zip.start_file("OEBPS/content.opf", options)
            .expect("OPF entry should start");
        let opf = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>Magic Test</dc:title>
    <dc:creator>Tester</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="img" href="{image_href}" media-type="{image_mime}"/>
  </manifest>
  <spine>
    <itemref idref="nav"/>
  </spine>
</package>"#
        );
        zip.write_all(opf.as_bytes())
            .expect("OPF should be writable");

        zip.start_file("OEBPS/nav.xhtml", options)
            .expect("nav entry should start");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><nav></nav></body></html>"#,
        )
        .expect("nav should be writable");

        zip.start_file(format!("OEBPS/{image_href}"), options)
            .expect("image entry should start");
        zip.write_all(image_data)
            .expect("image data should be writable");
        zip.finish().expect("EPUB archive should finish");
    }

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

    #[test]
    fn extracts_epub_resource_by_magic_before_declared_extension_and_mime() {
        let temp_dir = temp_test_dir("magic-before-labels");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");

        write_minimal_epub(
            &input_path,
            "images/mislabeled.jpg",
            "image/jpeg",
            MINIMAL_PNG,
        );

        let allowed_formats = HashSet::from([ImageFormat::Png]);
        let config = ExtractionConfig {
            convert: None,
            quality: 85,
            lossless: false,
            gif_output: None,
        };

        let result = process_file(
            &input_path,
            &output_dir,
            &allowed_formats,
            false,
            false,
            &EpubFilter::default(),
            &config,
        )
        .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert!(output_dir.join("Tester - Magic Test.png").exists());
        assert!(!output_dir.join("Tester - Magic Test.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn extracts_epub_resource_by_magic_without_declared_image_hints() {
        let temp_dir = temp_test_dir("magic-without-hints");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");

        write_minimal_epub(
            &input_path,
            "images/mislabeled.bin",
            "application/octet-stream",
            MINIMAL_PNG,
        );

        let allowed_formats = HashSet::from([ImageFormat::Png]);
        let config = ExtractionConfig {
            convert: None,
            quality: 85,
            lossless: false,
            gif_output: None,
        };

        let result = process_file(
            &input_path,
            &output_dir,
            &allowed_formats,
            false,
            false,
            &EpubFilter::default(),
            &config,
        )
        .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert!(output_dir.join("Tester - Magic Test.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
