//! Extraction run intake for turning parsed user options into a ready run.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use crate::Args;
use crate::conversion::{ConversionPolicy, ConversionPolicyError, ConversionRequest};
use crate::document_extraction::DocumentExtractionPolicy;
use crate::document_selection::EpubFilter;
use crate::extraction_run::ExtractionRunRequest;
use crate::image_format::ImageFormat;

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
