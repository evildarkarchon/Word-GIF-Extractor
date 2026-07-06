//! Common utilities shared between document processors

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::convert::OutputFormat;
use crate::extraction_warning::DocumentExtractionWarning;

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

/// Counts of images extracted during a single file processing operation.
///
/// `extracted` counts ALL images written to disk (including GIFs routed
/// to a separate directory). `gifs_routed` counts only those GIFs written
/// to the `--gif-output` directory specifically.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExtractionCounts {
    /// Total number of images extracted (includes routed GIFs)
    pub extracted: usize,
    /// Number of GIF files routed to the GIF output directory
    pub gifs_routed: usize,
    /// Number of images successfully converted to target format
    pub converted: usize,
    /// Number of images skipped during conversion (unsupported format or error)
    pub skipped: usize,
}

/// Configuration for image extraction and conversion behavior.
///
/// Bundles conversion-related parameters that are threaded through
/// the dispatch chain from extraction run intake to format-specific processors.
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Target format for conversion (None = extract as-is)
    pub convert: Option<OutputFormat>,
    /// JPEG/WebP encoding quality (1-100)
    pub quality: u8,
    /// Use lossless WebP encoding
    pub lossless: bool,
    /// Separate output directory for GIF files
    pub gif_output: Option<PathBuf>,
}

/// Result returned by one document adapter after image extraction.
///
/// Counts describe images written by the Image write pipeline. Warnings are
/// structured facts routed through the Extraction run observer so lower modules
/// do not print directly to stderr.
#[derive(Debug, Default)]
pub struct DocumentExtractionResult {
    /// Write/conversion counts for the document.
    pub counts: ExtractionCounts,
    /// User-visible extraction warning facts.
    pub warnings: Vec<DocumentExtractionWarning>,
}

impl DocumentExtractionResult {
    /// Creates a document extraction result from write counts and warning facts.
    pub fn new(counts: ExtractionCounts, warnings: Vec<DocumentExtractionWarning>) -> Self {
        Self { counts, warnings }
    }
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

    outfile
        .write_all(data)
        .with_context(|| format!("Failed to write image data to {}", output_path.display()))?;

    outfile
        .flush()
        .with_context(|| format!("Failed to flush data to {}", output_path.display()))?;

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
    fn test_extraction_counts_default() {
        let counts = ExtractionCounts::default();
        assert_eq!(counts.extracted, 0);
        assert_eq!(counts.gifs_routed, 0);
        assert_eq!(counts.converted, 0);
        assert_eq!(counts.skipped, 0);
    }

    #[test]
    fn test_extraction_counts_accumulation() {
        let mut total = ExtractionCounts::default();
        let file1 = ExtractionCounts {
            extracted: 5,
            gifs_routed: 2,
            converted: 3,
            skipped: 1,
        };
        let file2 = ExtractionCounts {
            extracted: 3,
            gifs_routed: 0,
            converted: 2,
            skipped: 0,
        };
        total.extracted += file1.extracted;
        total.gifs_routed += file1.gifs_routed;
        total.converted += file1.converted;
        total.skipped += file1.skipped;
        total.extracted += file2.extracted;
        total.gifs_routed += file2.gifs_routed;
        total.converted += file2.converted;
        total.skipped += file2.skipped;
        assert_eq!(total.extracted, 8);
        assert_eq!(total.gifs_routed, 2);
        assert_eq!(total.converted, 5);
        assert_eq!(total.skipped, 1);
    }

    #[test]
    fn test_extraction_config_construction() {
        use crate::convert::OutputFormat;
        use std::path::Path;

        // With conversion enabled
        let gif_dir = Path::new("/tmp/gifs");
        let config = ExtractionConfig {
            convert: Some(OutputFormat::Png),
            quality: 90,
            lossless: false,
            gif_output: Some(gif_dir.to_path_buf()),
        };
        assert!(config.convert.is_some());
        assert_eq!(config.quality, 90);
        assert!(!config.lossless);
        assert!(config.gif_output.is_some());

        // Without conversion (defaults)
        let config_none = ExtractionConfig {
            convert: None,
            quality: 85,
            lossless: false,
            gif_output: None,
        };
        assert!(config_none.convert.is_none());
        assert_eq!(config_none.quality, 85);
        assert!(config_none.gif_output.is_none());

        // Verify Debug derive works
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("ExtractionConfig"));
    }

    #[test]
    fn test_extraction_config_clone() {
        use crate::convert::OutputFormat;

        let config = ExtractionConfig {
            convert: Some(OutputFormat::Jpg),
            quality: 85,
            lossless: false,
            gif_output: None,
        };
        let config_copy = config.clone();
        assert_eq!(config.quality, config_copy.quality);
    }
}
