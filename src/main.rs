use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the input .docx file or directory
    #[arg(short, long, required_unless_present = "input_pos")]
    input: Option<PathBuf>,

    /// Path to the input .docx file or directory
    #[arg(required_unless_present = "input")]
    input_pos: Option<PathBuf>,

    /// Optional output directory (defaults to current directory)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Recursively search for .docx files if input is a directory
    #[arg(short, long)]
    recursive: bool,

    /// Image formats to extract (e.g., "png,jpg"). Defaults to all supported formats.
    #[arg(short, long, value_delimiter = ',', num_args = 0..)]
    formats: Option<Vec<String>>,
}

fn get_supported_extensions() -> HashSet<&'static str> {
    HashSet::from([
        "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "svg", "wmf", "emf", "webp", "ico",
    ])
}

fn normalize_format(fmt: &str) -> Vec<&'static str> {
    let fmt_lower = fmt.trim().to_lowercase();
    match fmt_lower.as_str() {
        "jpg" | "jpeg" => vec!["jpg", "jpeg"],
        "png" => vec!["png"],
        "gif" => vec!["gif"],
        "bmp" => vec!["bmp"],
        "tiff" | "tif" => vec!["tiff", "tif"],
        "svg" => vec!["svg"],
        "wmf" => vec!["wmf"],
        "emf" => vec!["emf"],
        "webp" => vec!["webp"],
        "ico" => vec!["ico"],
        _ => {
            eprintln!("Warning: Unrecognized format '{}' ignored", fmt.trim());
            vec![]
        }
    }
}

/// Processes a single .docx file, extracting images matching the allowed extensions.
/// Returns the number of images extracted.
fn process_file(
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

    struct ImageToExtract {
        index: usize,
        extension: String,
    }

    let mut images: Vec<ImageToExtract> = Vec::new();

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name();

        // Defense-in-depth: skip entries with path traversal patterns
        if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
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

    if !output_base_dir.exists() {
        fs::create_dir_all(output_base_dir).context("Failed to create output directory")?;
    }

    let total_images = images.len();
    println!(
        "Found {} image files in {}.",
        total_images,
        input_path.display()
    );

    for (seq_index, image) in images.iter().enumerate() {
        let mut file = archive.by_index(image.index)?;

        let output_filename = if total_images > 1 {
            format!("{}_{}.{}", doc_name, seq_index + 1, image.extension)
        } else {
            format!("{}.{}", doc_name, image.extension)
        };

        let mut output_path = output_base_dir.join(output_filename);

        // Counter-based approach to avoid infinite loops and produce cleaner filenames
        if output_path.exists() {
            let base_stem = output_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let base_ext = output_path
                .extension()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let mut counter = 0u32;
            const MAX_ATTEMPTS: u32 = 1000;

            while output_path.exists() {
                counter += 1;
                if counter > MAX_ATTEMPTS {
                    anyhow::bail!(
                        "Could not find unique filename after {} attempts for {}",
                        MAX_ATTEMPTS,
                        base_stem
                    );
                }
                let new_filename = if base_ext.is_empty() {
                    format!("{}_{}", base_stem, counter)
                } else {
                    format!("{}_{}.{}", base_stem, counter, base_ext)
                };
                output_path.set_file_name(new_filename);
            }
        }

        println!("Extracting to: {}", output_path.display());

        let outfile = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
        let mut outfile = io::BufWriter::new(outfile);

        io::copy(&mut file, &mut outfile)
            .with_context(|| format!("Failed to write image data to {}", output_path.display()))?;
    }

    Ok(total_images)
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
                if path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
                {
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
                if path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
                {
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
