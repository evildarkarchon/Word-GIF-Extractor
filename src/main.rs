use anyhow::{Context, Result};
use clap::Parser;
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
}

fn process_file(input_path: &Path, output_base_dir: &Path) -> Result<()> {
    let doc_name = input_path
        .file_stem()
        .context("Invalid filename")?
        .to_string_lossy()
        .to_string();

    let file = fs::File::open(input_path)
        .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;

    let mut gif_indices = Vec::new();

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name();
        if name.to_lowercase().ends_with(".gif") {
            gif_indices.push(i);
        }
    }

    if gif_indices.is_empty() {
        return Ok(());
    }

    if !output_base_dir.exists() {
        fs::create_dir_all(&output_base_dir).context("Failed to create output directory")?;
    }

    let total_gifs = gif_indices.len();
    println!("Found {} .gif files in {}.", total_gifs, input_path.display());

    for (seq_index, &zip_index) in gif_indices.iter().enumerate() {
        let mut file = archive.by_index(zip_index)?;

        let output_filename = if total_gifs > 1 {
            format!("{}_{}.gif", doc_name, seq_index + 1)
        } else {
            format!("{}.gif", doc_name)
        };

        let output_path = output_base_dir.join(output_filename);

        println!("Extracting to: {}", output_path.display());

        let mut outfile = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

        io::copy(&mut file, &mut outfile)?;
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    let input_path_buf = args.input.or(args.input_pos).unwrap();
    let output_dir = args.output.unwrap_or_else(|| PathBuf::from("."));

    if !input_path_buf.exists() {
        anyhow::bail!("Input path does not exist: {}", input_path_buf.display());
    }

    if input_path_buf.is_file() {
        process_file(&input_path_buf, &output_dir)?;
    } else if input_path_buf.is_dir() {
        if args.recursive {
            for entry in WalkDir::new(&input_path_buf).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file()
                    && path
                        .extension()
                        .map_or(false, |ext| ext.eq_ignore_ascii_case("docx"))
                {
                    if let Err(e) = process_file(path, &output_dir) {
                        eprintln!("Error processing {}: {}", path.display(), e);
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
                        .map_or(false, |ext| ext.eq_ignore_ascii_case("docx"))
                {
                    if let Err(e) = process_file(&path, &output_dir) {
                        eprintln!("Error processing {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    println!("Processing complete!");

    Ok(())
}