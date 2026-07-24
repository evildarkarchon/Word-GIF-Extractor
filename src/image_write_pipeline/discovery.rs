//! Incremental Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use crate::image_format::ImageFormat;

use super::purpose::{
    FilteredFormatAction, ImageWritePurpose, SourceEligibility, UnidentifiedFormatAction,
};
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

/// Source facts supplied before discovery reads one archive resource.
///
/// The optional path evidence encodes eligibility directly: normal sources
/// include it, while required covers deliberately omit it.
#[derive(Debug, Clone)]
pub(crate) struct ArchiveImageSource {
    diagnostic_name: String,
    path_evidence_name: Option<String>,
    declared_mime: Option<String>,
}

impl ArchiveImageSource {
    /// Creates a normal archive source whose safe name may identify its format.
    pub(crate) fn named(source_name: impl Into<String>) -> Self {
        let source_name = source_name.into();
        Self {
            diagnostic_name: source_name.clone(),
            path_evidence_name: Some(source_name),
            declared_mime: None,
        }
    }

    /// Adds document-declared MIME evidence after magic and eligible path evidence.
    #[must_use]
    pub(crate) fn with_mime(mut self, mime: impl Into<String>) -> Self {
        self.declared_mime = Some(mime.into());
        self
    }

    /// Creates a required cover whose path is diagnostic identity, not evidence.
    ///
    /// Required covers use bounded byte evidence before declared MIME and never
    /// fall back to the manifest path extension.
    pub(crate) fn required_cover(source_name: impl Into<String>, mime: impl Into<String>) -> Self {
        Self {
            diagnostic_name: source_name.into(),
            path_evidence_name: None,
            declared_mime: Some(mime.into()),
        }
    }

    pub(super) fn diagnostic_name(&self) -> &str {
        &self.diagnostic_name
    }

    pub(super) fn path_evidence_name(&self) -> Option<&str> {
        self.path_evidence_name.as_deref()
    }

    pub(super) fn declared_mime(&self) -> Option<&str> {
        self.declared_mime.as_deref()
    }
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
pub(super) struct DiscoveredImage {
    pub(super) outcome: ArchiveImageDiscoveryOutcome,
    pub(super) warnings: Vec<ImageWriteWarning>,
}

/// Purpose-independent completion state from one archive source discovery attempt.
pub(super) enum ArchiveImageDiscoveryOutcome {
    /// The complete payload was acquired and accepted by Image write policy.
    Accepted(AcceptedImage),
    /// Evidence produced a final non-emitting decision.
    Completed,
    /// Reading the source failed, permitting required-cover candidate retry.
    AcquisitionFailed,
}

/// Acquires and identifies one source through a statically selected Image write purpose.
///
/// Identification checks magic bytes, an eligible source-path extension, then
/// declared MIME. Unidentified and requested-format decisions remain delegated
/// to the purpose. Rejected or filtered sources consume at most 1,027 evidence
/// bytes; accepted sources retain that prefix and append the remaining payload.
/// Any read failure returns `AcquisitionFailed` with the existing warning facts.
pub(super) fn discover_image<P: ImageWritePurpose>(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
    purpose: &P,
) -> DiscoveredImage {
    if matches!(
        purpose.source_eligibility(source),
        SourceEligibility::Reject
    ) {
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings: Vec::new(),
        };
    }

    let mut warnings = Vec::new();
    let mut data = Vec::new();
    if let Err(error) = reader.take(FORMAT_EVIDENCE_LIMIT).read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            source.diagnostic_name(),
            error,
        ));
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            warnings,
        };
    }

    let identified = identify_source(&data, source);
    let (format, evidence) = match identified {
        Some(identified) => (identified.format, Some(identified.evidence)),
        None => {
            let decision = purpose.unidentified_format(source);
            if let Some(warning) = decision.warning {
                warnings.push(warning);
            }
            match decision.action {
                UnidentifiedFormatAction::ContinueWith(format) => (format, None),
                UnidentifiedFormatAction::Complete => {
                    return DiscoveredImage {
                        outcome: ArchiveImageDiscoveryOutcome::Completed,
                        warnings,
                    };
                }
            }
        }
    };

    match evidence {
        Some(IdentificationEvidence::SourcePathExtension) => {
            if let Some(source_name) = source.path_evidence_name() {
                warnings.push(ImageWriteWarning::ExtensionFallback {
                    source_name: source_name.to_string(),
                    format,
                });
            }
        }
        Some(IdentificationEvidence::MagicBytes | IdentificationEvidence::DeclaredMime) | None => {}
    }

    if !allowed_formats.contains(&format) {
        let decision = purpose.filtered_format(format);
        if let Some(warning) = decision.warning {
            warnings.push(warning);
        }
        let FilteredFormatAction::CompleteWithoutEmission = decision.action;
        return DiscoveredImage {
            outcome: ArchiveImageDiscoveryOutcome::Completed,
            warnings,
        };
    }

    if let Err(error) = reader.read_to_end(&mut data) {
        warnings.push(ImageWriteWarning::archive_image_acquisition_failed(
            source.diagnostic_name(),
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
fn identify_source(data: &[u8], source: &ArchiveImageSource) -> Option<IdentifiedImage> {
    if let Some(format) = format_from_magic(data) {
        return Some(IdentifiedImage {
            format,
            evidence: IdentificationEvidence::MagicBytes,
        });
    }

    if let Some(source_name) = source.path_evidence_name()
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

    if let Some(mime) = source.declared_mime()
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
