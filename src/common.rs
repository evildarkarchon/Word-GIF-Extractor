//! Common utilities shared between document processors

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

/// Returns the set of supported image file extensions
pub fn get_supported_extensions() -> HashSet<&'static str> {
    HashSet::from([
        "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "svg", "wmf", "emf", "webp", "ico",
    ])
}

/// Normalizes a format string to actual file extensions
pub fn normalize_format(fmt: &str) -> Vec<&'static str> {
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

/// Sanitizes a string to be safe for use as a filename
/// Replaces invalid characters with underscores
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Struct representing an image to be extracted
pub struct ImageToExtract {
    pub index: usize,
    pub extension: String,
}

/// Generates a unique output path, appending a counter if the file already exists
pub fn get_unique_output_path(
    output_base_dir: &Path,
    base_name: &str,
    seq_index: usize,
    total_images: usize,
    extension: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let output_filename = if total_images > 1 {
        format!("{}_{}.{}", base_name, seq_index + 1, extension)
    } else {
        format!("{}.{}", base_name, extension)
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

    Ok(output_path)
}

/// Writes image data to a file
pub fn write_image_to_file(output_path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use anyhow::Context;

    let outfile = fs::File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    let mut outfile = io::BufWriter::new(outfile);

    io::copy(&mut data.as_ref(), &mut outfile)
        .with_context(|| format!("Failed to write image data to {}", output_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Normal Name"), "Normal Name");
        assert_eq!(
            sanitize_filename("File/With\\Bad:Chars"),
            "File_With_Bad_Chars"
        );
        assert_eq!(sanitize_filename("Test*?\"<>|"), "Test______"); // 6 special chars
        assert_eq!(sanitize_filename("  Trimmed  "), "Trimmed");
    }

    #[test]
    fn test_normalize_format() {
        assert_eq!(normalize_format("jpg"), vec!["jpg", "jpeg"]);
        assert_eq!(normalize_format("JPEG"), vec!["jpg", "jpeg"]);
        assert_eq!(normalize_format("png"), vec!["png"]);
        assert_eq!(normalize_format("unknown").len(), 0);
    }

    #[test]
    fn test_get_supported_extensions() {
        let exts = get_supported_extensions();
        assert!(exts.contains("jpg"));
        assert!(exts.contains("png"));
        assert!(exts.contains("gif"));
        assert!(!exts.contains("pdf"));
    }
}
