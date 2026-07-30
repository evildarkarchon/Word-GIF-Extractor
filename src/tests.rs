//! Tests for the command-line argument structure owned by the crate root.
//!
//! These moved out of the binary with [`Args`](super::Args) itself: they read its
//! fields, so leaving them in `src/main.rs` would have meant publishing all fourteen
//! flags just to let a test look at them. They follow the type again when Extraction
//! run intake takes ownership of it.

use super::*;
use std::path::PathBuf;

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
