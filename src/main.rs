use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::io;
use std::path::PathBuf;
use zip::ZipArchive;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the input .docx file
    #[arg(short, long, required_unless_present = "input_pos")]
    input: Option<PathBuf>,

    /// Path to the input .docx file
    #[arg(required_unless_present = "input")]
    input_pos: Option<PathBuf>,

    /// Optional output directory (defaults to current directory)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let input_path_buf = args.input.or(args.input_pos).unwrap();

    if !input_path_buf.exists() {
        anyhow::bail!("Input file does not exist: {:?}", input_path_buf);
    }

    let input_path = input_path_buf.as_path();
    let doc_name = input_path
        .file_stem()
        .context("Invalid filename")?
        .to_string_lossy()
        .to_string();

    let output_dir = args.output.unwrap_or_else(|| PathBuf::from("."));
    if !output_dir.exists() {
        fs::create_dir_all(&output_dir).context("Failed to create output directory")?;
    }

    let file = fs::File::open(input_path).context("Failed to open input file")?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    let mut gif_indices = Vec::new();

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name();
        if name.to_lowercase().ends_with(".gif") {
            gif_indices.push(i);
        }
    }

    let total_gifs = gif_indices.len();
    println!("Found {} .gif files in the document.", total_gifs);

    for (seq_index, &zip_index) in gif_indices.iter().enumerate() {
        let mut file = archive.by_index(zip_index)?;
        
        let output_filename = if total_gifs > 1 {
            format!("{}_{}.gif", doc_name, seq_index + 1)
        } else {
            format!("{}.gif", doc_name)
        };

        let output_path = output_dir.join(output_filename);
        
        println!("Extracting to: {:?}", output_path);
        
        let mut outfile = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create output file: {:?}", output_path))?;
            
        io::copy(&mut file, &mut outfile)?;
    }

    println!("Extraction complete!");

    Ok(())
}