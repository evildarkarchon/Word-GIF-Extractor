//! Extraction run intake for turning parsed user options into a ready run.

use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;

use crate::Args;
use crate::common::ExtractionConfig;
use crate::epub::EpubFilter;
use crate::extraction_run::RunOptions;
use crate::image_format::ImageFormat;

const DEFAULT_QUALITY: u8 = 85;

/// Prepared extraction run data consumed by the CLI adapter.
///
/// The extraction run options are the stable workflow interface. The remaining
/// fields are adapter facts needed to preserve user-facing messages after the
/// run has completed.
pub(crate) struct PreparedExtractionRun {
    /// Ready-to-run extraction workflow options.
    pub(crate) options: RunOptions,
    /// Whether conversion was requested by the user.
    pub(crate) has_convert: bool,
    /// GIF output directory for final summary rendering.
    pub(crate) gif_output: Option<PathBuf>,
    /// Current directory used when the user did not provide any input.
    pub(crate) defaulted_input: Option<PathBuf>,
    /// Format tokens ignored while preparing allowed image formats.
    pub(crate) ignored_formats: Vec<String>,
}

/// Prepares an extraction run from parsed CLI arguments.
///
/// This concentrates input fallback, format selection, GIF-only behavior,
/// conversion defaults, EPUB filters, and post-run summary facts behind one
/// intake interface.
pub(crate) fn prepare(args: Args) -> Result<PreparedExtractionRun> {
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

    let mut all_inputs: Vec<PathBuf> = inputs.into_iter().chain(named_inputs).collect();
    let defaulted_input = if all_inputs.is_empty() {
        let cwd = std::env::current_dir()?;
        all_inputs.push(cwd.clone());
        Some(cwd)
    } else {
        None
    };

    let (allowed_formats, ignored_formats) = select_allowed_formats(formats, gif_only);
    let quality = quality.unwrap_or(DEFAULT_QUALITY);
    let has_convert = convert.is_some();
    let gif_output_for_summary = gif_output.clone();

    let options = RunOptions {
        inputs: all_inputs,
        recursive,
        output,
        allowed_formats,
        cover_only,
        cover_fallback,
        epub_filter: EpubFilter { title, author },
        extraction: ExtractionConfig {
            convert,
            quality,
            lossless,
            gif_output,
        },
    };

    Ok(PreparedExtractionRun {
        options,
        has_convert,
        gif_output: gif_output_for_summary,
        defaulted_input,
        ignored_formats,
    })
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
mod tests {
    use super::*;
    use clap::Parser;

    fn prepare_from<const N: usize>(args: [&str; N]) -> PreparedExtractionRun {
        let args = Args::try_parse_from(args).expect("test args should parse");
        prepare(args).expect("extraction run intake should succeed")
    }

    #[test]
    fn combines_positional_and_named_inputs() {
        let prepared = prepare_from(["test", "first.docx", "--input", "second.epub"]);

        assert_eq!(
            prepared.options.inputs,
            vec![PathBuf::from("first.docx"), PathBuf::from("second.epub")]
        );
        assert!(prepared.defaulted_input.is_none());
    }

    #[test]
    fn defaults_to_current_directory_when_inputs_are_empty() {
        let prepared = prepare_from(["test"]);
        let cwd = std::env::current_dir().expect("current directory should be readable");

        assert_eq!(prepared.options.inputs, vec![cwd.clone()]);
        assert_eq!(prepared.defaulted_input, Some(cwd));
    }

    #[test]
    fn parses_allowed_formats_and_records_ignored_tokens() {
        let prepared = prepare_from(["test", "book.epub", "--formats", "png,unknown,jpeg"]);

        assert!(prepared.options.allowed_formats.contains(&ImageFormat::Png));
        assert!(prepared.options.allowed_formats.contains(&ImageFormat::Jpg));
        assert_eq!(prepared.ignored_formats, vec!["unknown"]);
    }

    #[test]
    fn falls_back_to_all_formats_when_no_valid_formats_are_supplied() {
        let prepared = prepare_from(["test", "book.epub", "--formats", "unknown"]);

        assert_eq!(prepared.options.allowed_formats, ImageFormat::all_set());
        assert_eq!(prepared.ignored_formats, vec!["unknown"]);
    }

    #[test]
    fn gif_only_overrides_format_selection() {
        let prepared = prepare_from(["test", "book.epub", "--formats", "png,jpg", "--gif-only"]);

        assert_eq!(
            prepared.options.allowed_formats,
            HashSet::from([ImageFormat::Gif])
        );
    }

    #[test]
    fn defaults_conversion_quality_to_85() {
        let prepared = prepare_from(["test", "book.epub", "--convert", "jpg"]);

        assert_eq!(prepared.options.extraction.quality, DEFAULT_QUALITY);
    }

    #[test]
    fn threads_conversion_and_summary_facts() {
        let prepared = prepare_from([
            "test",
            "book.epub",
            "--convert",
            "webp",
            "--quality",
            "90",
            "--gif-output",
            "gifs",
        ]);

        assert!(prepared.has_convert);
        assert_eq!(prepared.options.extraction.quality, 90);
        assert_eq!(prepared.gif_output, Some(PathBuf::from("gifs")));
        assert_eq!(
            prepared.options.extraction.gif_output,
            Some(PathBuf::from("gifs"))
        );
    }
}
