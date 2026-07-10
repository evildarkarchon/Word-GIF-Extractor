//! Extraction run intake for turning parsed user options into a ready run.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use crate::Args;
use crate::conversion::{ConversionPolicy, ConversionPolicyError, ConversionRequest};
use crate::document_selection::EpubFilter;
use crate::extraction_run::RunOptions;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};

/// Failure while turning parsed user options into a ready Extraction run.
#[derive(Debug)]
pub(crate) enum ExtractionRunIntakeError {
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
pub(crate) fn prepare(args: Args) -> Result<PreparedExtractionRun, ExtractionRunIntakeError> {
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

    let has_convert = convert.is_some();
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
    let gif_output_for_summary = gif_output.clone();
    let image_write_pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        allowed_formats,
        conversion,
        gif_output,
    ));

    let options = RunOptions {
        inputs: all_inputs,
        recursive,
        output,
        cover_only,
        cover_fallback,
        epub_filter: EpubFilter { title, author },
        image_write_pipeline,
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
    use crate::image_write_pipeline::{ArchiveImageSource, ImageWriteRequest};
    use clap::Parser;
    use image::DynamicImage;
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn prepare_from<const N: usize>(args: [&str; N]) -> PreparedExtractionRun {
        let args = Args::try_parse_from(args).expect("test args should parse");
        prepare(args).expect("extraction run intake should succeed")
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-intake-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn write_sources(
        prepared: &PreparedExtractionRun,
        output_dir: &Path,
        sources: Vec<(ArchiveImageSource, Vec<u8>)>,
    ) -> crate::image_write_pipeline::ImageWriteResult {
        prepared
            .options
            .image_write_pipeline
            .write_from(
                ImageWriteRequest::normal_images(output_dir, "sample"),
                |visitor| {
                    for (source, data) in sources {
                        visitor.visit(source, &mut Cursor::new(data))?;
                    }
                    Ok(())
                },
            )
            .expect("prepared pipeline should write test sources")
    }

    fn valid_png() -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(1, 1)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .expect("test PNG should encode");
        cursor.into_inner()
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
        let temp_dir = temp_test_dir("selected-formats");

        let result = write_sources(
            &prepared,
            &temp_dir,
            vec![
                (
                    ArchiveImageSource::named("image.bin"),
                    b"\x89PNG\r\n\x1A\n".to_vec(),
                ),
                (
                    ArchiveImageSource::named("photo.bin"),
                    b"\xFF\xD8\xFF".to_vec(),
                ),
                (
                    ArchiveImageSource::named("animation.bin"),
                    b"GIF89a".to_vec(),
                ),
            ],
        );

        assert_eq!(result.counts.extracted, 2);
        assert_eq!(prepared.ignored_formats, vec!["unknown"]);
        assert!(temp_dir.join("sample_1.png").exists());
        assert!(temp_dir.join("sample_2.jpg").exists());
        assert!(!temp_dir.join("sample_3.gif").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn falls_back_to_all_formats_when_no_valid_formats_are_supplied() {
        let prepared = prepare_from(["test", "book.epub", "--formats", "unknown"]);
        let temp_dir = temp_test_dir("all-formats-fallback");

        let result = write_sources(
            &prepared,
            &temp_dir,
            vec![(ArchiveImageSource::named("vector.bin"), b"<svg/>".to_vec())],
        );

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(prepared.ignored_formats, vec!["unknown"]);
        assert!(temp_dir.join("sample.svg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn gif_only_overrides_format_selection() {
        let prepared = prepare_from(["test", "book.epub", "--formats", "png,jpg", "--gif-only"]);
        let temp_dir = temp_test_dir("gif-only");

        let result = write_sources(
            &prepared,
            &temp_dir,
            vec![
                (
                    ArchiveImageSource::named("image.bin"),
                    b"\x89PNG\r\n\x1A\n".to_vec(),
                ),
                (
                    ArchiveImageSource::named("animation.bin"),
                    b"GIF89a".to_vec(),
                ),
            ],
        );

        assert_eq!(result.counts.extracted, 1);
        assert!(temp_dir.join("sample.gif").exists());
        assert!(!temp_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn builds_default_conversion_policy() {
        let prepared = prepare_from(["test", "book.epub", "--convert", "jpg"]);
        let temp_dir = temp_test_dir("default-conversion");

        let result = write_sources(
            &prepared,
            &temp_dir,
            vec![(ArchiveImageSource::named("image.png"), valid_png())],
        );

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(result.counts.converted, 1);
        assert!(temp_dir.join("sample.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn retains_conversion_and_gif_summary_facts() {
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
        assert_eq!(prepared.gif_output, Some(PathBuf::from("gifs")));
    }

    #[test]
    fn returns_typed_conversion_policy_error() {
        let args =
            Args::try_parse_from(["test", "book.epub", "--convert", "png", "--quality", "90"])
                .expect("CLI syntax should parse before semantic validation");

        let error = match prepare(args) {
            Ok(_) => panic!("PNG quality should be rejected by intake"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ExtractionRunIntakeError::ConversionPolicy(
                ConversionPolicyError::QualityUnsupportedForPng
            )
        ));
    }
}
