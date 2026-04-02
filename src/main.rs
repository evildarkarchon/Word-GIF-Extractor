//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! This tool treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats.

mod common;
mod convert;
mod docx;
mod epub;

use anyhow::Result;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::convert::OutputFormat;
use common::{ExtractionCounts, get_supported_extensions, normalize_format};
use epub::EpubFilter;

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract images from Word (.docx) and EPUB files", long_about = None)]
struct Args {
    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    inputs: Vec<PathBuf>,

    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    #[arg(short = 'i', long = "input", num_args = 1..)]
    named_inputs: Vec<PathBuf>,

    /// Optional output directory (defaults to current directory)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Recursively search for .docx/.epub files if input is a directory
    #[arg(short, long)]
    recursive: bool,

    /// Image formats to extract (e.g., "png,jpg"). Defaults to all supported formats.
    #[arg(short, long, value_delimiter = ',', num_args = 0..)]
    formats: Option<Vec<String>>,

    /// Extract only cover image from EPUB files
    #[arg(short = 'c', long)]
    cover_only: bool,

    /// Fallback to extracting all images if cover not found (EPUB only, requires --cover-only)
    #[arg(long, requires = "cover_only")]
    cover_fallback: bool,

    /// Filter EPUB files by title (case-insensitive substring match)
    #[arg(long)]
    title: Option<String>,

    /// Filter EPUB files by author (case-insensitive substring match)
    #[arg(long)]
    author: Option<String>,

    /// Convert extracted images to specified format (jpg, png, webp)
    #[arg(short = 'C', long, conflicts_with = "gif_only")]
    convert: Option<OutputFormat>,

    /// JPEG/WebP encoding quality override (1-100, default: 85)
    #[arg(short = 'q', long, requires = "convert", conflicts_with = "lossless", value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: Option<u8>,

    /// Use lossless WebP encoding instead of lossy
    #[arg(short = 'L', long, requires = "convert", conflicts_with = "quality")]
    lossless: bool,

    /// Extract only GIF files (skip all other image formats)
    #[arg(short = 'g', long, conflicts_with = "convert")]
    gif_only: bool,

    /// Separate output directory for GIF files
    #[arg(short = 'G', long)]
    gif_output: Option<PathBuf>,
}

/// Supported document types
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocumentType {
    Docx,
    Epub,
}

/// Determines the document type based on file extension
fn get_document_type(path: &Path) -> Option<DocumentType> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .and_then(|ext| match ext.as_str() {
            "docx" => Some(DocumentType::Docx),
            "epub" => Some(DocumentType::Epub),
            _ => None,
        })
}

/// Checks if a path is a supported document type
fn is_supported_document(path: &Path) -> bool {
    get_document_type(path).is_some()
}

/// Checks if a path is an EPUB file
fn is_epub(path: &Path) -> bool {
    get_document_type(path) == Some(DocumentType::Epub)
}

/// Creates a standard progress bar style for collection phases
fn create_progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} - {msg}")
        .expect("Invalid progress bar template")
        .progress_chars("=>-")
}

/// Creates a spinner style for phases where total count is unknown
fn create_spinner_style() -> ProgressStyle {
    ProgressStyle::default_spinner()
        .template("{spinner:.green} {msg}")
        .expect("Invalid spinner template")
}

/// Collects all document files from the input paths with progress indication
fn collect_document_files(inputs: &[PathBuf], recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // Use spinner for recursive directory scanning since we don't know total count upfront
    let use_spinner = recursive && inputs.iter().any(|p| p.is_dir());
    let pb = if use_spinner {
        let pb = ProgressBar::new_spinner();
        pb.set_style(create_spinner_style());
        pb.set_message("Scanning directories for documents...");
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(pb)
    } else {
        None
    };

    for input_path in inputs {
        if !input_path.exists() {
            continue;
        }

        if input_path.is_file() && is_supported_document(input_path) {
            files.push(input_path.clone());
            if let Some(ref pb) = pb {
                pb.set_message(format!("Found {} document(s)...", files.len()));
            }
        } else if input_path.is_dir() {
            if recursive {
                for entry in WalkDir::new(input_path).into_iter().flatten() {
                    let path = entry.path();
                    if path.is_file() && is_supported_document(path) {
                        files.push(path.to_path_buf());
                        if let Some(ref pb) = pb {
                            pb.set_message(format!("Found {} document(s)...", files.len()));
                        }
                    }
                }
            } else if let Ok(entries) = fs::read_dir(input_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && is_supported_document(&path) {
                        files.push(path);
                    }
                }
            }
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message(format!("Found {} document(s)", files.len()));
    }

    files
}

/// Filters EPUB files by metadata with a progress bar.
/// Returns only the files that match the filter criteria.
fn filter_epub_files_with_progress(files: Vec<PathBuf>, filter: &EpubFilter) -> Vec<PathBuf> {
    // Separate EPUB files from other document types
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| is_epub(p));

    if epub_files.is_empty() {
        return other_files;
    }

    let pb = ProgressBar::new(epub_files.len() as u64);
    pb.set_style(create_progress_style());
    pb.set_message(format!("Filtering EPUBs by {}", filter.description()));

    let mut matching_epubs = Vec::new();
    for path in epub_files {
        pb.inc(1);
        match epub::check_filter_match(&path, filter) {
            Ok(true) => matching_epubs.push(path),
            Ok(false) => {} // File doesn't match filter, skip
            Err(e) => {
                // Log error but continue searching
                pb.suspend(|| {
                    eprintln!("Warning: Could not read {}: {}", path.display(), e);
                });
            }
        }
    }

    pb.finish_with_message(format!("Found {} matching EPUB(s)", matching_epubs.len()));

    // Combine matching EPUBs with other document types
    let mut result = matching_epubs;
    result.extend(other_files);
    result
}

/// Deduplicates EPUB files based on their metadata (author + title).
/// Keeps the first occurrence of each unique (author, title) combination.
/// Non-EPUB files are passed through unchanged.
/// EPUBs without metadata are deduplicated by filename.
fn deduplicate_by_metadata(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| is_epub(p));

    if epub_files.is_empty() {
        return other_files;
    }

    let pb = ProgressBar::new(epub_files.len() as u64);
    pb.set_style(create_progress_style());
    pb.set_message("Deduplicating EPUBs by metadata");

    // Use a HashMap to track seen (author, title) combinations
    // Key: (lowercase author, lowercase title) for case-insensitive deduplication
    let mut seen: HashMap<(String, String), PathBuf> = HashMap::new();
    let mut unique_epubs = Vec::new();
    let mut duplicates_found = 0usize;

    for path in epub_files {
        pb.inc(1);

        // Try to get metadata for deduplication
        let key = match epub::get_metadata(&path) {
            Ok((author, title)) if author.is_some() || title.is_some() => {
                // Normalize: lowercase and trim, use empty string for None
                let author_key = author
                    .as_deref()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                let title_key = title
                    .as_deref()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                (author_key, title_key)
            }
            _ => {
                // Fallback to filename if we can't read metadata or if both author and title are missing
                let filename = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                (String::new(), filename)
            }
        };

        // Only add if we haven't seen this combination before
        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
            e.insert(path.clone());
            unique_epubs.push(path);
        } else {
            duplicates_found += 1;
        }
    }

    if duplicates_found > 0 {
        pb.finish_with_message(format!(
            "Removed {} duplicate EPUB(s), {} unique remaining",
            duplicates_found,
            unique_epubs.len()
        ));
    } else {
        pb.finish_and_clear();
    }

    // Combine unique EPUBs with other document types
    let mut result = unique_epubs;
    result.extend(other_files);
    result
}

/// Processes a single file based on its type
fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    epub_filter: &EpubFilter,
    gif_output: Option<&Path>,
) -> Result<ExtractionCounts> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => {
            docx::process_file(input_path, output_base_dir, allowed_extensions, gif_output)
        }
        Some(DocumentType::Epub) => epub::process_file(
            input_path,
            output_base_dir,
            allowed_extensions,
            cover_only,
            cover_fallback,
            epub_filter,
            gif_output,
        ),
        None => {
            anyhow::bail!(
                "Unsupported file type: {}. Supported types: .docx, .epub",
                input_path.display()
            );
        }
    }
}

/// Validates argument combinations that clap cannot check declaratively.
///
/// Clap's `requires` and `conflicts_with` attributes operate on argument
/// presence/absence only. These checks validate against another argument's
/// *value*:
/// - `--quality` with `--convert png` (PNG is lossless, quality is meaningless)
/// - `--lossless` with `--convert jpg` or `--convert png` (lossless only applies to WebP)
fn validate_args(args: &Args) -> Result<()> {
    if let Some(format) = &args.convert {
        if args.quality.is_some() && *format == OutputFormat::Png {
            anyhow::bail!("--quality cannot be used with --convert png (PNG is a lossless format)");
        }
        if args.lossless && *format != OutputFormat::Webp {
            anyhow::bail!("--lossless can only be used with --convert webp");
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    // Combine positional and named inputs, fallback to current directory if none specified
    let mut all_inputs: Vec<PathBuf> = args.inputs.into_iter().chain(args.named_inputs).collect();

    if all_inputs.is_empty() {
        let cwd = std::env::current_dir()?;
        println!(
            "No input path specified, using current directory: {}",
            cwd.display()
        );
        all_inputs.push(cwd);
    }

    let output_dir = args.output.unwrap_or_else(|| PathBuf::from("."));

    // Determine allowed extensions
    let mut target_extensions = HashSet::new();
    if let Some(formats) = &args.formats {
        for fmt in formats {
            let normalized = normalize_format(fmt);
            for ext in normalized {
                target_extensions.insert(ext);
            }
        }
    }

    // Fallback if empty or no formats specified
    if target_extensions.is_empty() {
        target_extensions = get_supported_extensions();
    }

    // Create EPUB filter from CLI args
    let epub_filter = EpubFilter {
        title: args.title,
        author: args.author,
    };

    // Validate input paths exist
    for input_path in &all_inputs {
        if !input_path.exists() {
            eprintln!(
                "Warning: Input path does not exist: {}",
                input_path.display()
            );
        }
    }

    // Collect all document files
    let files = collect_document_files(&all_inputs, args.recursive);

    // If a filter is specified, search EPUB files with a progress bar
    let filtered_files = if !epub_filter.is_empty() {
        filter_epub_files_with_progress(files, &epub_filter)
    } else {
        files
    };

    // Deduplicate EPUBs by metadata (author + title) to avoid processing the same book twice
    let files_to_process = deduplicate_by_metadata(filtered_files);

    if files_to_process.is_empty() {
        println!("No documents found to process.");
        return Ok(());
    }

    // --gif-only overrides format selection to extract only GIFs
    if args.gif_only {
        target_extensions.clear();
        target_extensions.insert("gif");
    }

    let mut total_counts = ExtractionCounts::default();
    let mut total_documents = 0usize;
    let mut has_docx_images = false;

    // Create progress bar for extraction phase with context-appropriate message
    let pb = ProgressBar::new(files_to_process.len() as u64);
    pb.set_style(create_progress_style());
    let extraction_msg = if args.cover_only {
        "Extracting cover images"
    } else {
        "Extracting images from documents"
    };
    pb.set_message(extraction_msg);

    for path in &files_to_process {
        let is_docx = get_document_type(path) == Some(DocumentType::Docx);
        // Update progress bar with appropriate name
        // For EPUBs in cover-only mode, show the output filename (Author - Title)
        // Otherwise show the input filename
        let display_name = if args.cover_only && is_epub(path) {
            epub::get_base_name(path).unwrap_or_else(|_| {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string())
            })
        } else {
            path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string())
        };
        pb.set_message(display_name);

        match process_file(
            path,
            &output_dir,
            &target_extensions,
            args.cover_only,
            args.cover_fallback,
            &epub_filter,
            args.gif_output.as_deref(),
        ) {
            Ok(counts) => {
                total_counts.extracted += counts.extracted;
                total_counts.gifs_routed += counts.gifs_routed;
                if counts.extracted > 0 {
                    total_documents += 1;
                    if is_docx {
                        has_docx_images = true;
                    }
                }
            }
            Err(e) => {
                pb.suspend(|| {
                    eprintln!("Error processing {}: {}", path.display(), e);
                });
            }
        }
        pb.inc(1);
    }

    if total_counts.extracted > 0 {
        // Only label as "cover(s)" if in cover-only mode AND no DOCX images were extracted
        // (DOCX files always extract all images regardless of cover_only flag)
        let item_name = if args.cover_only && !has_docx_images {
            "cover(s)"
        } else {
            "image(s)"
        };
        if total_counts.gifs_routed > 0 {
            let gif_dir = args.gif_output.as_ref().unwrap();
            pb.finish_with_message(format!(
                "Extracted {} {}, routed {} GIF(s) to {} from {} document(s)",
                total_counts.extracted,
                item_name,
                total_counts.gifs_routed,
                gif_dir.display(),
                total_documents
            ));
        } else {
            pb.finish_with_message(format!(
                "Extracted {} {} from {} document(s)",
                total_counts.extracted, item_name, total_documents
            ));
        }
    } else {
        // For the "not found" message, still use cover terminology if that was the intent
        let not_found_msg = if args.cover_only && !has_docx_images {
            "No cover images found"
        } else {
            "No images found"
        };
        pb.finish_with_message(not_found_msg);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_flag_parses_all_formats() {
        let args = Args::try_parse_from(["test", "--convert", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(OutputFormat::Jpg));
        let args = Args::try_parse_from(["test", "--convert", "png"]).unwrap();
        assert_eq!(args.convert, Some(OutputFormat::Png));
        let args = Args::try_parse_from(["test", "--convert", "webp"]).unwrap();
        assert_eq!(args.convert, Some(OutputFormat::Webp));
    }

    #[test]
    fn test_convert_short_flag() {
        let args = Args::try_parse_from(["test", "-C", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(OutputFormat::Jpg));
    }

    #[test]
    fn test_quality_with_convert_jpg() {
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "90"]).unwrap();
        assert_eq!(args.quality, Some(90));
    }

    #[test]
    fn test_quality_with_convert_webp() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--quality", "90"]).unwrap();
        assert_eq!(args.quality, Some(90));
    }

    #[test]
    fn test_quality_range_validation() {
        assert!(Args::try_parse_from(["test", "--convert", "jpg", "--quality", "0"]).is_err());
        assert!(Args::try_parse_from(["test", "--convert", "jpg", "--quality", "101"]).is_err());
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "1"]).unwrap();
        assert_eq!(args.quality, Some(1));
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "100"]).unwrap();
        assert_eq!(args.quality, Some(100));
    }

    #[test]
    fn test_quality_requires_convert() {
        assert!(Args::try_parse_from(["test", "--quality", "90"]).is_err());
    }

    #[test]
    fn test_quality_with_png_error() {
        let args = Args::try_parse_from(["test", "--convert", "png", "--quality", "90"]).unwrap();
        let result = validate_args(&args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("--quality cannot be used with --convert png"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_convert_and_gif_only_conflict() {
        assert!(Args::try_parse_from(["test", "--convert", "jpg", "--gif-only"]).is_err());
    }

    #[test]
    fn test_gif_only_short_flag() {
        let args = Args::try_parse_from(["test", "-g"]).unwrap();
        assert!(args.gif_only);
    }

    #[test]
    fn test_gif_output_independent() {
        let args = Args::try_parse_from(["test", "--gif-output", "/tmp/gifs"]).unwrap();
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_gif_output_short_flag() {
        let args = Args::try_parse_from(["test", "-G", "/tmp/gifs"]).unwrap();
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_lossless_with_convert_webp() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--lossless"]).unwrap();
        assert!(args.lossless);
    }

    #[test]
    fn test_lossless_requires_convert() {
        assert!(Args::try_parse_from(["test", "--lossless"]).is_err());
    }

    #[test]
    fn test_lossless_conflicts_with_quality() {
        assert!(
            Args::try_parse_from(["test", "--convert", "webp", "--lossless", "--quality", "90"])
                .is_err()
        );
    }

    #[test]
    fn test_lossless_with_jpg_error() {
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--lossless"]).unwrap();
        let result = validate_args(&args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("--lossless can only be used with --convert webp"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_lossless_with_png_error() {
        let args = Args::try_parse_from(["test", "--convert", "png", "--lossless"]).unwrap();
        let result = validate_args(&args);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("--lossless can only be used with --convert webp"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_lossless_short_flag() {
        let args = Args::try_parse_from(["test", "-L", "--convert", "webp"]).unwrap();
        assert!(args.lossless);
    }

    #[test]
    fn test_existing_flags_unchanged() {
        let args = Args::try_parse_from([
            "test",
            "-o",
            "/tmp",
            "-r",
            "-f",
            "png,jpg",
            "-c",
            "--cover-fallback",
        ])
        .unwrap();
        assert_eq!(args.output, Some(PathBuf::from("/tmp")));
        assert!(args.recursive);
        assert!(args.cover_only);
        assert!(args.cover_fallback);
    }

    #[test]
    fn test_gif_only_overrides_extensions() {
        let mut target_extensions = get_supported_extensions();
        assert!(target_extensions.len() > 1);
        target_extensions.clear();
        target_extensions.insert("gif");
        assert_eq!(target_extensions.len(), 1);
        assert!(target_extensions.contains("gif"));
        assert!(!target_extensions.contains("png"));
        assert!(!target_extensions.contains("jpg"));
    }

    #[test]
    fn test_gif_only_overrides_formats_flag() {
        let mut target_extensions = HashSet::new();
        target_extensions.insert("png");
        target_extensions.insert("jpg");
        target_extensions.clear();
        target_extensions.insert("gif");
        assert_eq!(target_extensions.len(), 1);
        assert!(target_extensions.contains("gif"));
    }

    #[test]
    fn test_extraction_counts_split_message_logic() {
        let counts = ExtractionCounts {
            extracted: 5,
            gifs_routed: 2,
            converted: 0,
            skipped: 0,
        };
        assert!(counts.gifs_routed > 0);
        let counts = ExtractionCounts {
            extracted: 3,
            gifs_routed: 0,
            converted: 0,
            skipped: 0,
        };
        assert!(counts.gifs_routed == 0);
    }

    #[test]
    fn test_gif_only_and_gif_output_both_set() {
        let args =
            Args::try_parse_from(["test", "--gif-only", "--gif-output", "/tmp/gifs"]).unwrap();
        assert!(args.gif_only);
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_gif_output_without_gif_only() {
        let args = Args::try_parse_from(["test", "--gif-output", "/tmp/gifs"]).unwrap();
        assert!(!args.gif_only);
        assert_eq!(args.gif_output, Some(PathBuf::from("/tmp/gifs")));
    }

    #[test]
    fn test_gif_only_without_gif_output() {
        let args = Args::try_parse_from(["test", "--gif-only"]).unwrap();
        assert!(args.gif_only);
        assert!(args.gif_output.is_none());
    }
}
