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

/// Structured fact that the terminal adapter renders before an Extraction run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreRunNotice {
    /// Intake selected the current directory because no input was requested.
    DefaultedInput { path: PathBuf },
    /// Intake ignored one unrecognized image-format token.
    IgnoredFormat { format: String },
}

/// Intake result containing one executable request and its ordered pre-run notices.
pub(crate) struct PreparedExtractionRun {
    /// Opaque request consumed by the Extraction run exactly once.
    pub(crate) request: ExtractionRunRequest,
    /// Ordered pre-run facts for adapter rendering.
    pub(crate) notices: Vec<PreRunNotice>,
}

/// Prepares an extraction run from parsed CLI arguments.
///
/// This concentrates input fallback, format selection, GIF-only behavior,
/// conversion defaults, EPUB filters, and production-valid request construction
/// behind one intake interface.
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
mod tests {
    use super::*;
    use crate::extraction_run::{
        ExtractionOutputKind, ExtractionRunObservation, ExtractionRunObserver,
        ExtractionRunOutcome, ProducedOutput, run,
    };
    use clap::Parser;
    use image::DynamicImage;
    use std::fs;
    use std::io::{Cursor, Write};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

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

    /// Ignores the live run timeline in intake-focused outcome tests.
    #[derive(Default)]
    struct SilentExtractionRunObserver;

    impl ExtractionRunObserver for SilentExtractionRunObserver {
        /// Ignores live observations because intake tests assert files and semantic outcomes.
        fn on_observation(&mut self, _observation: ExtractionRunObservation) {}
    }

    /// Writes one DOCX fixture containing the supplied archive sources.
    fn write_docx(input_path: &Path, sources: Vec<(&str, Vec<u8>)>) {
        let file = fs::File::create(input_path).expect("test DOCX should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        for (name, data) in sources {
            zip.start_file(name, SimpleFileOptions::default())
                .expect("ZIP entry should start");
            zip.write_all(&data)
                .expect("ZIP entry payload should be writable");
        }
        zip.finish().expect("test DOCX should finish");
    }

    /// Prepares and executes one archive-backed DOCX request through the public operation seam.
    fn run_docx(
        test_name: &str,
        sources: Vec<(&str, Vec<u8>)>,
        extra_args: &[&str],
    ) -> (PreparedExtractionRun, PathBuf, PathBuf) {
        let temp_dir = temp_test_dir(test_name);
        let input_path = temp_dir.join("input.docx");
        let output_dir = temp_dir.join("output");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        write_docx(&input_path, sources);

        let input = input_path.to_string_lossy().into_owned();
        let output = output_dir.to_string_lossy().into_owned();
        let mut args = vec!["test".to_string(), input, "--output".to_string(), output];
        args.extend(extra_args.iter().map(|argument| (*argument).to_string()));
        let args = Args::try_parse_from(args).expect("test args should parse");
        let prepared = prepare(args).expect("extraction run intake should succeed");

        (prepared, temp_dir, output_dir)
    }

    /// Executes one intake-produced request exactly once.
    fn execute(prepared: PreparedExtractionRun) -> ExtractionRunOutcome {
        let mut observer = SilentExtractionRunObserver;
        run(prepared.request, &mut observer)
    }

    /// Extracts produced-output facts from a semantic outcome.
    fn produced(outcome: ExtractionRunOutcome) -> ProducedOutput {
        match outcome {
            ExtractionRunOutcome::ProducedOutput(output) => output,
            other => {
                panic!("expected produced output, got {other:?}");
            }
        }
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
        let temp_dir = temp_test_dir("combined-inputs");
        let first = temp_dir.join("first.docx");
        let second = temp_dir.join("second.docx");
        let output_dir = temp_dir.join("output");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        write_docx(
            &first,
            vec![("word/media/first.png", b"\x89PNG\r\n\x1A\n".to_vec())],
        );
        write_docx(
            &second,
            vec![("word/media/second.png", b"\x89PNG\r\n\x1A\n".to_vec())],
        );
        let args = Args::try_parse_from([
            "test",
            first.to_string_lossy().as_ref(),
            "--input",
            second.to_string_lossy().as_ref(),
            "--output",
            output_dir.to_string_lossy().as_ref(),
        ])
        .expect("test args should parse");
        let prepared = prepare(args).expect("Extraction run intake should succeed");

        assert!(prepared.notices.is_empty());
        let output = produced(execute(prepared));
        assert_eq!(output.emitted_images(), 2);
        assert_eq!(output.documents_with_output(), 2);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn defaults_to_current_directory_when_inputs_are_empty() {
        let prepared = prepare_from(["test"]);
        let cwd = std::env::current_dir().expect("current directory should be readable");

        assert_eq!(
            prepared.notices,
            vec![PreRunNotice::DefaultedInput { path: cwd }]
        );
    }

    #[test]
    fn returns_defaulted_input_before_ignored_format_notices() {
        let prepared = prepare_from(["test", "--formats", "unknown"]);
        let cwd = std::env::current_dir().expect("current directory should be readable");

        assert_eq!(
            prepared.notices,
            vec![
                PreRunNotice::DefaultedInput { path: cwd },
                PreRunNotice::IgnoredFormat {
                    format: "unknown".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parses_allowed_formats_and_records_ignored_tokens() {
        let (prepared, temp_dir, output_dir) = run_docx(
            "selected-formats",
            vec![
                ("image.bin", b"\x89PNG\r\n\x1A\n".to_vec()),
                ("photo.bin", b"\xFF\xD8\xFF".to_vec()),
                ("animation.bin", b"GIF89a".to_vec()),
            ],
            &["--formats", "png,unknown,jpeg"],
        );

        assert_eq!(
            prepared.notices,
            vec![PreRunNotice::IgnoredFormat {
                format: "unknown".to_string(),
            }]
        );
        assert_eq!(produced(execute(prepared)).emitted_images(), 2);
        assert!(output_dir.join("input_1.png").exists());
        assert!(output_dir.join("input_2.jpg").exists());
        assert!(!output_dir.join("input_3.gif").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn falls_back_to_all_formats_when_no_valid_formats_are_supplied() {
        let (prepared, temp_dir, output_dir) = run_docx(
            "all-formats-fallback",
            vec![("vector.bin", b"<svg/>".to_vec())],
            &["--formats", "unknown"],
        );

        assert_eq!(
            prepared.notices,
            vec![PreRunNotice::IgnoredFormat {
                format: "unknown".to_string(),
            }]
        );
        assert_eq!(produced(execute(prepared)).emitted_images(), 1);
        assert!(output_dir.join("input.svg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn gif_only_overrides_format_selection() {
        let (prepared, temp_dir, output_dir) = run_docx(
            "gif-only",
            vec![
                ("image.bin", b"\x89PNG\r\n\x1A\n".to_vec()),
                ("animation.bin", b"GIF89a".to_vec()),
            ],
            &["--formats", "png,jpg", "--gif-only"],
        );

        assert_eq!(produced(execute(prepared)).emitted_images(), 1);
        assert!(output_dir.join("input.gif").exists());
        assert!(!output_dir.join("input.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn builds_default_conversion_policy() {
        let (prepared, temp_dir, output_dir) = run_docx(
            "default-conversion",
            vec![("image.png", valid_png())],
            &["--convert", "jpg"],
        );

        let output = produced(execute(prepared));
        assert_eq!(output.emitted_images(), 1);
        assert_eq!(
            output
                .conversion()
                .expect("conversion facts should apply")
                .converted_images(),
            1
        );
        assert!(output_dir.join("input.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn builds_validated_epub_cover_extraction_policy() {
        let (prepared, temp_dir, _) = run_docx(
            "cover-policy",
            Vec::new(),
            &["--cover-only", "--cover-fallback"],
        );

        assert_eq!(
            execute(prepared),
            ExtractionRunOutcome::NoOutput(ExtractionOutputKind::Covers)
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
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
