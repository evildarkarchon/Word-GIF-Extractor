//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! This tool treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats.

mod archive_image_discovery;
mod common;
mod convert;
mod docx;
mod epub;
mod extraction_run;
mod extraction_run_intake;
mod image_format;
mod image_writer;

use anyhow::Result;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};

use crate::convert::OutputFormat;
use crate::extraction_run::{RunEvent, RunObserver, RunReport};
use crate::extraction_run_intake::PreparedExtractionRun;

#[derive(Parser, Debug)]
#[command(author, version, about = "Extract images from Word (.docx) and EPUB files", long_about = None)]
struct Args {
    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    inputs: Vec<PathBuf>,

    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    #[arg(short = 'i', long = "input", num_args = 1..)]
    named_inputs: Vec<PathBuf>,

    /// Optional output directory (defaults to each input file's directory)
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

/// Adapts extraction-run events to terminal progress bars.
struct IndicatifRunObserver {
    scan_pb: Option<ProgressBar>,
    epub_filter_pb: Option<ProgressBar>,
    epub_dedup_pb: Option<ProgressBar>,
    extraction_pb: Option<ProgressBar>,
}

impl IndicatifRunObserver {
    /// Creates a progress observer with no active progress bars.
    fn new() -> Self {
        Self {
            scan_pb: None,
            epub_filter_pb: None,
            epub_dedup_pb: None,
            extraction_pb: None,
        }
    }

    /// Finishes the extraction progress bar with the final run summary.
    fn finish_extraction(&self, message: String) {
        if let Some(pb) = &self.extraction_pb {
            pb.finish_with_message(message);
        }
    }

    /// Runs a closure while the extraction progress bar is suspended, when present.
    fn suspend_extraction<F>(&self, f: F)
    where
        F: FnOnce(),
    {
        if let Some(pb) = &self.extraction_pb {
            pb.suspend(f);
        } else {
            f();
        }
    }
}

impl RunObserver for IndicatifRunObserver {
    fn on_event(&mut self, event: RunEvent) {
        match event {
            RunEvent::InputWarning { path } => {
                eprintln!("Warning: Input path does not exist: {}", path.display());
            }
            RunEvent::ScanStarted { use_spinner } => {
                if use_spinner {
                    let pb = ProgressBar::new_spinner();
                    pb.set_style(create_spinner_style());
                    pb.set_message("Scanning directories for documents...");
                    pb.enable_steady_tick(std::time::Duration::from_millis(100));
                    self.scan_pb = Some(pb);
                }
            }
            RunEvent::DocumentDiscovered { count } => {
                if let Some(pb) = &self.scan_pb {
                    pb.set_message(format!("Found {} document(s)...", count));
                }
            }
            RunEvent::ScanFinished { count } => {
                if let Some(pb) = self.scan_pb.take() {
                    pb.finish_with_message(format!("Found {} document(s)", count));
                }
            }
            RunEvent::EpubFilterStarted { description, total } => {
                let pb = ProgressBar::new(total as u64);
                pb.set_style(create_progress_style());
                pb.set_message(format!("Filtering EPUBs by {}", description));
                self.epub_filter_pb = Some(pb);
            }
            RunEvent::EpubFilterAdvanced => {
                if let Some(pb) = &self.epub_filter_pb {
                    pb.inc(1);
                }
            }
            RunEvent::EpubFilterWarning { path, message } => {
                if let Some(pb) = &self.epub_filter_pb {
                    pb.suspend(|| {
                        eprintln!("Warning: Could not read {}: {}", path.display(), message);
                    });
                } else {
                    eprintln!("Warning: Could not read {}: {}", path.display(), message);
                }
            }
            RunEvent::EpubFilterFinished { matching } => {
                if let Some(pb) = self.epub_filter_pb.take() {
                    pb.finish_with_message(format!("Found {} matching EPUB(s)", matching));
                }
            }
            RunEvent::EpubDedupStarted { total } => {
                let pb = ProgressBar::new(total as u64);
                pb.set_style(create_progress_style());
                pb.set_message("Deduplicating EPUBs by metadata");
                self.epub_dedup_pb = Some(pb);
            }
            RunEvent::EpubDedupAdvanced => {
                if let Some(pb) = &self.epub_dedup_pb {
                    pb.inc(1);
                }
            }
            RunEvent::EpubDedupFinished {
                duplicates_found,
                unique_remaining,
            } => {
                if let Some(pb) = self.epub_dedup_pb.take() {
                    if duplicates_found > 0 {
                        pb.finish_with_message(format!(
                            "Removed {} duplicate EPUB(s), {} unique remaining",
                            duplicates_found, unique_remaining
                        ));
                    } else {
                        pb.finish_and_clear();
                    }
                }
            }
            RunEvent::ExtractionStarted { total, cover_only } => {
                let pb = ProgressBar::new(total as u64);
                pb.set_style(create_progress_style());
                let extraction_msg = if cover_only {
                    "Extracting cover images"
                } else {
                    "Extracting images from documents"
                };
                pb.set_message(extraction_msg);
                self.extraction_pb = Some(pb);
            }
            RunEvent::DocumentStarted { display_name, .. } => {
                if let Some(pb) = &self.extraction_pb {
                    pb.set_message(display_name);
                }
            }
            RunEvent::DocumentError { path, message } => {
                self.suspend_extraction(|| {
                    eprintln!("Error processing {}: {}", path.display(), message);
                });
            }
            RunEvent::DocumentWarning { message, .. } => {
                self.suspend_extraction(|| {
                    eprintln!("Warning: {}", message);
                });
            }
            RunEvent::DocumentFinished { .. } => {
                if let Some(pb) = &self.extraction_pb {
                    pb.inc(1);
                }
            }
        }
    }
}

/// Builds the final extraction summary shown in the terminal.
fn final_summary_message(
    report: &RunReport,
    has_convert: bool,
    gif_output: Option<&Path>,
) -> String {
    if report.total_counts.extracted > 0 {
        // Only label as "cover(s)" if in cover-only mode AND no DOCX images were extracted
        // (DOCX files always extract all images regardless of cover_only flag)
        let item_name = if report.cover_only && !report.has_docx_images {
            "cover(s)"
        } else {
            "image(s)"
        };
        let has_gif_routing = report.total_counts.gifs_routed > 0;

        match (has_convert, has_gif_routing) {
            (true, true) => {
                // D-04: Combined conversion + GIF routing message
                let gif_dir = gif_output.unwrap();
                format!(
                    "Extracted {} {}, converted {}, skipped {}, routed {} GIF(s) to {} from {} document(s)",
                    report.total_counts.extracted,
                    item_name,
                    report.total_counts.converted,
                    report.total_counts.skipped,
                    report.total_counts.gifs_routed,
                    gif_dir.display(),
                    report.documents_with_output
                )
            }
            (true, false) => {
                // D-01: Conversion stats only
                format!(
                    "Extracted {} {}, converted {}, skipped {} from {} document(s)",
                    report.total_counts.extracted,
                    item_name,
                    report.total_counts.converted,
                    report.total_counts.skipped,
                    report.documents_with_output
                )
            }
            (false, true) => {
                // Existing GIF routing message (no conversion) -- unchanged
                let gif_dir = gif_output.unwrap();
                format!(
                    "Extracted {} {}, routed {} GIF(s) to {} from {} document(s)",
                    report.total_counts.extracted,
                    item_name,
                    report.total_counts.gifs_routed,
                    gif_dir.display(),
                    report.documents_with_output
                )
            }
            (false, false) => {
                // Existing default message -- unchanged
                format!(
                    "Extracted {} {} from {} document(s)",
                    report.total_counts.extracted, item_name, report.documents_with_output
                )
            }
        }
    } else if report.cover_only && !report.has_docx_images {
        "No cover images found".to_string()
    } else {
        "No images found".to_string()
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let PreparedExtractionRun {
        options,
        has_convert,
        gif_output,
        defaulted_input,
        ignored_formats,
    } = extraction_run_intake::prepare(args)?;

    if let Some(cwd) = defaulted_input {
        println!(
            "No input path specified, using current directory: {}",
            cwd.display()
        );
    }

    for format in ignored_formats {
        eprintln!("Warning: Unrecognized format '{}' ignored", format);
    }

    let mut observer = IndicatifRunObserver::new();
    let report = extraction_run::run(options, &mut observer)?;

    if report.documents_to_process == 0 {
        println!("No documents found to process.");
        return Ok(());
    }

    observer.finish_extraction(final_summary_message(
        &report,
        has_convert,
        gif_output.as_deref(),
    ));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ExtractionCounts;

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

    #[test]
    fn test_extraction_counts_conversion_fields() {
        let mut total = ExtractionCounts::default();
        let counts = ExtractionCounts {
            extracted: 10,
            gifs_routed: 2,
            converted: 7,
            skipped: 1,
        };
        total.extracted += counts.extracted;
        total.gifs_routed += counts.gifs_routed;
        total.converted += counts.converted;
        total.skipped += counts.skipped;
        assert_eq!(total.extracted, 10);
        assert_eq!(total.gifs_routed, 2);
        assert_eq!(total.converted, 7);
        assert_eq!(total.skipped, 1);
    }

    #[test]
    fn test_conversion_message_format() {
        let msg = format!(
            "Extracted {} {}, converted {}, skipped {} from {} document(s)",
            10, "image(s)", 7, 1, 3
        );
        assert!(msg.contains("converted 7"));
        assert!(msg.contains("skipped 1"));
        assert!(msg.contains("Extracted 10 image(s)"));
        assert!(msg.contains("from 3 document(s)"));
    }

    #[test]
    fn test_combined_conversion_gif_message_format() {
        let gif_dir = std::path::PathBuf::from("/tmp/gifs");
        let msg = format!(
            "Extracted {} {}, converted {}, skipped {}, routed {} GIF(s) to {} from {} document(s)",
            10,
            "image(s)",
            5,
            2,
            3,
            gif_dir.display(),
            4
        );
        assert!(msg.contains("converted 5"));
        assert!(msg.contains("skipped 2"));
        assert!(msg.contains("routed 3 GIF(s)"));
        assert!(msg.contains("/tmp/gifs"));
    }

    #[test]
    fn test_convert_and_lossless_args_threaded() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--lossless"]).unwrap();
        assert_eq!(args.convert, Some(OutputFormat::Webp));
        assert!(args.lossless);
    }
}
