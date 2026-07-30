//! Extraction run intake for turning parsed user options into a ready run.
//!
//! Intake owns the command-line argument structure it consumes. That keeps the
//! module graph acyclic — nothing here reaches back up to the crate root, which in
//! turn reaches back down for the module tree — and it keeps the fourteen flags
//! defined in exactly one place. `clap` in the library interface is the accepted
//! cost of that: the alternative is a fourteen-field parallel structure with a
//! mapping to keep in sync.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::conversion::{
    ConversionPolicy, ConversionPolicyError, ConversionRequest, ConversionTarget,
};
use crate::document_extraction::DocumentExtractionPolicy;
use crate::document_selection::EpubFilter;
use crate::extraction_run::ExtractionRunRequest;
use crate::image_format::ImageFormat;

/// Conversion target spelling accepted by the CLI adapter.
///
/// Crate-visible to match the field that names it: a narrower visibility trips
/// `private_interfaces`, because [`Args::convert`] is reachable crate-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ConversionTargetArg {
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
/// The fields are crate-visible rather than public: outside the crate this is only
/// ever parsed and handed to [`prepare`], while in-crate tests build it directly so
/// that run-level tests do not have to spell command-line flags to reach the run.
///
/// [`Default`] is derived only under `cfg(test)`, where it matches what `clap`
/// produces for an invocation with no flags. It exists so a test can name the two
/// or three options it cares about and leave the rest alone; it is not part of what
/// the library promises. [`PartialEq`] is test-only for the same reason: it lets one
/// test hold that default against a real parse without enumerating the fields, so a
/// fifteenth flag cannot drift the two apart unnoticed.
#[derive(Parser, Debug)]
#[cfg_attr(test, derive(Default, PartialEq))]
#[command(author, version, about = "Extract images from Word (.docx) and EPUB files", long_about = None)]
pub struct Args {
    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    pub(crate) inputs: Vec<PathBuf>,

    /// Paths to input .docx/.epub files or directories (defaults to current directory)
    #[arg(short = 'i', long = "input", num_args = 1..)]
    pub(crate) named_inputs: Vec<PathBuf>,

    /// Optional output directory (defaults to each input file's directory)
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,

    /// Recursively search for .docx/.epub files if input is a directory
    #[arg(short, long)]
    pub(crate) recursive: bool,

    /// Image formats to extract (e.g., "png,jpg"). Defaults to all supported formats.
    #[arg(short, long, value_delimiter = ',', num_args = 0..)]
    pub(crate) formats: Option<Vec<String>>,

    /// Extract only cover image from EPUB files
    #[arg(short = 'c', long)]
    pub(crate) cover_only: bool,

    /// Fallback to extracting all images if cover not found (EPUB only, requires --cover-only)
    #[arg(long, requires = "cover_only")]
    pub(crate) cover_fallback: bool,

    /// Filter EPUB files by title (case-insensitive substring match)
    #[arg(long)]
    pub(crate) title: Option<String>,

    /// Filter EPUB files by author (case-insensitive substring match)
    #[arg(long)]
    pub(crate) author: Option<String>,

    /// Convert extracted images to specified format (jpg, png, webp)
    #[arg(short = 'C', long, conflicts_with = "gif_only")]
    pub(crate) convert: Option<ConversionTargetArg>,

    /// JPEG/WebP encoding quality override (1-100, default: 85)
    #[arg(short = 'q', long, requires = "convert", conflicts_with = "lossless", value_parser = clap::value_parser!(u8).range(1..=100))]
    pub(crate) quality: Option<u8>,

    /// Use lossless WebP encoding instead of lossy
    #[arg(short = 'L', long, requires = "convert", conflicts_with = "quality")]
    pub(crate) lossless: bool,

    /// Extract only GIF files (skip all other image formats)
    #[arg(short = 'g', long, conflicts_with = "convert")]
    pub(crate) gif_only: bool,

    /// Separate output directory for GIF files
    #[arg(short = 'G', long)]
    pub(crate) gif_output: Option<PathBuf>,
}

/// Failure while turning parsed user options into a ready Extraction run.
#[derive(Debug)]
pub enum ExtractionRunIntakeError {
    /// The fallback input directory could not be resolved.
    CurrentDirectory(std::io::Error),
    /// The requested conversion facts did not form a valid Conversion policy.
    ConversionPolicy(ConversionPolicyError),
}

impl fmt::Display for ExtractionRunIntakeError {
    /// Formats the underlying intake failure without CLI-specific flag wording.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractionRunIntakeError::CurrentDirectory(error) => error.fmt(formatter),
            ExtractionRunIntakeError::ConversionPolicy(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExtractionRunIntakeError {
    /// Returns the lower module failure that prevented intake from completing.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExtractionRunIntakeError::CurrentDirectory(error) => Some(error),
            ExtractionRunIntakeError::ConversionPolicy(error) => Some(error),
        }
    }
}

/// Structured fact that the terminal adapter renders before an Extraction run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreRunNotice {
    /// Intake selected the current directory because no input was requested.
    DefaultedInput { path: PathBuf },
    /// Intake ignored one unrecognized image-format token.
    IgnoredFormat { format: String },
}

/// Intake result containing one executable request and its ordered pre-run notices.
pub struct PreparedExtractionRun {
    /// Opaque request consumed by the Extraction run exactly once.
    pub request: ExtractionRunRequest,
    /// Ordered pre-run facts for adapter rendering.
    pub notices: Vec<PreRunNotice>,
}

/// Prepares an extraction run from parsed CLI arguments.
///
/// This concentrates input fallback, format selection, GIF-only behavior,
/// conversion defaults, EPUB filters, and production-valid request construction
/// behind one intake interface.
pub fn prepare(args: Args) -> Result<PreparedExtractionRun, ExtractionRunIntakeError> {
    let Args {
        inputs,
        named_inputs,
        output,
        recursive,
        formats,
        cover_only,
        cover_fallback,
        title,
        author,
        convert,
        quality,
        lossless,
        gif_only,
        gif_output,
    } = args;

    let conversion = convert
        .map(|target| {
            ConversionPolicy::try_from(ConversionRequest {
                target: target.into(),
                quality,
                lossless,
            })
        })
        .transpose()
        .map_err(ExtractionRunIntakeError::ConversionPolicy)?;

    let mut all_inputs: Vec<PathBuf> = inputs.into_iter().chain(named_inputs).collect();
    let defaulted_input = if all_inputs.is_empty() {
        let cwd = std::env::current_dir().map_err(ExtractionRunIntakeError::CurrentDirectory)?;
        all_inputs.push(cwd.clone());
        Some(cwd)
    } else {
        None
    };

    let (allowed_formats, ignored_formats) = select_allowed_formats(formats, gif_only);
    let document_extraction_policy = if cover_only {
        DocumentExtractionPolicy::EpubCover {
            fallback_to_normal_images: cover_fallback,
        }
    } else {
        DocumentExtractionPolicy::NormalImages
    };

    let request = ExtractionRunRequest::new(
        all_inputs,
        recursive,
        output,
        EpubFilter { title, author },
        document_extraction_policy,
        allowed_formats,
        conversion,
        gif_output,
    );
    let mut notices = Vec::new();
    if let Some(path) = defaulted_input {
        notices.push(PreRunNotice::DefaultedInput { path });
    }
    notices.extend(
        ignored_formats
            .into_iter()
            .map(|format| PreRunNotice::IgnoredFormat { format }),
    );

    Ok(PreparedExtractionRun { request, notices })
}

/// Resolves the image formats accepted by one extraction run.
///
/// Unknown user tokens are returned to the adapter for warning output. If the
/// user supplies no valid formats, the extractor preserves the existing
/// compatibility behavior of accepting every supported image format.
fn select_allowed_formats(
    formats: Option<Vec<String>>,
    gif_only: bool,
) -> (HashSet<ImageFormat>, Vec<String>) {
    let mut target_formats = HashSet::new();
    let mut ignored_formats = Vec::new();

    if let Some(formats) = formats {
        for fmt in formats {
            let normalized = fmt.trim();
            if let Some(format) = ImageFormat::from_user_format(normalized) {
                target_formats.insert(format);
            } else {
                ignored_formats.push(normalized.to_string());
            }
        }
    }

    if target_formats.is_empty() {
        target_formats = ImageFormat::all_set();
    }

    if gif_only {
        target_formats.clear();
        target_formats.insert(ImageFormat::Gif);
    }

    (target_formats, ignored_formats)
}

#[cfg(test)]
mod tests;
