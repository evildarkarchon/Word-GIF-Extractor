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
    /// Path to the input .docx/.epub file or directory
    #[arg(short, long, required_unless_present = "input_pos")]
    input: Option<PathBuf>,

    /// Path to the input .docx/.epub file or directory
    #[arg(required_unless_present = "input")]
    input_pos: Option<PathBuf>,

    /// Optional output directory (defaults to current directory)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Recursively search for .docx/.epub files if input is a directory
    #[arg(short, long)]
    recursive: bool,

    /// Image formats to extract (e.g., "png,jpg"). Defaults to all supported formats.
    #[arg(short, long, value_delimiter = ',', num_args = 0..)]
    formats: Option<Vec<String>>,
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
) -> Result<usize> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => {
            docx::process_file(input_path, output_base_dir, allowed_extensions)
        }
        Some(DocumentType::Epub) => {
            epub::process_file(input_path, output_base_dir, allowed_extensions)
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

    let input_path_buf = args.input.or(args.input_pos).unwrap();
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

    if !input_path_buf.exists() {
        anyhow::bail!("Input path does not exist: {}", input_path_buf.display());
    }

    let mut total_images = 0usize;
    let mut total_documents = 0usize;

    if input_path_buf.is_file() {
        match process_file(&input_path_buf, &output_dir, &target_extensions) {
            Ok(count) => {
                total_images += count;
                if count > 0 {
                    total_documents += 1;
                }
            }
            Err(e) => return Err(e),
        }
    } else if input_path_buf.is_dir() {
        if args.recursive {
            for entry in WalkDir::new(&input_path_buf)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() && is_supported_document(path) {
                    match process_file(path, &output_dir, &target_extensions) {
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
            for entry in fs::read_dir(&input_path_buf)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() && is_supported_document(&path) {
                    match process_file(&path, &output_dir, &target_extensions) {
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
