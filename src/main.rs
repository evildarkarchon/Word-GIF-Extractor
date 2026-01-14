//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! This tool treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats.

mod common;
mod docx;
mod epub;

use anyhow::Result;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use common::{get_supported_extensions, normalize_format};
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

/// Collects all document files from the input paths
fn collect_document_files(inputs: &[PathBuf], recursive: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for input_path in inputs {
        if !input_path.exists() {
            continue;
        }

        if input_path.is_file() && is_supported_document(input_path) {
            files.push(input_path.clone());
        } else if input_path.is_dir() {
            if recursive {
                for entry in WalkDir::new(input_path).into_iter().flatten() {
                    let path = entry.path();
                    if path.is_file() && is_supported_document(path) {
                        files.push(path.to_path_buf());
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
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} - {msg}")
            .expect("Invalid progress bar template")
            .progress_chars("=>-"),
    );
    pb.set_message(format!(
        "Searching for epub files with {}",
        filter.description()
    ));

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

    pb.finish_with_message(format!(
        "Found {} matching epub file(s)",
        matching_epubs.len()
    ));

    // Combine matching EPUBs with other document types
    let mut result = matching_epubs;
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
) -> Result<usize> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => {
            docx::process_file(input_path, output_base_dir, allowed_extensions)
        }
        Some(DocumentType::Epub) => epub::process_file(
            input_path,
            output_base_dir,
            allowed_extensions,
            cover_only,
            cover_fallback,
            epub_filter,
        ),
        None => {
            anyhow::bail!(
                "Unsupported file type: {}. Supported types: .docx, .epub",
                input_path.display()
            );
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

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
    let files_to_process = if !epub_filter.is_empty() {
        filter_epub_files_with_progress(files, &epub_filter)
    } else {
        files
    };

    let mut total_images = 0usize;
    let mut total_documents = 0usize;

    for path in &files_to_process {
        match process_file(
            path,
            &output_dir,
            &target_extensions,
            args.cover_only,
            args.cover_fallback,
            &epub_filter,
        ) {
            Ok(count) => {
                total_images += count;
                if count > 0 {
                    total_documents += 1;
                }
            }
            Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
        }
    }

    if total_images > 0 {
        println!(
            "Processing complete! Extracted {} images from {} document(s).",
            total_images, total_documents
        );
    } else {
        println!("Processing complete! No images found.");
    }

    Ok(())
}
