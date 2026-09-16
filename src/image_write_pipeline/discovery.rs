//! Incremental Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use crate::image_format::ImageFormat;

use super::{AcceptedImage, ImageWriteWarning};

// SVG inspection searches 1,024 bytes after an optional three-byte UTF-8 BOM.
pub(super) const FORMAT_EVIDENCE_LIMIT: u64 = 1027;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1A\n";
const JPG_MAGIC: &[u8] = b"\xFF\xD8\xFF";
const GIF_MAGIC: &[u8] = b"GIF8";
const BMP_MAGIC: &[u8] = b"BM";
const TIFF_LE_MAGIC: &[u8] = b"II\x2A\x00";
const TIFF_BE_MAGIC: &[u8] = b"MM\x00\x2A";
const RIFF_MAGIC: &[u8] = b"RIFF";
const WEBP_MAGIC: &[u8] = b"WEBP";
const ICO_MAGIC: &[u8] = b"\x00\x00\x01\x00";
const WMF_PLACEABLE_MAGIC: &[u8] = b"\xD7\xCD\xC6\x9A";
const WMF_STANDARD_MAGIC: &[u8] = b"\x01\x00\x09\x00";
const EMF_PREFIX_MAGIC: &[u8] = b"\x01\x00\x00\x00";
const EMF_SIGNATURE_OFFSET: usize = 40;
const EMF_SIGNATURE: &[u8] = b" EMF";
const SVG_SEARCH_LIMIT: usize = 1024;

/// Borrowed evidence for a normal image; the name is also checked for path safety.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NormalImageSource<'a> {
    source_name: &'a str,
    declared_mime: Option<&'a str>,
}

impl<'a> NormalImageSource<'a> {
    /// Creates a named source with no declared MIME evidence.
    pub(crate) fn named(source_name: &'a str) -> Self {
        Self {
            source_name,
            declared_mime: None,
        }
    }

    /// Creates a declared source whose MIME follows magic and safe extension evidence.
    pub(crate) fn declared(source_name: &'a str, mime: &'a str) -> Self {
        Self {
            source_name,
            declared_mime: Some(mime),
        }
    }

    pub(super) fn diagnostic_name(self) -> &'a str {
        self.source_name
    }

    /// Returns whether discovery may touch this normal source's reader.
    pub(super) fn is_safe(self) -> bool {
        is_safe_archive_path(self.source_name)
    }
}

/// Borrowed required-cover facts; its name is diagnostic identity only.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequiredCoverSource<'a> {
    diagnostic_name: &'a str,
    declared_mime: &'a str,
}

impl<'a> RequiredCoverSource<'a> {
    /// Requires both diagnostic identity and MIME; path extensions never supply evidence.
    pub(crate) fn new(diagnostic_name: &'a str, declared_mime: &'a str) -> Self {
        Self {
            diagnostic_name,
            declared_mime,
        }
    }

    pub(super) fn diagnostic_name(self) -> &'a str {
        self.diagnostic_name
    }
}

/// The two real purposes, bound to only the evidence each permits.
enum DiscoverySource<'a> {
    Normal(NormalImageSource<'a>),
    RequiredCover(RequiredCoverSource<'a>),
}

/// Facts selected together by the single exhaustive purpose dispatch.
struct DiscoveryRules<'a> {
    diagnostic_name: &'a str,
    path_evidence_name: Option<&'a str>,
    declared_mime: Option<&'a str>,
    eligible: bool,
    default_format: Option<(ImageFormat, &'a str)>,
    warn_filtered: bool,
}

/// Acquires normal images with safe-name, extension and optional MIME evidence.
pub(super) fn discover_normal_image(
    source: NormalImageSource<'_>,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
    discover_image(DiscoverySource::Normal(source), reader, allowed_formats)
}

/// Acquires a required cover using magic and mandatory declared MIME evidence.
pub(super) fn discover_required_cover(
    source: RequiredCoverSource<'_>,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
    discover_image(
        DiscoverySource::RequiredCover(source),
        reader,
        allowed_formats,
    )
}

/// Discovery-private evidence that selected one canonical Image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentificationEvidence {
    MagicBytes,
    SourcePathExtension,
    DeclaredMime,
}

/// Discovery-private result retaining the fact needed for fallback warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IdentifiedImage {
    format: ImageFormat,
    evidence: IdentificationEvidence,
}

/// One source's typed discovery outcome and phase-ordered warning facts.
#[derive(Debug, PartialEq)]
pub(super) struct DiscoveredImage {
    pub(super) outcome: ArchiveImageDiscoveryOutcome,
    pub(super) warnings: Vec<ImageWriteWarning>,
}

/// Purpose-independent completion state from one archive source discovery attempt.
#[derive(Debug, PartialEq)]
pub(super) enum ArchiveImageDiscoveryOutcome {
    /// The complete payload was acquired and accepted by Image write policy.
    Accepted(AcceptedImage),
    /// Evidence produced a final non-emitting decision.
    Completed,
    /// Reading the source failed, permitting required-cover candidate retry.
    AcquisitionFailed,
}

/// Acquires a source after one exhaustive dispatch binds its evidence and decisions.
///
/// Rejected or filtered sources consume at most 1,027 evidence bytes. Accepted
/// sources retain that prefix and append the remaining payload; read failures
/// retain warnings already produced and permit required-cover acquisition retry.
fn discover_image(
    source: DiscoverySource<'_>,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
    let rules = match source {
        DiscoverySource::Normal(source) => DiscoveryRules {
            diagnostic_name: source.source_name,
            path_evidence_name: Some(source.source_name),
            declared_mime: source.declared_mime,
            eligible: source.is_safe(),
            default_format: None,
            warn_filtered: false,
        },
        DiscoverySource::RequiredCover(source) => DiscoveryRules {
            diagnostic_name: source.diagnostic_name,
            path_evidence_name: None,
            declared_mime: Some(source.declared_mime),
            eligible: true,
            default_format: Some((ImageFormat::Jpg, source.declared_mime)),
            warn_filtered: true,
        },
    };
    if !rules.eligible {
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings: Vec::new(),
        };
    }

    let mut warnings = Vec::new();
    let mut data = Vec::new();
    if let Err(error) = reader.take(FORMAT_EVIDENCE_LIMIT).read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            rules.diagnostic_name,
            error,
        ));
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            warnings,
        };
    }

    let identified = identify_source(&data, &rules);
    let (format, evidence) = match identified {
        Some(identified) => (identified.format, Some(identified.evidence)),
        None => match rules.default_format {
            Some((format, mime)) => {
                warnings.push(ImageWriteWarning::CoverDefaultToJpeg {
                    mime: mime.to_string(),
                });
                (format, None)
            }
            None => {
                return DiscoveredImage {
                    outcome: ArchiveImageDiscoveryOutcome::Completed,
                    warnings,
                };
            }
        },
    };

    match evidence {
        Some(IdentificationEvidence::SourcePathExtension) => {
            if let Some(source_name) = rules.path_evidence_name {
                warnings.push(ImageWriteWarning::ExtensionFallback {
                    source_name: source_name.to_string(),
                    format,
                });
            }
        }
        Some(IdentificationEvidence::MagicBytes | IdentificationEvidence::DeclaredMime) | None => {}
    }

    if !allowed_formats.contains(&format) {
        if rules.warn_filtered {
            warnings.push(ImageWriteWarning::UnsupportedCoverFormat { format });
        }
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings,
        };
    }

    if let Err(error) = reader.read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            rules.diagnostic_name,
            error,
        ));
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            warnings,
        };
    }

    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Accepted(AcceptedImage { data, format }),
        warnings,
    }
}

/// Identifies a canonical format from one bounded evidence prefix and source facts.
///
/// Magic bytes outrank an eligible source-path extension, which outranks declared
/// MIME. The function performs no I/O and returns `None` when all supplied evidence
/// is absent or unrecognized; callers enforce the 1,027-byte read boundary.
fn identify_source(data: &[u8], rules: &DiscoveryRules<'_>) -> Option<IdentifiedImage> {
    if let Some(format) = format_from_magic(data) {
        return Some(IdentifiedImage {
            format,
            evidence: IdentificationEvidence::MagicBytes,
        });
    }

    if let Some(source_name) = rules.path_evidence_name
        && let Some(format) = Path::new(source_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(ImageFormat::from_extension)
    {
        return Some(IdentifiedImage {
            format,
            evidence: IdentificationEvidence::SourcePathExtension,
        });
    }

    if let Some(mime) = rules.declared_mime
        && let Some(format) = format_from_mime(mime)
    {
        return Some(IdentifiedImage {
            format,
            evidence: IdentificationEvidence::DeclaredMime,
        });
    }

    None
}

/// Recognizes all supported binary and textual signatures in a bounded prefix.
///
/// Returns `None` for short, incomplete, or unknown evidence. SVG inspection is
/// limited to 1,024 bytes after an optional UTF-8 BOM and performs no reads itself.
fn format_from_magic(data: &[u8]) -> Option<ImageFormat> {
    if data.starts_with(PNG_MAGIC) {
        return Some(ImageFormat::Png);
    }
    if data.starts_with(JPG_MAGIC) {
        return Some(ImageFormat::Jpg);
    }
    if data.starts_with(GIF_MAGIC) {
        return Some(ImageFormat::Gif);
    }
    if data.starts_with(BMP_MAGIC) {
        return Some(ImageFormat::Bmp);
    }
    if data.starts_with(TIFF_LE_MAGIC) || data.starts_with(TIFF_BE_MAGIC) {
        return Some(ImageFormat::Tiff);
    }
    if data.len() >= 12 && data.starts_with(RIFF_MAGIC) && &data[8..12] == WEBP_MAGIC {
        return Some(ImageFormat::Webp);
    }
    if data.starts_with(ICO_MAGIC) {
        return Some(ImageFormat::Ico);
    }
    if data.starts_with(WMF_PLACEABLE_MAGIC) || data.starts_with(WMF_STANDARD_MAGIC) {
        return Some(ImageFormat::Wmf);
    }
    if is_emf(data) {
        return Some(ImageFormat::Emf);
    }
    if is_svg(data) {
        return Some(ImageFormat::Svg);
    }

    None
}

/// Maps a declared MIME value to one canonical Image format.
///
/// Parameters are ignored after the first semicolon and matching is case-insensitive.
/// Unknown or non-image declarations return `None`; this function cannot fail.
fn format_from_mime(mime: &str) -> Option<ImageFormat> {
    let normalized = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();

    match normalized.as_str() {
        "image/jpeg" => Some(ImageFormat::Jpg),
        "image/png" => Some(ImageFormat::Png),
        "image/gif" => Some(ImageFormat::Gif),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/tiff" => Some(ImageFormat::Tiff),
        "image/svg+xml" => Some(ImageFormat::Svg),
        "image/x-emf" | "image/emf" => Some(ImageFormat::Emf),
        "image/x-wmf" | "image/wmf" => Some(ImageFormat::Wmf),
        "image/webp" => Some(ImageFormat::Webp),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some(ImageFormat::Ico),
        _ => None,
    }
}

fn is_emf(data: &[u8]) -> bool {
    data.starts_with(EMF_PREFIX_MAGIC)
        && data
            .get(EMF_SIGNATURE_OFFSET..EMF_SIGNATURE_OFFSET + EMF_SIGNATURE.len())
            .is_some_and(|signature| signature == EMF_SIGNATURE)
}

fn is_svg(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let search_window = &data[..data.len().min(SVG_SEARCH_LIMIT)];

    (0..search_window.len()).any(|start| {
        starts_with_ignore_ascii_case(&search_window[start..], b"<svg")
            && matches!(
                search_window.get(start + 4),
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>')
            )
    })
}

fn starts_with_ignore_ascii_case(data: &[u8], prefix: &[u8]) -> bool {
    data.len() >= prefix.len()
        && data[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
}

/// Returns whether an archive path is safe to use as image source evidence.
fn is_safe_archive_path(name: &str) -> bool {
    if name.contains('\0') || name.contains("..") {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    // Colons enable drive-letter and alternate-data-stream syntax on Windows.
    if name.contains(':') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests;
