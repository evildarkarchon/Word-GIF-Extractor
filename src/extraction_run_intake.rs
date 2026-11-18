//! Extraction run intake for turning parsed user options into a ready run.
//!
//! Intake owns the command-line argument structure it consumes. That keeps the
//! module graph acyclic — nothing here reaches back up to the crate root, which in
//! turn reaches back down for the module tree — and it keeps the sixteen flags
//! defined in exactly one place. `clap` in the library interface is the accepted
//! cost of that: the alternative is a sixteen-field parallel structure with a
//! mapping to keep in sync.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::PathBuf;

use clap::builder::{PossibleValue, TypedValueParser};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Arg, Command, Parser, ValueEnum};

use crate::conversion::{
    ConversionPolicy, ConversionPolicyError, ConversionRequest, ConversionTarget,
};
use crate::document_extraction::EpubCoverPolicy;
use crate::document_selection::EpubFilter;
use crate::extraction_run::ExtractionRunRequest;
use crate::image_format::ImageFormat;
use crate::image_size_limit::ImageSizeLimit;
use crate::image_write_pipeline::ImageWritePolicy;

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

/// Parses one `--formats` token into its canonical Image format.
///
/// Spelling belongs to [`ImageFormat::from_user_format`], so the aliases it accepts
/// (`jpeg`, `tif`, any case, a leading dot) are accepted here too, with no second
/// list to keep in step. A mirrored [`ValueEnum`], as [`ConversionTargetArg`] uses,
/// would have to repeat every alias. The canonical extensions are advertised as the
/// possible values, so `--help` lists them and a rejected token's error does too.
#[derive(Debug, Clone, Copy)]
struct ImageFormatArgParser;

impl ImageFormatArgParser {
    /// Returns the canonical spelling of every Image format, in vocabulary order.
    fn canonical_names() -> impl Iterator<Item = &'static str> {
        ImageFormat::all().iter().map(|format| format.extension())
    }
}

impl TypedValueParser for ImageFormatArgParser {
    type Value = ImageFormat;

    /// Accepts a recognized format token and rejects anything else at parse time.
    ///
    /// An unrecognized token is an [`ErrorKind::InvalidValue`] error carrying the
    /// same context `clap` attaches to its own enumerated arguments, so it renders
    /// as `invalid value 'pgn' for '--formats <FORMATS>'` followed by the possible
    /// values, and the process exits with the usage status 2. An empty token, as in
    /// `png,,jpg`, renders as a missing value instead.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidUtf8`] for a token that is not UTF-8, and
    /// [`ErrorKind::InvalidValue`] for one that names no Image format.
    fn parse_ref(
        &self,
        cmd: &Command,
        arg: Option<&Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let Some(token) = value.to_str() else {
            return Err(clap::Error::new(ErrorKind::InvalidUtf8).with_cmd(cmd));
        };
        if let Some(format) = ImageFormat::from_user_format(token) {
            return Ok(format);
        }

        let mut error = clap::Error::new(ErrorKind::InvalidValue).with_cmd(cmd);
        // The rich formatter only renders an InvalidValue error that carries both
        // the argument and the value; without them it falls back to a bare
        // "invalid value" line that names neither.
        error.insert(
            ContextKind::InvalidArg,
            ContextValue::String(arg.map_or_else(|| "...".to_owned(), ToString::to_string)),
        );
        error.insert(
            ContextKind::InvalidValue,
            ContextValue::String(token.to_owned()),
        );
        error.insert(
            ContextKind::ValidValue,
            ContextValue::Strings(Self::canonical_names().map(str::to_owned).collect()),
        );
        Err(error)
    }

    /// Lists the canonical spellings for help output; aliases stay unadvertised.
    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(Self::canonical_names().map(PossibleValue::new)))
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
/// sixteenth flag cannot drift the two apart unnoticed.
#[derive(Parser, Debug)]
#[cfg_attr(test, derive(Default, PartialEq))]
#[command(author, version, about = "Extract images from Office (.docx, .pptx, .xlsx, .vsdx), OpenDocument and EPUB files", long_about = None)]
pub struct Args {
    /// Paths to input documents (.docx, .pptx, .xlsx, .odt, .epub, ...) or directories (defaults to current directory)
    pub(crate) inputs: Vec<PathBuf>,

    /// Paths to input documents (.docx, .pptx, .xlsx, .odt, .epub, ...) or directories (defaults to current directory)
    #[arg(short = 'i', long = "input", num_args = 1..)]
    pub(crate) named_inputs: Vec<PathBuf>,

    /// Optional output directory (defaults to each input file's directory)
    #[arg(short, long)]
    pub(crate) output: Option<PathBuf>,

    /// Recursively search for documents if input is a directory
    #[arg(short, long)]
    pub(crate) recursive: bool,

    /// Image formats to extract (e.g., "png,jpg"). Defaults to all supported formats.
    // One value per occurrence, split on commas. An open-ended `num_args` would keep
    // consuming arguments, so `-f png,jpg book.docx` would swallow the document path
    // as a format token and the run would fall back to the current directory.
    // Repeating the flag still accumulates, because `Vec` fields append by default.
    // Every token must name a format: a filter that ignored a typo used to widen the
    // run to every format when nothing else was recognized. See ADR-0027.
    #[arg(
        short,
        long,
        value_delimiter = ',',
        num_args = 1,
        action = clap::ArgAction::Append,
        value_parser = ImageFormatArgParser
    )]
    pub(crate) formats: Option<Vec<ImageFormat>>,

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

    /// Process every EPUB, even when several declare the same book
    #[arg(long)]
    pub(crate) keep_duplicates: bool,

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

    /// Skip any image larger than SIZE, with a warning (bytes, or K, M, G binary suffix)
    // The default is the type's own spelling of its default, which a test pins to
    // parse to that value, so `--help` and the policy cannot disagree. Parsing goes
    // through the type's `FromStr`, so a zero, a decimal unit such as `MB` or an
    // overflow is a usage error before anything runs, as ADR-0027 does for format
    // names.
    #[arg(long, value_name = "SIZE", default_value = ImageSizeLimit::DEFAULT_SPELLING)]
    pub(crate) max_image_size: ImageSizeLimit,
}

/// Failure while turning parsed user options into a ready Extraction run.
#[derive(Debug)]
pub enum ExtractionRunIntakeError {
    /// The fallback input directory could not be resolved.
    CurrentDirectory(io::Error),
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
}

/// Intake result containing one executable request and its ordered pre-run notices.
pub struct PreparedExtractionRun {
    /// Plain-data request consumed by the Extraction run exactly once.
    pub request: ExtractionRunRequest,
    /// Ordered pre-run facts for adapter rendering.
    pub notices: Vec<PreRunNotice>,
}

/// Prepares an extraction run from parsed CLI arguments.
///
/// This concentrates input fallback, format selection, GIF-only behavior,
/// conversion defaults, EPUB filters, and production-valid request construction
/// behind one intake interface.
///
/// `current_dir` resolves the fallback input directory, and intake reads no
/// process state of its own. It is called only when no input was named, and only
/// after the conversion request has been validated: a run that names its inputs
/// must keep working from a working directory that can no longer be resolved.
/// Production passes [`std::env::current_dir`].
///
/// # Errors
///
/// Returns [`ExtractionRunIntakeError::ConversionPolicy`] when the conversion
/// options do not form a valid policy, and
/// [`ExtractionRunIntakeError::CurrentDirectory`] when the fallback directory was
/// needed and `current_dir` failed to resolve it.
pub fn prepare(
    args: Args,
    current_dir: impl FnOnce() -> io::Result<PathBuf>,
) -> Result<PreparedExtractionRun, ExtractionRunIntakeError> {
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
        keep_duplicates,
        convert,
        quality,
        lossless,
        gif_only,
        gif_output,
        max_image_size,
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
        let cwd = current_dir().map_err(ExtractionRunIntakeError::CurrentDirectory)?;
        all_inputs.push(cwd.clone());
        Some(cwd)
    } else {
        None
    };

    let allowed_formats = select_allowed_formats(formats, gif_only);
    // A run that seeks no covers carries no cover policy: the absence is the
    // request for normal images, so there is no value to hand a document kind
    // that has no covers. See ADR-0010.
    let epub_cover_policy = match (cover_only, cover_fallback) {
        (false, _) => None,
        (true, false) => Some(EpubCoverPolicy::CoverOnly),
        (true, true) => Some(EpubCoverPolicy::CoverThenNormalImages),
    };

    // Intake selects all four Image write policy ingredients, so it assembles the
    // policy here and hands the finished value over. The request keeps it as a
    // value; the Extraction run builds the Image write pipeline from it.
    let image_write_policy = ImageWritePolicy::new(allowed_formats, conversion, gif_output)
        .with_image_size_limit(max_image_size);

    let request = ExtractionRunRequest::new(
        all_inputs,
        recursive,
        output,
        EpubFilter { title, author },
        keep_duplicates,
        epub_cover_policy,
        image_write_policy,
    );
    let mut notices = Vec::new();
    if let Some(path) = defaulted_input {
        notices.push(PreRunNotice::DefaultedInput { path });
    }

    Ok(PreparedExtractionRun { request, notices })
}

/// Resolves the image formats accepted by one extraction run.
///
/// Every token was already parsed into an Image format, so there is nothing left to
/// reject here. No `--formats` flag accepts every format, and `--gif-only` replaces
/// whatever was requested with GIF alone.
fn select_allowed_formats(
    formats: Option<Vec<ImageFormat>>,
    gif_only: bool,
) -> HashSet<ImageFormat> {
    if gif_only {
        return HashSet::from([ImageFormat::Gif]);
    }
    formats.map_or_else(ImageFormat::all_set, |formats| {
        formats.into_iter().collect()
    })
}

#[cfg(test)]
mod tests;
