//! Tests for the argument structure and for turning parsed user options into one ready
//! Extraction run request.

use super::*;
use crate::extraction_run::run;
use crate::extraction_run_observation::{
    ExtractionOutputKind, ExtractionRunOutcome, ProducedOutput,
};
use crate::test_support::{
    SilentExtractionRunObserver, TempTestPath, no_fallback_directory, temp_test_dir, valid_png,
    write_docx,
};
use clap::Parser;
use std::cell::Cell;
use std::fs;

/// Fallback directory the resolver in [`prepare_from`] hands to intake.
///
/// Deliberately not the process working directory, so a test seeing it in a
/// notice knows the path came from the injected resolver and not from a matching
/// read of process state.
const FALLBACK_DIRECTORY: &str = "fallback-directory";

/// Prepares a run from argument strings with a resolver that always succeeds.
fn prepare_from<const N: usize>(args: [&str; N]) -> PreparedExtractionRun {
    let args = Args::try_parse_from(args).expect("test args should parse");
    prepare(args, || Ok(PathBuf::from(FALLBACK_DIRECTORY)))
        .expect("extraction run intake should succeed")
}

/// Prepares and executes one archive-backed DOCX request through the public operation seam.
///
/// The returned guard owns the temporary directory. A caller binds it to a named
/// variable, `_temp_dir` when unused, because `_` would drop it at once and remove
/// the directory while the test is still reading its output.
fn run_docx(
    test_name: &str,
    sources: Vec<(&str, Vec<u8>)>,
    extra_args: &[&str],
) -> (PreparedExtractionRun, TempTestPath, PathBuf) {
    let temp_dir = temp_test_dir("intake", test_name);
    let input_path = temp_dir.join("input.docx");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let entries: Vec<(&str, &[u8])> = sources
        .iter()
        .map(|(name, data)| (*name, data.as_slice()))
        .collect();
    write_docx(&input_path, &entries);

    let input = input_path.to_string_lossy().into_owned();
    let output = output_dir.to_string_lossy().into_owned();
    let mut args = vec!["test".to_string(), input, "--output".to_string(), output];
    args.extend(extra_args.iter().map(|argument| (*argument).to_string()));
    let args = Args::try_parse_from(args).expect("test args should parse");
    let prepared =
        prepare(args, no_fallback_directory).expect("extraction run intake should succeed");

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

#[test]
fn combines_positional_and_named_inputs() {
    let temp_dir = temp_test_dir("intake", "combined-inputs");
    let first = temp_dir.join("first.docx");
    let second = temp_dir.join("second.docx");
    let output_dir = temp_dir.join("output");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_docx(&first, &[("word/media/first.png", b"\x89PNG\r\n\x1A\n")]);
    write_docx(&second, &[("word/media/second.png", b"\x89PNG\r\n\x1A\n")]);
    let args = Args::try_parse_from([
        "test",
        first.to_string_lossy().as_ref(),
        "--input",
        second.to_string_lossy().as_ref(),
        "--output",
        output_dir.to_string_lossy().as_ref(),
    ])
    .expect("test args should parse");
    let prepared =
        prepare(args, no_fallback_directory).expect("Extraction run intake should succeed");

    assert!(prepared.notices.is_empty());
    let output = produced(execute(prepared));
    assert_eq!(output.emitted_images(), 2);
    assert_eq!(output.documents_with_output(), 2);
}

#[test]
fn defaults_to_current_directory_when_inputs_are_empty() {
    let prepared = prepare_from(["test"]);

    assert_eq!(
        prepared.notices,
        vec![PreRunNotice::DefaultedInput {
            path: PathBuf::from(FALLBACK_DIRECTORY)
        }]
    );
}

/// Verifies an unresolvable fallback directory is reported as its own intake failure.
#[test]
fn reports_an_unresolvable_fallback_directory() {
    let args = Args::try_parse_from(["test"]).expect("test args should parse");

    let error = match prepare(args, || {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "working directory was removed",
        ))
    }) {
        Ok(_) => panic!("an unresolvable fallback directory should fail intake"),
        Err(error) => error,
    };

    match error {
        ExtractionRunIntakeError::CurrentDirectory(error) => {
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
            assert_eq!(error.to_string(), "working directory was removed");
        }
        other => panic!("expected a current-directory failure, got {other:?}"),
    }
}

/// Verifies a run that names its input never asks for the fallback directory.
///
/// Every named-input test already passes a resolver that panics when called; this
/// one states the rule outright, and counts rather than panics so that a failure
/// reads as the rule it breaks.
#[test]
fn named_inputs_never_resolve_the_fallback_directory() {
    let args = Args::try_parse_from(["test", "book.epub"]).expect("test args should parse");
    let resolutions = Cell::new(0);

    let prepared = prepare(args, || {
        resolutions.set(resolutions.get() + 1);
        Ok(PathBuf::from(FALLBACK_DIRECTORY))
    })
    .expect("extraction run intake should succeed");

    assert_eq!(resolutions.get(), 0);
    assert!(prepared.notices.is_empty());
}

#[test]
fn returns_defaulted_input_before_ignored_format_notices() {
    let prepared = prepare_from(["test", "--formats", "unknown"]);

    assert_eq!(
        prepared.notices,
        vec![
            PreRunNotice::DefaultedInput {
                path: PathBuf::from(FALLBACK_DIRECTORY)
            },
            PreRunNotice::IgnoredFormat {
                format: "unknown".to_string(),
            },
        ]
    );
}

#[test]
fn parses_allowed_formats_and_records_ignored_tokens() {
    let (prepared, _temp_dir, output_dir) = run_docx(
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
}

#[test]
fn falls_back_to_all_formats_when_no_valid_formats_are_supplied() {
    let (prepared, _temp_dir, output_dir) = run_docx(
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
}

#[test]
fn gif_only_overrides_format_selection() {
    let (prepared, _temp_dir, output_dir) = run_docx(
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
}

#[test]
fn builds_default_conversion_policy() {
    let (prepared, _temp_dir, output_dir) = run_docx(
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
}

#[test]
fn builds_validated_epub_cover_extraction_policy() {
    let (prepared, _temp_dir, _) = run_docx(
        "cover-policy",
        Vec::new(),
        &["--cover-only", "--cover-fallback"],
    );

    assert_eq!(
        execute(prepared),
        ExtractionRunOutcome::NoOutput {
            output_kind: ExtractionOutputKind::Covers,
            failed_documents: None
        }
    );
}

#[test]
fn returns_typed_conversion_policy_error() {
    let args = Args::try_parse_from(["test", "book.epub", "--convert", "png", "--quality", "90"])
        .expect("CLI syntax should parse before semantic validation");

    let error = match prepare(args, no_fallback_directory) {
        Ok(_) => panic!("PNG quality should be rejected by intake"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ExtractionRunIntakeError::ConversionPolicy(ConversionPolicyError::QualityUnsupportedForPng)
    ));
}

// Argument-parsing tests. These followed [`Args`] here from the crate root, which is
// where they waited while the binary still owned the type: they read its fields, so
// they have to live wherever the fields are visible.

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
fn test_gif_only_and_gif_output_both_set() {
    let args = Args::try_parse_from(["test", "--gif-only", "--gif-output", "/tmp/gifs"]).unwrap();
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
fn test_convert_and_lossless_args_threaded() {
    let args = Args::try_parse_from(["test", "--convert", "webp", "--lossless"]).unwrap();
    assert_eq!(args.convert, Some(ConversionTargetArg::Webp));
    assert!(args.lossless);
}

/// Verifies the test-only [`Default`] matches what `clap` parses with no flags supplied.
///
/// Run-level tests build [`Args`] from this default and override two or three fields, so
/// a drift between the two would silently change what those tests actually request. The
/// whole-value comparison is deliberate: a per-field version would keep passing after a
/// fifteenth flag arrived.
#[test]
fn default_args_match_an_invocation_with_no_flags() {
    let parsed = Args::try_parse_from(["test"]).expect("an invocation with no flags should parse");

    assert_eq!(parsed, Args::default());
}
