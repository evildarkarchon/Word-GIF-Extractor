//! Focused tests for Image format recognition and bounded-reader mechanics.
//!
//! Purpose policy is exercised through the pipeline visitors. These tests keep
//! precise signature, read-window, and accepted-prefix assertions close to the
//! implementation. Literal byte counts pin the documented evidence boundary.

use std::io::Cursor;

use super::*;

const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR";

/// One magic-evidence fixture naming the format its payload must identify as.
///
/// The canonical extension for every format is asserted by the Image format
/// module's own tests, so this table deliberately carries no extension column.
struct MagicFormatCase {
    name: &'static str,
    format: ImageFormat,
    payload: Vec<u8>,
}

/// Returns one representative magic-byte payload per supported signature.
fn magic_format_cases() -> Vec<MagicFormatCase> {
    let mut emf = vec![0x01, 0x00, 0x00, 0x00];
    emf.resize(40, 0);
    emf.extend_from_slice(b" EMF payload");

    vec![
        MagicFormatCase {
            name: "jpeg",
            format: ImageFormat::Jpg,
            payload: b"\xFF\xD8\xFF\xE0jpeg payload".to_vec(),
        },
        MagicFormatCase {
            name: "png",
            format: ImageFormat::Png,
            payload: MINIMAL_PNG.to_vec(),
        },
        MagicFormatCase {
            name: "gif",
            format: ImageFormat::Gif,
            payload: b"GIF89a payload".to_vec(),
        },
        MagicFormatCase {
            name: "bmp",
            format: ImageFormat::Bmp,
            payload: b"BM bitmap payload".to_vec(),
        },
        MagicFormatCase {
            name: "tiff-little-endian",
            format: ImageFormat::Tiff,
            payload: b"II\x2A\x00tiff payload".to_vec(),
        },
        MagicFormatCase {
            name: "tiff-big-endian",
            format: ImageFormat::Tiff,
            payload: b"MM\x00\x2Atiff payload".to_vec(),
        },
        MagicFormatCase {
            name: "svg",
            format: ImageFormat::Svg,
            payload: b"<?xml version=\"1.0\"?><svg/>".to_vec(),
        },
        MagicFormatCase {
            name: "wmf-placeable",
            format: ImageFormat::Wmf,
            payload: b"\xD7\xCD\xC6\x9Awmf payload".to_vec(),
        },
        MagicFormatCase {
            name: "wmf-standard",
            format: ImageFormat::Wmf,
            payload: b"\x01\x00\x09\x00wmf payload".to_vec(),
        },
        MagicFormatCase {
            name: "emf",
            format: ImageFormat::Emf,
            payload: emf,
        },
        MagicFormatCase {
            name: "webp",
            format: ImageFormat::Webp,
            payload: b"RIFF\x00\x00\x00\x00WEBP payload".to_vec(),
        },
        MagicFormatCase {
            name: "ico",
            format: ImageFormat::Ico,
            payload: b"\x00\x00\x01\x00ico payload".to_vec(),
        },
    ]
}

#[test]
fn magic_evidence_identifies_every_supported_format() {
    for case in magic_format_cases() {
        assert_eq!(
            format_from_magic(&case.payload),
            Some(case.format),
            "{} evidence should identify its format",
            case.name
        );
    }
}

#[test]
fn short_or_incomplete_magic_evidence_identifies_nothing() {
    assert_eq!(format_from_magic(b""), None);
    assert_eq!(format_from_magic(b"\x89"), None);
    // The PNG signature is eight bytes; seven of them are not evidence of PNG.
    assert_eq!(format_from_magic(&MINIMAL_PNG[..7]), None);
    assert_eq!(format_from_magic(b"unknown payload"), None);
}

#[test]
fn riff_evidence_without_the_webp_marker_identifies_nothing() {
    // The marker occupies bytes 8..12, so eleven bytes cannot reach it.
    let too_short = b"RIFF\x00\x00\x00\x00WEB";
    assert_eq!(too_short.len(), 11);
    assert_eq!(format_from_magic(too_short), None);

    assert_eq!(format_from_magic(b"RIFF\x00\x00\x00\x00WAVE payload"), None);
}

#[test]
fn emf_prefix_without_the_signature_at_its_offset_is_not_emf() {
    let mut prefix_only = vec![0x01, 0x00, 0x00, 0x00];
    // The signature occupies bytes 40..44, so forty-three bytes cannot reach it.
    prefix_only.resize(43, 0);
    assert!(!is_emf(&prefix_only));

    let mut wrong_signature = vec![0x01, 0x00, 0x00, 0x00];
    wrong_signature.resize(40, 0);
    wrong_signature.extend_from_slice(b" WMF payload");
    assert!(!is_emf(&wrong_signature));

    let mut at_the_offset = vec![0x01, 0x00, 0x00, 0x00];
    at_the_offset.resize(40, 0);
    at_the_offset.extend_from_slice(b" EMF payload");
    assert!(is_emf(&at_the_offset));
}

#[test]
fn svg_marker_matches_only_when_a_documented_delimiter_follows() {
    // A tag whose name merely starts with the marker is a different element.
    assert!(!is_svg(b"<svgfoo>"));

    for delimited in [
        b"<svg ".as_slice(),
        b"<svg\t",
        b"<svg\r",
        b"<svg\n",
        b"<svg/",
        b"<svg>",
        b"<svg",
    ] {
        assert!(
            is_svg(delimited),
            "{delimited:?} should be recognized as SVG evidence"
        );
    }

    assert!(is_svg(b"<?xml version=\"1.0\"?><SVG xmlns=\"...\">"));
}

#[test]
fn svg_marker_matches_at_the_last_window_position_but_not_past_it() {
    // The window is the 1,024 bytes following an optional three-byte BOM. A
    // four-byte marker therefore fits when it starts at window position 1020.
    let mut ending_inside = b"\xEF\xBB\xBF".to_vec();
    ending_inside.extend(std::iter::repeat_n(b' ', 1020));
    ending_inside.extend_from_slice(b"<svg");
    assert!(is_svg(&ending_inside));

    let mut one_byte_past = b"\xEF\xBB\xBF".to_vec();
    one_byte_past.extend(std::iter::repeat_n(b' ', 1021));
    one_byte_past.extend_from_slice(b"<svg");
    assert!(!is_svg(&one_byte_past));
}

#[test]
fn declared_mime_is_normalized_before_it_is_looked_up() {
    assert_eq!(format_from_mime("IMAGE/PNG"), Some(ImageFormat::Png));
    assert_eq!(
        format_from_mime("image/svg+xml; charset=utf-8"),
        Some(ImageFormat::Svg)
    );
    assert_eq!(format_from_mime("text/plain"), None);
}

/// Returns the discovery outcome that accepts `data` as `format`, carrying `warnings`.
///
/// Every accepted-source assertion below compares against one of these, so the
/// payload is asserted as a value rather than read back out of a match arm.
fn accepted_with(
    data: Vec<u8>,
    format: ImageFormat,
    warnings: Vec<ImageWriteWarning>,
) -> DiscoveredImage {
    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Accepted(AcceptedImage { data, format }),
        warnings,
    }
}

/// Returns the discovery outcome that accepts `data` as `format`, with no warning.
fn accepted(data: Vec<u8>, format: ImageFormat) -> DiscoveredImage {
    accepted_with(data, format, Vec::new())
}

/// Returns the discovery outcome that completes without emission and without a warning.
fn completed_silently() -> DiscoveredImage {
    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Completed,
        warnings: Vec::new(),
    }
}

#[test]
fn unidentified_normal_source_completes_silently_after_the_bounded_read() {
    let source = NormalImageSource::named("word/media/image.bin");
    let mut reader = Cursor::new(vec![0; 4096]);

    let discovered = discover_normal_image(source, &mut reader, &HashSet::from([ImageFormat::Png]));

    assert_eq!(discovered, completed_silently());
    assert_eq!(reader.position(), 1027);
}

#[test]
fn filtered_source_consumes_the_whole_evidence_window() {
    let source = NormalImageSource::named("word/media/animation.gif");
    let mut payload = vec![0; 4096];
    payload[..6].copy_from_slice(b"GIF89a");
    let mut reader = Cursor::new(payload);

    let discovered = discover_normal_image(source, &mut reader, &HashSet::from([ImageFormat::Png]));

    // The format is identifiable from byte 0, so the read boundary is not
    // coupled to how early identification succeeds: the window is consumed
    // first, then the decision is made.
    assert_eq!(discovered, completed_silently());
    assert_eq!(reader.position(), 1027);
}

#[test]
fn accepted_source_retains_its_evidence_prefix_and_appends_the_remainder() {
    let source = NormalImageSource::named("word/media/image.bin");
    let mut original = vec![0; 4096];
    original[..MINIMAL_PNG.len()].copy_from_slice(MINIMAL_PNG);
    for (index, byte) in original[MINIMAL_PNG.len()..].iter_mut().enumerate() {
        // A varying tail, so a payload rebuilt from the wrong offset cannot
        // compare equal to the original by accident.
        *byte = (index % 251) as u8;
    }
    let mut reader = Cursor::new(original.clone());

    let discovered = discover_normal_image(source, &mut reader, &HashSet::from([ImageFormat::Png]));

    assert_eq!(discovered, accepted(original.clone(), ImageFormat::Png));
    assert_eq!(reader.position(), original.len() as u64);
}

#[test]
fn bom_prefixed_svg_ending_inside_the_window_is_accepted() {
    let source = NormalImageSource::named("word/media/vector.bin");
    let mut svg = b"\xEF\xBB\xBF".to_vec();
    svg.extend(std::iter::repeat_n(b' ', 1019));
    svg.extend_from_slice(b"<svg>");
    // The bounded read and the SVG window meet exactly here: three BOM bytes
    // plus the 1,024-byte window is the whole evidence prefix.
    assert_eq!(svg.len(), 1027);
    let mut reader = Cursor::new(svg.clone());

    let discovered = discover_normal_image(source, &mut reader, &HashSet::from([ImageFormat::Svg]));

    assert_eq!(discovered, accepted(svg, ImageFormat::Svg));
    // The payload ends exactly at the read boundary, so acceptance must not have
    // gone looking for a tail that is not there.
    assert_eq!(reader.position(), 1027);
}

#[test]
fn bom_prefixed_svg_beyond_the_window_completes_at_the_read_boundary() {
    let source = NormalImageSource::named("word/media/vector.bin");
    let mut svg = b"\xEF\xBB\xBF".to_vec();
    svg.extend(std::iter::repeat_n(b' ', 1024));
    svg.extend_from_slice(b"<svg>");
    let mut reader = Cursor::new(svg);

    let discovered = discover_normal_image(source, &mut reader, &HashSet::from([ImageFormat::Svg]));

    assert_eq!(discovered, completed_silently());
    assert_eq!(reader.position(), 1027);
}
