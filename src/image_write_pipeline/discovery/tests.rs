//! Tests for the format recognizers behind Archive image discovery.
//!
//! The recognizers are module-private, and they are tested directly on purpose.
//! Each one carries its own documented contract — the bounded magic-byte scan,
//! the declared-MIME normalization, the SVG search window and the EMF offset
//! check — and each contract is a value-to-value function with no I/O. Routing
//! these assertions back through `discover_image` would mean constructing a
//! source, an Image write purpose and an allow-set in order to observe that one
//! signature is recognized, which reproduces at smaller scale the coupling this
//! module's tests exist to remove. `discover_image` is tested separately, for
//! the facts that only it produces: evidence precedence, the bounded read
//! boundary, the fallback warning and the two-phase read.
//!
//! Window and limit boundaries are asserted against literal byte counts rather
//! than against the constants that define them; comparing a constant to itself
//! pins nothing, while the literal pins the documented contract.

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
