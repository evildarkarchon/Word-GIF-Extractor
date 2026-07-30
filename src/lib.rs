//! Word Image Extractor - extraction of images from DOCX and EPUB files.
//!
//! This library treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats. It owns the whole module tree; `src/main.rs` links
//! against it and adds only the terminal presentation the CLI needs.
//!
//! # Interface
//!
//! Every module below is crate-private, and the only items reachable from outside are
//! the ones re-exported at the root. That set is deliberately temporary and wider than
//! it will end up: terminal handling still lives in the binary, so the observation
//! vocabulary it renders has to cross the library boundary. Once Extraction run
//! presentation moves in here, the interface collapses to a single entry point and the
//! rest of this list goes back to being crate-private.
//!
//! Two things about the set are worth knowing before adding to it.
//!
//! Some types are public but not re-exported — the Extraction run request and the
//! produced-output facts. They appear inside re-exported signatures, so they have to be
//! public, but nothing outside needs to name them: the binary destructures the prepared
//! run and hands the request straight back. Leaving them unnameable is the narrower
//! choice, and re-exporting a type merely because it is already public is not a reason.
//!
//! [`ConversionFacts`], [`GifRoutingFacts`] and [`ExtractionRunOutcome::try_produced`]
//! are the one concession: they are published for the binary's terminal-summary tests,
//! which have to build a produced outcome to render it, and which cannot move in here
//! while the rendering they test is still in the binary. They are the whole of that
//! concession, and it closes when presentation moves. Nothing else should be widened to
//! let a test reach in — in-crate tests stay in-crate for exactly that reason, and a
//! test whose subject lives in the library belongs in the library with it.

mod conversion;
mod document_extraction;
mod document_selection;
mod epub_declarations;
mod extraction_run;
mod extraction_run_intake;
mod image_format;
mod image_write_pipeline;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

use crate::conversion::ConversionTarget;

pub use crate::conversion::ConversionPolicyError;
pub use crate::document_extraction::DocumentExtractionWarning;
pub use crate::document_selection::{
    DocumentSelectionDiagnostic, DocumentSelectionPhaseStatus, DocumentSelectionProgress,
    DocumentSelectionScanScope, EpubFilter, EpubMetadataPurpose,
};
pub use crate::extraction_run::{
    ConversionFacts, ExtractionOutputKind, ExtractionRunObservation, ExtractionRunObserver,
    ExtractionRunOutcome, GifRoutingFacts, run as execute_extraction_run,
};
pub use crate::extraction_run_intake::{
    ExtractionRunIntakeError, PreRunNotice, PreparedExtractionRun,
    prepare as prepare_extraction_run,
};

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

/// Parsed user options for one extraction run.
///
/// The fields stay private: the binary only parses this and hands it to
/// [`prepare_extraction_run`], and Extraction run intake reads the fields as a
/// descendant of this module. That keeps the fourteen flags defined exactly once
/// without publishing them as a structure anyone outside the crate can build.
#[derive(Parser, Debug)]
#[command(author, version, about = "Extract images from Word (.docx) and EPUB files", long_about = None)]
pub struct Args {
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
