//! Tests for the format recognizers and acquisition entry point of Archive image discovery.
//!
//! The recognizers are module-private, and they are tested directly on purpose.
//! Each one carries its own documented contract — the bounded magic-byte scan,
//! the declared-MIME normalization, the SVG search window and the EMF offset
//! check — and each contract is a value-to-value function with no I/O. Routing
//! these assertions back through `discover_image` would mean constructing a
//! source, with its Image write purpose, and an allow-set in order to observe
//! that one signature is recognized, which reproduces at smaller scale the
//! coupling this module's tests exist to remove. `discover_image` is tested
//! below, separately from the recognizers, for the facts that only it produces:
//! evidence precedence, the bounded read boundary, the fallback warning, the
//! decisions that differ by the source's Image write purpose and the two-phase
//! read.
//!
//! The entry-point tests run over in-memory readers, with no archive and no
//! filesystem: the returned discovery outcome, its accepted payload and its
//! ordered warning facts are the whole external behaviour, and asserting them as
//! values is what distinguishes a non-emitting outcome from a failed
//! acquisition. Both Image write purposes are driven here so that their
//! divergent handling of unidentified evidence — silent completion for a normal
//! image, defaulting with a warning for a required cover — reads side by side.
//!
//! Window and limit boundaries are asserted against literal byte counts rather
//! than against the constants that define them; comparing a constant to itself
//! pins nothing, while the literal pins the documented contract.

use std::io::Cursor;

use super::*;
use crate::test_support::{FailAfterReader, MINIMAL_PNG};

/// One magic-evidence fixture naming the format its payload must identify as.
///
/// The canonical extension for every format is asserted by the Image format
/// module's own tests, so this table deliberately carries no extension column.
struct MagicFormatCase {
    name: &'static str,
    format: ImageFormat,
    payload: Vec<u8>,
}

/// Returns a BMP file header followed by a DIB header that declares `dib_size`.
fn bmp_header(dib_size: u32) -> Vec<u8> {
    let mut bmp = b"BM".to_vec();
    // File size, two reserved words and the pixel-data offset; their values are
    // not part of the signature check.
    bmp.extend_from_slice(&[0; 12]);
    bmp.extend_from_slice(&dib_size.to_le_bytes());
    bmp.extend_from_slice(b"bitmap payload");
    bmp
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
            payload: bmp_header(40),
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

/// The opening of Calibre's EPUB title page: XHTML that wraps the cover in inline SVG.
const CALIBRE_TITLE_PAGE: &[u8] = br#"<?xml version='1.0' encoding='utf-8'?>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en">
    <head>
        <title>Cover</title>
    </head>
    <body>
        <div>
            <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1" width="100%" height="100%" viewBox="0 0 600 800" preserveAspectRatio="none">
                <image width="600" height="800" xlink:href="cover.jpeg"/>
            </svg>
        </div>
    </body>
</html>"#;

/// Verifies a document with an inline `<svg>` below another root is not SVG evidence.
#[test]
fn inline_svg_inside_another_root_is_not_svg_evidence() {
    assert!(!is_svg(CALIBRE_TITLE_PAGE));
    assert_eq!(format_from_magic(CALIBRE_TITLE_PAGE), None);
    assert!(!is_svg(b"<html><svg/></html>"));
    assert!(!is_svg(b"text before <svg>"));
}

/// Verifies the prolog constructs a real SVG file may open with are skipped.
#[test]
fn svg_root_is_found_after_the_xml_prolog() {
    for svg in [
        b"  \r\n\t<svg>".as_slice(),
        b"<?xml version=\"1.0\" standalone=\"no\"?>\n<svg xmlns=\"...\">",
        b"<?xml version=\"1.0\"?><?xml-stylesheet href=\"a.css\"?><svg/>",
        b"<!-- Generator: Adobe Illustrator --> <svg>",
        b"<!-- a > inside a comment --><svg>",
        b"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n<svg>",
        b"<!doctype svg [ <!ENTITY ns \"http://www.w3.org/2000/svg\"> ]><svg>",
        b"<?xml version=\"1.0\"?>\n<!-- c -->\n<!DOCTYPE svg>\n<!-- d -->\n<svg width=\"1\">",
    ] {
        assert!(
            is_svg(svg),
            "{:?} should be recognized as SVG evidence",
            String::from_utf8_lossy(svg)
        );
    }
}

/// Verifies an unterminated prolog construct is not evidence of what follows it.
#[test]
fn unterminated_prolog_is_not_svg_evidence() {
    assert!(!is_svg(b"<?xml version=\"1.0\" <svg>"));
    assert!(!is_svg(b"<!-- never closed <svg>"));
    assert!(!is_svg(b"<!DOCTYPE svg [ <svg>"));
}

/// Verifies a bare `BM` prefix is not BMP evidence without a known DIB header size.
#[test]
fn bmp_requires_a_known_dib_header_size() {
    for size in [12, 16, 40, 52, 56, 64, 108, 124] {
        assert_eq!(
            format_from_magic(&bmp_header(size)),
            Some(ImageFormat::Bmp),
            "DIB header size {size}"
        );
    }
    assert_eq!(format_from_magic(b"BM bitmap payload"), None);
    assert_eq!(format_from_magic(&bmp_header(41)), None);
    // Shorter than the file header plus the DIB size field.
    assert_eq!(format_from_magic(&bmp_header(40)[..17]), None);
}

/// Verifies an XHTML page wrapping its image in inline SVG is not written as an image.
///
/// EPUB normal-image extraction offers every manifest resource to discovery, and
/// magic bytes outrank the declared MIME, so before the root-element requirement a
/// Calibre title page was accepted as an `.svg` image containing HTML.
#[test]
fn xhtml_page_with_inline_svg_completes_silently() {
    let source =
        ArchiveImageSource::named("OEBPS/titlepage.xhtml").with_mime("application/xhtml+xml");
    let mut reader = Cursor::new(CALIBRE_TITLE_PAGE.to_vec());

    let discovered = discover_image(
        &source,
        &mut reader,
        &ImageFormat::all_set(),
        ImageSizeLimit::DEFAULT,
    );

    assert_eq!(discovered, completed_silently());
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

/// Returns the discovery outcome that accepts `data` as `format` from the source
/// named `source_name`, carrying `warnings`.
///
/// Every accepted-source assertion below compares against one of these, so the
/// payload is asserted as a value rather than read back out of a match arm. The
/// source name is spelled out by each test, which pins that discovery hands the
/// source's diagnostic name on to image preparation.
fn accepted_with(
    source_name: &str,
    data: Vec<u8>,
    format: ImageFormat,
    warnings: Vec<ImageWriteWarning>,
) -> DiscoveredImage {
    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Accepted(AcceptedImage {
            source_name: source_name.to_string(),
            data,
            format,
        }),
        warnings,
    }
}

/// Returns the discovery outcome that accepts `data` as `format` from the source
/// named `source_name`, with no warning.
fn accepted(source_name: &str, data: Vec<u8>, format: ImageFormat) -> DiscoveredImage {
    accepted_with(source_name, data, format, Vec::new())
}

/// Returns the discovery outcome that completes without emission and without a warning.
fn completed_silently() -> DiscoveredImage {
    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Completed,
        warnings: Vec::new(),
    }
}

#[test]
fn magic_evidence_outranks_a_conflicting_extension_and_mime() {
    let source = ArchiveImageSource::named("word/media/image.jpg").with_mime("image/gif");
    let mut reader = Cursor::new(MINIMAL_PNG.to_vec());

    let discovered = discover_image(
        &source,
        &mut reader,
        &ImageFormat::all_set(),
        ImageSizeLimit::DEFAULT,
    );

    // The empty warning list is half the point: the fallback warning belongs to
    // extension evidence alone, and a normal source's `.jpg` name never got to speak.
    assert_eq!(
        discovered,
        accepted(
            "word/media/image.jpg",
            MINIMAL_PNG.to_vec(),
            ImageFormat::Png
        )
    );
}

#[test]
fn path_extension_outranks_mime_and_emits_the_fallback_warning() {
    let source = ArchiveImageSource::named("word/media/image.png").with_mime("image/jpeg");
    let payload = b"not actually a png".to_vec();
    let mut reader = Cursor::new(payload.clone());

    let discovered = discover_image(
        &source,
        &mut reader,
        &ImageFormat::all_set(),
        ImageSizeLimit::DEFAULT,
    );

    // The warning carries the source name and the format it fell back to, which
    // is what makes it Archive image discovery's warning rather than any Image
    // write purpose's — no purpose-specific decision runs on this path.
    assert_eq!(
        discovered,
        accepted_with(
            "word/media/image.png",
            payload,
            ImageFormat::Png,
            vec![ImageWriteWarning::ExtensionFallback {
                source_name: "word/media/image.png".to_string(),
                format: ImageFormat::Png,
            }]
        )
    );
}

#[test]
fn declared_mime_identifies_only_after_magic_and_extension_evidence_fail() {
    let source = ArchiveImageSource::named("media/image.bin").with_mime("image/png");
    let payload = b"unknown bytes".to_vec();
    let mut reader = Cursor::new(payload.clone());

    let discovered = discover_image(
        &source,
        &mut reader,
        &ImageFormat::all_set(),
        ImageSizeLimit::DEFAULT,
    );

    // `.bin` is not an image extension, so the extension rung is skipped rather
    // than failed — and MIME evidence emits no fallback warning of its own.
    assert_eq!(
        discovered,
        accepted("media/image.bin", payload, ImageFormat::Png)
    );
}

#[test]
fn unidentified_normal_source_completes_silently_after_the_bounded_read() {
    let source = ArchiveImageSource::named("word/media/image.bin");
    let mut reader = Cursor::new(vec![0; 4096]);

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        ImageSizeLimit::DEFAULT,
    );

    assert_eq!(discovered, completed_silently());
    assert_eq!(reader.position(), 1027);
}

#[test]
fn filtered_source_consumes_the_whole_evidence_window() {
    let source = ArchiveImageSource::named("word/media/animation.gif");
    let mut payload = vec![0; 4096];
    payload[..6].copy_from_slice(b"GIF89a");
    let mut reader = Cursor::new(payload);

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        ImageSizeLimit::DEFAULT,
    );

    // The format is identifiable from byte 0, so the read boundary is not
    // coupled to how early identification succeeds: the window is consumed
    // first, then the decision is made.
    assert_eq!(discovered, completed_silently());
    assert_eq!(reader.position(), 1027);
}

#[test]
fn accepted_source_retains_its_evidence_prefix_and_appends_the_remainder() {
    let source = ArchiveImageSource::named("word/media/image.bin");
    let mut original = vec![0; 4096];
    original[..MINIMAL_PNG.len()].copy_from_slice(MINIMAL_PNG);
    for (index, byte) in original[MINIMAL_PNG.len()..].iter_mut().enumerate() {
        // A varying tail, so a payload rebuilt from the wrong offset cannot
        // compare equal to the original by accident.
        *byte = (index % 251) as u8;
    }
    let mut reader = Cursor::new(original.clone());

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        ImageSizeLimit::DEFAULT,
    );

    assert_eq!(
        discovered,
        accepted("word/media/image.bin", original.clone(), ImageFormat::Png)
    );
    assert_eq!(reader.position(), original.len() as u64);
}

#[test]
fn bom_prefixed_svg_ending_inside_the_window_is_accepted() {
    let source = ArchiveImageSource::named("word/media/vector.bin");
    let mut svg = b"\xEF\xBB\xBF".to_vec();
    svg.extend(std::iter::repeat_n(b' ', 1019));
    svg.extend_from_slice(b"<svg>");
    // The bounded read and the SVG window meet exactly here: three BOM bytes
    // plus the 1,024-byte window is the whole evidence prefix.
    assert_eq!(svg.len(), 1027);
    let mut reader = Cursor::new(svg.clone());

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Svg]),
        ImageSizeLimit::DEFAULT,
    );

    assert_eq!(
        discovered,
        accepted("word/media/vector.bin", svg, ImageFormat::Svg)
    );
    // The payload ends exactly at the read boundary, so acceptance must not have
    // gone looking for a tail that is not there.
    assert_eq!(reader.position(), 1027);
}

#[test]
fn bom_prefixed_svg_beyond_the_window_completes_at_the_read_boundary() {
    let source = ArchiveImageSource::named("word/media/vector.bin");
    let mut svg = b"\xEF\xBB\xBF".to_vec();
    svg.extend(std::iter::repeat_n(b' ', 1024));
    svg.extend_from_slice(b"<svg>");
    let mut reader = Cursor::new(svg);

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Svg]),
        ImageSizeLimit::DEFAULT,
    );

    assert_eq!(discovered, completed_silently());
    assert_eq!(reader.position(), 1027);
}

/// Pins the required cover's divergence from the silent normal-image path above.
#[test]
fn required_cover_with_unidentified_evidence_continues_as_jpeg() {
    let source = ArchiveImageSource::required_cover("OPS/cover.png", "application/octet-stream");
    let original = vec![0x5a; 4096];
    let mut reader = Cursor::new(original.clone());

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Jpg]),
        ImageSizeLimit::DEFAULT,
    );

    // The `.png` in the name is diagnostic identity, not evidence: a required
    // cover carries no path evidence, so unknown MIME leaves it unidentified.
    assert_eq!(
        discovered,
        accepted_with(
            "OPS/cover.png",
            original.clone(),
            ImageFormat::Jpg,
            vec![ImageWriteWarning::CoverDefaultToJpeg {
                mime: "application/octet-stream".to_string(),
            }]
        )
    );
    assert_eq!(reader.position(), original.len() as u64);
}

#[test]
fn required_cover_whose_format_is_filtered_completes_with_a_warning() {
    let source = ArchiveImageSource::required_cover("OPS/cover.jpg", "image/jpeg");
    let mut payload = vec![0xff; 4096];
    payload[..3].copy_from_slice(b"\xFF\xD8\xFF");
    let mut reader = Cursor::new(payload);

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        ImageSizeLimit::DEFAULT,
    );

    // Where a filtered normal image completes silently, a filtered cover says
    // why it produced nothing — the same outcome, a different warning list.
    assert_eq!(
        discovered,
        DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings: vec![ImageWriteWarning::UnsupportedCoverFormat {
                format: ImageFormat::Jpg,
            }],
        }
    );
    assert_eq!(reader.position(), 1027);
}

#[test]
fn required_cover_failing_mid_payload_returns_ordered_acquisition_facts() {
    let source = ArchiveImageSource::required_cover("OPS/cover.bin", "application/octet-stream");
    // Fails after the evidence window but before the payload ends, so the
    // defaulting decision is already recorded when acquisition gives out.
    let mut reader = FailAfterReader::new(vec![0; 2048], 1100);

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Jpg]),
        ImageSizeLimit::DEFAULT,
    );

    // Warning order is a property of this outcome, not of the fold that the
    // pipeline later performs over it, so it is asserted as a sequence here.
    assert_eq!(
        discovered,
        DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            warnings: vec![
                ImageWriteWarning::CoverDefaultToJpeg {
                    mime: "application/octet-stream".to_string(),
                },
                ImageWriteWarning::ArchiveImageAcquisitionFailed {
                    source_name: "OPS/cover.bin".to_string(),
                    detail: "injected archive resource failure".to_string(),
                },
            ],
        }
    );
}

/// Parses a test's Image size limit from its command-line spelling.
fn limit_of(text: &str) -> ImageSizeLimit {
    text.parse().expect("test size limit should parse")
}

/// Returns a PNG-identified payload of exactly `len` bytes with a varying tail.
fn png_payload(len: usize) -> Vec<u8> {
    let mut payload = vec![0; len];
    payload[..MINIMAL_PNG.len()].copy_from_slice(MINIMAL_PNG);
    for (index, byte) in payload[MINIMAL_PNG.len()..].iter_mut().enumerate() {
        *byte = (index % 251) as u8;
    }
    payload
}

/// Identifies `reader` as a normal PNG source and returns its pending payload.
fn pending_png<'reader>(
    reader: &'reader mut dyn Read,
    limit: ImageSizeLimit,
) -> AcceptedPayload<'reader> {
    let source = ArchiveImageSource::named("word/media/image.png");
    let discovered = identify_image(&source, reader, &HashSet::from([ImageFormat::Png]), limit);
    match discovered.outcome {
        ArchiveImageDiscoveryOutcome::Accepted(pending) => pending.payload,
        _ => panic!("a PNG source should be accepted"),
    }
}

/// A writer that fails every write, standing in for a full disk.
struct FailingWriter;

impl Write for FailingWriter {
    /// Rejects every write with an injected failure.
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected output failure"))
    }

    /// Has nothing to flush.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn payload_of_exactly_the_limit_is_accepted() {
    let source = ArchiveImageSource::named("word/media/image.png");
    let payload = png_payload(2048);
    let mut reader = Cursor::new(payload.clone());

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        limit_of("2K"),
    );

    assert_eq!(
        discovered,
        accepted("word/media/image.png", payload, ImageFormat::Png)
    );
}

#[test]
fn payload_past_the_limit_warns_and_reads_one_byte_beyond_it() {
    let source = ArchiveImageSource::named("word/media/image.png");
    let mut reader = Cursor::new(png_payload(8192));

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        limit_of("2K"),
    );

    assert_eq!(
        discovered,
        DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::ExceededSizeLimit,
            warnings: vec![ImageWriteWarning::ImageSizeLimitExceeded {
                source_name: "word/media/image.png".to_string(),
                limit: limit_of("2K"),
            }],
        }
    );
    // One byte past 2,048 is enough to know the limit was exceeded, so the rest of
    // the payload is never read into memory.
    assert_eq!(reader.position(), 2049);
}

/// A limit smaller than the evidence window still lets identification run, and
/// then rejects the payload before reading any further.
#[test]
fn limit_inside_the_evidence_window_is_exceeded_after_identification() {
    let source = ArchiveImageSource::named("word/media/image.png");
    let mut reader = Cursor::new(png_payload(4096));

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Png]),
        limit_of("100"),
    );

    assert_eq!(
        discovered.outcome,
        ArchiveImageDiscoveryOutcome::ExceededSizeLimit
    );
    assert_eq!(reader.position(), 1027);
}

/// Pins that a cover past the limit ends like an unreadable one, warning after
/// whatever identification already said.
#[test]
fn required_cover_past_the_limit_warns_after_its_identification_facts() {
    let source = ArchiveImageSource::required_cover("OPS/cover.bin", "application/octet-stream");
    let mut reader = Cursor::new(vec![0x5a; 4096]);

    let discovered = discover_image(
        &source,
        &mut reader,
        &HashSet::from([ImageFormat::Jpg]),
        limit_of("2K"),
    );

    assert_eq!(
        discovered,
        DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::ExceededSizeLimit,
            warnings: vec![
                ImageWriteWarning::CoverDefaultToJpeg {
                    mime: "application/octet-stream".to_string(),
                },
                ImageWriteWarning::ImageSizeLimitExceeded {
                    source_name: "OPS/cover.bin".to_string(),
                    limit: limit_of("2K"),
                },
            ],
        }
    );
}

#[test]
fn identification_leaves_an_accepted_payload_unread_past_its_prefix() {
    let payload = png_payload(200_000);
    let mut reader = Cursor::new(payload.clone());

    let pending = pending_png(&mut reader, ImageSizeLimit::DEFAULT);

    assert_eq!(pending.prefix, payload[..1027]);
    drop(pending);
    assert_eq!(reader.position(), 1027);
}

/// The chunk loop has to rebuild the payload from the prefix and every chunk in
/// order, so the payload is more than three chunks long.
#[test]
fn streamed_payload_is_copied_whole_across_chunks() {
    let payload = png_payload(200_000);
    let mut reader = Cursor::new(payload.clone());
    let mut output = Vec::new();

    pending_png(&mut reader, ImageSizeLimit::DEFAULT)
        .copy_into(&mut output)
        .expect("a readable payload within the limit should copy");

    assert_eq!(output, payload);
}

#[test]
fn streamed_payload_of_exactly_the_limit_is_copied() {
    let payload = png_payload(100_000);
    let mut reader = Cursor::new(payload.clone());
    let mut output = Vec::new();

    pending_png(&mut reader, limit_of("100000"))
        .copy_into(&mut output)
        .expect("a payload of exactly the limit should copy");

    assert_eq!(output, payload);
}

#[test]
fn streamed_payload_past_the_limit_stops_without_writing_the_excess_chunk() {
    let mut reader = Cursor::new(png_payload(200_000));
    let mut output = Vec::new();

    let error = pending_png(&mut reader, limit_of("100000"))
        .copy_into(&mut output)
        .expect_err("a payload past the limit should stop");

    assert!(matches!(
        error,
        PayloadCopyError::Source(PayloadShortfall::ExceedsLimit(limit))
            if limit == limit_of("100000")
    ));
    assert!(
        output.len() <= 100_000,
        "nothing past the limit should be written, wrote {}",
        output.len()
    );
}

#[test]
fn streamed_read_failure_is_a_source_shortfall() {
    let mut reader = FailAfterReader::new(png_payload(200_000), 70_000);
    let mut output = Vec::new();

    let error = pending_png(&mut reader, ImageSizeLimit::DEFAULT)
        .copy_into(&mut output)
        .expect_err("a reader that fails should stop the copy");

    assert!(matches!(
        &error,
        PayloadCopyError::Source(PayloadShortfall::Unreadable(detail))
            if detail.to_string() == "injected archive resource failure"
    ));
}

#[test]
fn streamed_output_failure_is_an_output_error_not_a_shortfall() {
    let mut reader = Cursor::new(png_payload(4096));

    let error = pending_png(&mut reader, ImageSizeLimit::DEFAULT)
        .copy_into(&mut FailingWriter)
        .expect_err("an output that fails should stop the copy");

    assert!(matches!(
        &error,
        PayloadCopyError::Output(detail) if detail.to_string() == "injected output failure"
    ));
}

#[test]
fn shortfalls_become_the_warning_naming_their_source() {
    assert_eq!(
        PayloadShortfall::ExceedsLimit(limit_of("2K")).into_warning("word/media/big.png"),
        ImageWriteWarning::ImageSizeLimitExceeded {
            source_name: "word/media/big.png".to_string(),
            limit: limit_of("2K"),
        }
    );
    assert_eq!(
        PayloadShortfall::Unreadable(io::Error::other("broken")).into_warning("word/media/x.png"),
        ImageWriteWarning::ArchiveImageAcquisitionFailed {
            source_name: "word/media/x.png".to_string(),
            detail: "broken".to_string(),
        }
    );
}

/// A reader that reports every other read as interrupted, as a signal can.
struct InterruptingReader {
    cursor: Cursor<Vec<u8>>,
    interrupt_next: bool,
}

impl Read for InterruptingReader {
    /// Alternates an `Interrupted` error with a real read of at most 1,000 bytes.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.interrupt_next = !self.interrupt_next;
        if !self.interrupt_next {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let len = buffer.len().min(1_000);
        self.cursor.read(&mut buffer[..len])
    }
}

/// The buffer grows in capped doublings and probes before each one, so a payload
/// that ends exactly where a doubling would start is the edge worth pinning, beside
/// one byte either side of it and a limit equal to the payload.
#[test]
fn buffered_payloads_are_whole_at_every_growth_boundary() {
    for len in [1_027, 1_028, 131_071, 131_072, 131_073, 262_144, 300_000] {
        let payload = png_payload(len);
        for limit in [ImageSizeLimit::DEFAULT, limit_of(&len.to_string())] {
            let mut reader = Cursor::new(payload.clone());
            let buffered = pending_png(&mut reader, limit)
                .buffer()
                .unwrap_or_else(|_| panic!("a {len}-byte payload should buffer"));
            assert!(buffered == payload, "a {len}-byte payload under {limit}");
        }
    }
}

/// Both ways of consuming a payload retry an interrupted read instead of treating
/// it as a failed source.
#[test]
fn interrupted_reads_are_retried_whether_buffered_or_streamed() {
    let payload = png_payload(200_000);

    let mut reader = InterruptingReader {
        cursor: Cursor::new(payload.clone()),
        interrupt_next: false,
    };
    let buffered = pending_png(&mut reader, ImageSizeLimit::DEFAULT)
        .buffer()
        .expect("an interrupted read should be retried while buffering");
    assert!(buffered == payload);

    let mut reader = InterruptingReader {
        cursor: Cursor::new(payload.clone()),
        interrupt_next: false,
    };
    let mut streamed = Vec::new();
    pending_png(&mut reader, ImageSizeLimit::DEFAULT)
        .copy_into(&mut streamed)
        .expect("an interrupted read should be retried while streaming");
    assert!(streamed == payload);
}
