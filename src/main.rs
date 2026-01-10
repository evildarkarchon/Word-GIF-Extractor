//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! This tool treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats.

mod common;
mod docx;
mod epub;

use anyhow::Result;
use clap::Parser;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use common::{get_supported_extensions, normalize_format};

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract images from Word (.docx) and EPUB files", long_about = None)]
struct Args {
    /// Paths to input .docx/.epub files or directories (positional)
    inputs: Vec<PathBuf>,

    /// Paths to input .docx/.epub files or directories (named)
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

/// Processes a single file based on its type
fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
) -> Result<usize> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => {
            docx::process_file(input_path, output_base_dir, allowed_extensions)
        }
        Some(DocumentType::Epub) => {
            epub::process_file(input_path, output_base_dir, allowed_extensions, cover_only, cover_fallback)
        }
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

    // Combine positional and named inputs
    let all_inputs: Vec<PathBuf> = args.inputs.into_iter().chain(args.named_inputs).collect();

    if all_inputs.is_empty() {
        anyhow::bail!("At least one input path is required");
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

    let mut total_images = 0usize;
    let mut total_documents = 0usize;

    for input_path_buf in &all_inputs {
        if !input_path_buf.exists() {
            eprintln!("Warning: Input path does not exist: {}", input_path_buf.display());
            continue;
        }

        if input_path_buf.is_file() {
            match process_file(
                input_path_buf,
                &output_dir,
                &target_extensions,
                args.cover_only,
                args.cover_fallback,
            ) {
                Ok(count) => {
                    total_images += count;
                    if count > 0 {
                        total_documents += 1;
                    }
                }
                Err(e) => eprintln!("Error processing {}: {}", input_path_buf.display(), e),
            }
        } else if input_path_buf.is_dir() {
            if args.recursive {
                for entry in WalkDir::new(input_path_buf) {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(e) => {
                            eprintln!("Warning: Could not access path: {}", e);
                            continue;
                        }
                    };
                    let path = entry.path();
                    if path.is_file() && is_supported_document(path) {
                        match process_file(path, &output_dir, &target_extensions, args.cover_only, args.cover_fallback) {
                            Ok(count) => {
                                total_images += count;
                                if count > 0 {
                                    total_documents += 1;
                                }
                            }
                            Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
                        }
                    }
                }
            } else {
                let entries = match fs::read_dir(input_path_buf) {
                    Ok(entries) => entries,
                    Err(e) => {
                        eprintln!("Warning: Could not read directory {}: {}", input_path_buf.display(), e);
                        continue;
                    }
                };
                for entry in entries {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(e) => {
                            eprintln!("Warning: Could not access entry: {}", e);
                            continue;
                        }
                    };
                    let path = entry.path();
                    if path.is_file() && is_supported_document(&path) {
                        match process_file(&path, &output_dir, &target_extensions, args.cover_only, args.cover_fallback) {
                            Ok(count) => {
                                total_images += count;
                                if count > 0 {
                                    total_documents += 1;
                                }
                            }
                            Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
                        }
                    }
                }
            }
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
