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

/// Source facts supplied before discovery reads one archive resource.
///
/// The Image write purpose is decided once, by the constructor that creates the
/// source, and discovery reads it back off the source rather than being told it
/// separately (ADR-0020). No source name ever decides where a file is written;
/// output names come only from the document's Output placement. That is why
/// ADR-0020 deleted the path-traversal guard on source names and why none should
/// return unless a source name starts reaching an output path.
#[derive(Debug, Clone)]
pub(crate) struct ArchiveImageSource {
    diagnostic_name: String,
    purpose: ImageWritePurpose,
}

/// The Image write purpose an archive image source was created with, and the
/// evidence that purpose permits.
///
/// Closed and data-carrying, so each purpose holds exactly the evidence it may
/// use: a required cover has no path evidence at all, which is how it never
/// falls back to its manifest extension. Only `ArchiveImageSource::required_cover`,
/// which is private to the Image write pipeline, builds the second variant.
#[derive(Debug, Clone)]
enum ImageWritePurpose {
    /// One of a document's normal images, whose name is extension evidence.
    NormalImage {
        path_evidence_name: String,
        declared_mime: Option<String>,
    },
    /// The one required EPUB cover an attempt is about, whose manifest path is
    /// diagnostic identity only.
    RequiredCover { declared_mime: String },
}

impl ArchiveImageSource {
    /// Creates a normal archive source whose name may identify its format.
    pub(crate) fn named(source_name: impl Into<String>) -> Self {
        let source_name = source_name.into();
        Self {
            diagnostic_name: source_name.clone(),
            purpose: ImageWritePurpose::NormalImage {
                path_evidence_name: source_name,
                declared_mime: None,
            },
        }
    }

    /// Adds document-declared MIME evidence after magic and path evidence.
    #[must_use]
    pub(crate) fn with_mime(mut self, mime: impl Into<String>) -> Self {
        // Replacing, never changing the purpose: MIME is evidence a source carries
        // under whichever purpose it was created with. Document adapters can only
        // build normal images, so in practice they only reach the first arm.
        match &mut self.purpose {
            ImageWritePurpose::NormalImage { declared_mime, .. } => {
                *declared_mime = Some(mime.into());
            }
            ImageWritePurpose::RequiredCover { declared_mime } => {
                *declared_mime = mime.into();
            }
        }
        self
    }

    /// Creates a required cover whose path is diagnostic identity, not evidence.
    ///
    /// Required covers use bounded byte evidence before declared MIME and never
    /// fall back to the manifest path extension.
    pub(super) fn required_cover(source_name: impl Into<String>, mime: impl Into<String>) -> Self {
        Self {
            diagnostic_name: source_name.into(),
            purpose: ImageWritePurpose::RequiredCover {
                declared_mime: mime.into(),
            },
        }
    }

    pub(super) fn diagnostic_name(&self) -> &str {
        &self.diagnostic_name
    }

    /// Returns the name that may identify a format by extension; normal images only.
    fn path_evidence_name(&self) -> Option<&str> {
        match &self.purpose {
            ImageWritePurpose::NormalImage {
                path_evidence_name, ..
            } => Some(path_evidence_name),
            ImageWritePurpose::RequiredCover { .. } => None,
        }
    }

    /// Returns the document-declared MIME, which a required cover always has.
    fn declared_mime(&self) -> Option<&str> {
        match &self.purpose {
            ImageWritePurpose::NormalImage { declared_mime, .. } => declared_mime.as_deref(),
            ImageWritePurpose::RequiredCover { declared_mime } => Some(declared_mime),
        }
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

/// Acquires and identifies one source under the Image write purpose it carries.
///
/// Identification checks magic bytes, a normal source's path extension, then
/// declared MIME. The two decisions that differ by purpose are made here by
/// matching on the source: unidentified evidence completes a normal image
/// silently but continues a required cover as JPEG with a warning, and a
/// filtered format completes either one, warning only for a required cover.
/// Every source is read; unidentified or filtered sources consume at most 1,027
/// evidence bytes, and accepted sources retain that prefix and append the
/// remaining payload. Any read failure returns `AcquisitionFailed` with the
/// existing warning facts.
pub(super) fn discover_image(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImage {
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
        None => match &source.purpose {
            ImageWritePurpose::NormalImage { .. } => {
                return DiscoveredImage {
                    outcome: ArchiveImageDiscoveryOutcome::Completed,
                    warnings,
                };
            }
            ImageWritePurpose::RequiredCover { declared_mime } => {
                warnings.push(ImageWriteWarning::CoverDefaultToJpeg {
                    mime: declared_mime.clone(),
                });
                (ImageFormat::Jpg, None)
            }
        },
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
        // Kept exhaustive so a new purpose has to state whether filtering warns.
        match &source.purpose {
            // A filtered normal image is an ordinary non-match, so it completes silently.
            ImageWritePurpose::NormalImage { .. } => {}
            ImageWritePurpose::RequiredCover { .. } => {
                warnings.push(ImageWriteWarning::UnsupportedCoverFormat { format });
            }
        }
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
/// Magic bytes outrank a normal source's path extension, which outranks declared
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

#[cfg(test)]
mod tests;
