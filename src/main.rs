//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.
//!
//! This tool treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats.

mod conversion;
mod document_extraction;
mod document_selection;
mod extraction_run;
mod extraction_run_intake;
mod image_format;
mod image_write_pipeline;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};

use crate::conversion::{ConversionPolicyError, ConversionTarget};
use crate::document_selection::{
    DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionPhaseStatus,
    DocumentSelectionProgress, DocumentSelectionScanScope, EpubFilter, EpubMetadataPurpose,
};
use crate::extraction_run::{RunEvent, RunObserver, RunReport};
use crate::extraction_run_intake::{ExtractionRunIntakeError, PreparedExtractionRun};

/// Conversion target spelling accepted by the CLI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ConversionTargetArg {
    /// JPEG output.
    Jpg,
    /// PNG output.
    Png,
    /// WebP output.
    Webp,
}

impl From<ConversionTargetArg> for ConversionTarget {
    /// Maps CLI target spelling to the Clap-independent conversion target.
    fn from(target: ConversionTargetArg) -> Self {
        match target {
            ConversionTargetArg::Jpg => ConversionTarget::Jpg,
            ConversionTargetArg::Png => ConversionTarget::Png,
            ConversionTargetArg::Webp => ConversionTarget::Webp,
        }
    }
}

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
    convert: Option<ConversionTargetArg>,

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

/// Formats EPUB filter criteria for terminal progress messages.
fn epub_filter_description(filter: &EpubFilter) -> String {
    match (&filter.author, &filter.title) {
        (Some(author), Some(title)) => format!("author '{}' and title '{}'", author, title),
        (Some(author), None) => format!("author '{}'", author),
        (None, Some(title)) => format!("title '{}'", title),
        (None, None) => String::new(),
    }
}

/// Renders typed Extraction run intake failures using CLI-specific wording.
fn render_intake_error(error: ExtractionRunIntakeError) -> anyhow::Error {
    match error {
        ExtractionRunIntakeError::CurrentDirectory(error) => error.into(),
        ExtractionRunIntakeError::ConversionPolicy(error) => match error {
            ConversionPolicyError::QualityOutOfRange { quality } => {
                anyhow::anyhow!("--quality must be between 1 and 100 (got {quality})")
            }
            ConversionPolicyError::QualityUnsupportedForPng => anyhow::anyhow!(
                "--quality cannot be used with --convert png (PNG is a lossless format)"
            ),
            ConversionPolicyError::LosslessUnsupportedForTarget { .. } => {
                anyhow::anyhow!("--lossless can only be used with --convert webp")
            }
            ConversionPolicyError::LosslessConflictsWithQuality => {
                anyhow::anyhow!("--lossless cannot be used with --quality")
            }
        },
    }
}

/// Adapts Extraction run and Document selection facts to terminal progress bars.
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

    /// Runs a closure while the supplied progress bar is suspended, when present.
    fn suspend_progress_bar<F>(progress_bar: Option<&ProgressBar>, f: F)
    where
        F: FnOnce(),
    {
        if let Some(pb) = progress_bar {
            pb.suspend(f);
        } else {
            f();
        }
    }
}

impl DocumentSelectionObserver for IndicatifRunObserver {
    /// Renders one Document selection progress snapshot without relying on event deltas.
    fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
        match progress {
            DocumentSelectionProgress::Scanning {
                scope,
                discovered,
                status,
            } => match status {
                DocumentSelectionPhaseStatus::Running => {
                    if scope == DocumentSelectionScanScope::RecursiveDirectories
                        && self.scan_pb.is_none()
                    {
                        let pb = ProgressBar::new_spinner();
                        pb.set_style(create_spinner_style());
                        pb.set_message("Scanning directories for documents...");
                        pb.enable_steady_tick(std::time::Duration::from_millis(100));
                        self.scan_pb = Some(pb);
                    }
                    if discovered > 0
                        && let Some(pb) = &self.scan_pb
                    {
                        pb.set_message(format!("Found {} document(s)...", discovered));
                    }
                }
                DocumentSelectionPhaseStatus::Finished => {
                    if let Some(pb) = self.scan_pb.take() {
                        pb.finish_with_message(format!("Found {} document(s)", discovered));
                    }
                }
            },
            DocumentSelectionProgress::FilteringEpubs {
                filter,
                checked,
                total,
                matching,
                status,
            } => match status {
                DocumentSelectionPhaseStatus::Running => {
                    if self.epub_filter_pb.is_none() {
                        let pb = ProgressBar::new(total as u64);
                        pb.set_style(create_progress_style());
                        pb.set_message(format!(
                            "Filtering EPUBs by {}",
                            epub_filter_description(&filter)
                        ));
                        self.epub_filter_pb = Some(pb);
                    }
                    if let Some(pb) = &self.epub_filter_pb {
                        pb.set_position(checked as u64);
                    }
                }
                DocumentSelectionPhaseStatus::Finished => {
                    if let Some(pb) = self.epub_filter_pb.take() {
                        pb.set_position(checked as u64);
                        pb.finish_with_message(format!("Found {} matching EPUB(s)", matching));
                    }
                }
            },
            DocumentSelectionProgress::DeduplicatingEpubs {
                checked,
                total,
                duplicates_found,
                unique_remaining,
                status,
            } => match status {
                DocumentSelectionPhaseStatus::Running => {
                    if self.epub_dedup_pb.is_none() {
                        let pb = ProgressBar::new(total as u64);
                        pb.set_style(create_progress_style());
                        pb.set_message("Deduplicating EPUBs by metadata");
                        self.epub_dedup_pb = Some(pb);
                    }
                    if let Some(pb) = &self.epub_dedup_pb {
                        pb.set_position(checked as u64);
                    }
                }
                DocumentSelectionPhaseStatus::Finished => {
                    if let Some(pb) = self.epub_dedup_pb.take() {
                        pb.set_position(checked as u64);
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
            },
        }
    }

    /// Renders one structured Document selection diagnostic with terminal wording.
    fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        match diagnostic {
            DocumentSelectionDiagnostic::MissingInput { path } => {
                eprintln!("Warning: Input path does not exist: {}", path.display());
            }
            DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                path,
                purpose,
                detail,
            } => match purpose {
                EpubMetadataPurpose::Filtering => {
                    let render = || {
                        eprintln!("Warning: Could not read {}: {}", path.display(), detail);
                    };
                    Self::suspend_progress_bar(self.epub_filter_pb.as_ref(), render);
                }
                EpubMetadataPurpose::Deduplication => {
                    let render = || {
                        eprintln!(
                            "Warning: Could not read EPUB metadata from {} during deduplication; using filename fallback: {}",
                            path.display(),
                            detail
                        );
                    };
                    Self::suspend_progress_bar(self.epub_dedup_pb.as_ref(), render);
                }
            },
        }
    }
}

impl RunObserver for IndicatifRunObserver {
    fn on_event(&mut self, event: RunEvent) {
        match event {
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
                Self::suspend_progress_bar(self.extraction_pb.as_ref(), || {
                    eprintln!("Error processing {}: {}", path.display(), message);
                });
            }
            RunEvent::DocumentWarning { message, .. } => {
                Self::suspend_progress_bar(self.extraction_pb.as_ref(), || {
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
        // EPUB fallback and DOCX output are normal images even during a cover-only run.
        // Label output as covers only when every emitted file used required-cover purpose.
        let item_name = if report.cover_only && !report.has_normal_image_output {
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
    } else if report.cover_only && !report.has_normal_image_output {
        "No cover images found".to_string()
    } else {
        "No images found".to_string()
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let PreparedExtractionRun {
        options,
        has_convert,
        gif_output,
        defaulted_input,
        ignored_formats,
    } = extraction_run_intake::prepare(args).map_err(render_intake_error)?;

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
    use crate::image_write_pipeline::ImageWriteCounts;

    #[test]
    fn test_convert_flag_parses_all_formats() {
        let args = Args::try_parse_from(["test", "--convert", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Jpg));
        let args = Args::try_parse_from(["test", "--convert", "png"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Png));
        let args = Args::try_parse_from(["test", "--convert", "webp"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Webp));
    }

    #[test]
    fn terminal_epub_filter_description_preserves_existing_wording() {
        assert_eq!(
            epub_filter_description(&EpubFilter {
                title: Some("Magic Book".to_string()),
                author: Some("Test Author".to_string()),
            }),
            "author 'Test Author' and title 'Magic Book'"
        );
    }

    #[test]
    fn test_convert_short_flag() {
        let args = Args::try_parse_from(["test", "-C", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Jpg));
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
        let err_msg = extraction_run_intake::prepare(args)
            .err()
            .map(render_intake_error)
            .expect("PNG quality should fail semantic intake")
            .to_string();
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
        let err_msg = extraction_run_intake::prepare(args)
            .err()
            .map(render_intake_error)
            .expect("JPEG lossless should fail semantic intake")
            .to_string();
        assert!(
            err_msg.contains("--lossless can only be used with --convert webp"),
            "Error was: {}",
            err_msg
        );
    }

    #[test]
    fn test_lossless_with_png_error() {
        let args = Args::try_parse_from(["test", "--convert", "png", "--lossless"]).unwrap();
        let err_msg = extraction_run_intake::prepare(args)
            .err()
            .map(render_intake_error)
            .expect("PNG lossless should fail semantic intake")
            .to_string();
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
        let counts = ImageWriteCounts {
            extracted: 5,
            gifs_routed: 2,
            converted: 0,
            skipped: 0,
        };
        assert!(counts.gifs_routed > 0);
        let counts = ImageWriteCounts {
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
        let mut total = ImageWriteCounts::default();
        let counts = ImageWriteCounts {
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
    fn conversion_summary_reports_preserved_matching_source_as_unconverted() {
        let report = RunReport {
            total_counts: ImageWriteCounts {
                extracted: 1,
                converted: 0,
                ..ImageWriteCounts::default()
            },
            documents_with_output: 1,
            ..RunReport::default()
        };

        let message = final_summary_message(&report, true, None);

        assert_eq!(
            message,
            "Extracted 1 image(s), converted 0, skipped 0 from 1 document(s)"
        );
    }

    #[test]
    fn combined_conversion_and_gif_summary_uses_run_report() {
        let gif_dir = std::path::PathBuf::from("/tmp/gifs");
        let report = RunReport {
            total_counts: ImageWriteCounts {
                extracted: 10,
                converted: 5,
                skipped: 2,
                gifs_routed: 3,
            },
            documents_with_output: 4,
            ..RunReport::default()
        };

        let message = final_summary_message(&report, true, Some(&gif_dir));

        assert_eq!(
            message,
            format!(
                "Extracted 10 image(s), converted 5, skipped 2, routed 3 GIF(s) to {} from 4 document(s)",
                gif_dir.display()
            )
        );
    }

    #[test]
    fn epub_cover_fallback_summary_reports_normal_images() {
        let report = RunReport {
            total_counts: ImageWriteCounts {
                extracted: 2,
                ..ImageWriteCounts::default()
            },
            documents_with_output: 1,
            has_normal_image_output: true,
            cover_only: true,
            documents_to_process: 1,
        };

        let message = final_summary_message(&report, false, None);

        assert_eq!(message, "Extracted 2 image(s) from 1 document(s)");
    }

    #[test]
    fn test_convert_and_lossless_args_threaded() {
        let args = Args::try_parse_from(["test", "--convert", "webp", "--lossless"]).unwrap();
        assert_eq!(args.convert, Some(ConversionTargetArg::Webp));
        assert!(args.lossless);
    }
}
