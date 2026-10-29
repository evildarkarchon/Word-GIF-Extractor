//! Tests for the argument structure and for turning parsed user options into one ready
//! Extraction run request.

use super::*;
use crate::test_support::no_fallback_directory;
use clap::Parser;
use std::cell::Cell;

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

/// Prepares a run that names its inputs, with a resolver that panics if asked.
///
/// Every request-shape test below names `book.epub`, so the fallback directory is
/// never needed; [`no_fallback_directory`] makes a regression there loud.
fn prepare_named<const N: usize>(args: [&str; N]) -> PreparedExtractionRun {
    let args = Args::try_parse_from(args).expect("test args should parse");
    prepare(args, no_fallback_directory).expect("extraction run intake should succeed")
}

/// Builds the request intake should produce for a run naming only `book.epub`.
///
/// The request is compared whole, so a test also proves that the flags it passes
/// changed nothing besides the policy it names. Recursion and the output
/// directory are left at their no-flags values; the inputs test spells its own.
fn expected_for_book(
    epub_filter: EpubFilter,
    epub_cover_policy: Option<EpubCoverPolicy>,
    image_write_policy: ImageWritePolicy,
) -> ExtractionRunRequest {
    ExtractionRunRequest::new(
        vec![PathBuf::from("book.epub")],
        false,
        None,
        epub_filter,
        epub_cover_policy,
        image_write_policy,
    )
}

/// The Image write policy of a run passing no format, conversion or GIF flags.
fn accepting_formats(allowed_formats: HashSet<ImageFormat>) -> ImageWritePolicy {
    ImageWritePolicy::new(allowed_formats, None, None)
}

#[test]
fn combines_positional_and_named_inputs() {
    let prepared = prepare_named([
        "test",
        "first.docx",
        "--input",
        "second.docx",
        "--output",
        "output",
    ]);

    assert!(prepared.notices.is_empty());
    assert_eq!(
        prepared.request,
        ExtractionRunRequest::new(
            vec![PathBuf::from("first.docx"), PathBuf::from("second.docx")],
            false,
            Some(PathBuf::from("output")),
            EpubFilter::default(),
            None,
            accepting_formats(ImageFormat::all_set()),
        )
    );
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
    let prepared = prepare_named(["test", "book.epub", "--formats", "png,unknown,jpeg"]);

    assert_eq!(
        prepared.notices,
        vec![PreRunNotice::IgnoredFormat {
            format: "unknown".to_string(),
        }]
    );
    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            None,
            accepting_formats(HashSet::from([ImageFormat::Png, ImageFormat::Jpg])),
        )
    );
}

#[test]
fn falls_back_to_all_formats_when_no_valid_formats_are_supplied() {
    let prepared = prepare_named(["test", "book.epub", "--formats", "unknown"]);

    assert_eq!(
        prepared.notices,
        vec![PreRunNotice::IgnoredFormat {
            format: "unknown".to_string(),
        }]
    );
    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            None,
            accepting_formats(ImageFormat::all_set()),
        )
    );
}

#[test]
fn gif_only_overrides_format_selection() {
    let prepared = prepare_named(["test", "book.epub", "--formats", "png,jpg", "--gif-only"]);

    assert!(prepared.notices.is_empty());
    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            None,
            accepting_formats(HashSet::from([ImageFormat::Gif])),
        )
    );
}

/// Verifies `--convert` alone builds the target's Conversion policy with no overrides.
///
/// What that policy's defaults are is Conversion policy's own subject; this pins
/// only that intake passes no quality or lossless setting the user did not ask for.
#[test]
fn builds_default_conversion_policy() {
    let prepared = prepare_named(["test", "book.epub", "--convert", "jpg"]);
    let conversion = ConversionPolicy::try_from(ConversionRequest {
        target: ConversionTarget::Jpg,
        quality: None,
        lossless: false,
    })
    .expect("a bare JPEG conversion request should be valid");

    assert!(prepared.notices.is_empty());
    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            None,
            ImageWritePolicy::new(ImageFormat::all_set(), Some(conversion), None),
        )
    );
}

#[test]
fn cover_only_builds_the_cover_only_policy() {
    let prepared = prepare_named(["test", "book.epub", "--cover-only"]);

    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            Some(EpubCoverPolicy::CoverOnly),
            accepting_formats(ImageFormat::all_set()),
        )
    );
}

#[test]
fn cover_fallback_builds_the_cover_then_normal_images_policy() {
    let prepared = prepare_named(["test", "book.epub", "--cover-only", "--cover-fallback"]);

    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            Some(EpubCoverPolicy::CoverThenNormalImages),
            accepting_formats(ImageFormat::all_set()),
        )
    );
}

#[test]
fn title_and_author_reach_the_epub_filter() {
    let prepared = prepare_named([
        "test",
        "book.epub",
        "--title",
        "Magic",
        "--author",
        "Tester",
    ]);

    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter {
                title: Some("Magic".to_string()),
                author: Some("Tester".to_string()),
            },
            None,
            accepting_formats(ImageFormat::all_set()),
        )
    );
}

#[test]
fn gif_output_reaches_the_image_write_policy() {
    let prepared = prepare_named(["test", "book.epub", "--gif-output", "gifs"]);

    assert_eq!(
        prepared.request,
        expected_for_book(
            EpubFilter::default(),
            None,
            ImageWritePolicy::new(ImageFormat::all_set(), None, Some(PathBuf::from("gifs"))),
        )
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
