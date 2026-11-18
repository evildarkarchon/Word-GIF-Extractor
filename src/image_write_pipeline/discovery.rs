//! Incremental Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;

use crate::image_format::ImageFormat;
use crate::image_size_limit::ImageSizeLimit;

use super::{AcceptedImage, ImageWriteWarning};

// SVG inspection searches 1,024 bytes after an optional three-byte UTF-8 BOM.
pub(super) const FORMAT_EVIDENCE_LIMIT: u64 = 1027;
// One read per chunk while a payload is streamed to its file. Large enough that a
// big image costs few reads, small enough that streaming holds next to nothing.
const STREAM_CHUNK_SIZE: usize = 64 * 1024;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1A\n";
const JPG_MAGIC: &[u8] = b"\xFF\xD8\xFF";
const GIF_MAGIC: &[u8] = b"GIF8";
const BMP_MAGIC: &[u8] = b"BM";
// The DIB header follows the 14-byte BMP file header and begins with its own size.
const BMP_DIB_HEADER_SIZE_OFFSET: usize = 14;
// Core (12), OS/2 2.x short and full (16, 64), and Windows info headers v1–v5
// (40, 52, 56, 108, 124).
const BMP_DIB_HEADER_SIZES: [u32; 8] = [12, 16, 40, 52, 56, 64, 108, 124];
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
///
/// `T` is what an accepted source carries: by default the [`AcceptedImage`] whose
/// whole payload [`discover_image`] buffered, or the [`PendingImage`] that
/// [`identify_image`] hands on with its payload still to read.
#[derive(Debug, PartialEq)]
pub(super) struct DiscoveredImage<T = AcceptedImage> {
    pub(super) outcome: ArchiveImageDiscoveryOutcome<T>,
    pub(super) warnings: Vec<ImageWriteWarning>,
}

/// Purpose-independent completion state from one archive source discovery attempt.
#[derive(Debug, PartialEq)]
pub(super) enum ArchiveImageDiscoveryOutcome<T = AcceptedImage> {
    /// Image write policy accepted the source; `T` carries its payload.
    Accepted(T),
    /// Evidence decided the source is not emitted: an unidentified normal image or
    /// a format outside the requested set. Nothing is written for it, so a required
    /// cover ending here hands over to the next cover candidate (ADR-0023).
    Completed,
    /// Reading the source failed, recorded as an acquisition warning fact. Kept
    /// apart from `Completed` because Archive image discovery reports the two
    /// differently, though either one writes no cover and so hands over.
    AcquisitionFailed,
    /// The payload ran past the Image size limit, recorded as a size-limit warning
    /// fact. Nothing is written for it, so a required cover ending here hands over
    /// exactly as after a read failure (ADR-0030).
    ExceededSizeLimit,
}

/// One accepted source whose payload has been read only as far as its evidence prefix.
///
/// The visitor decides from `format` whether the payload is buffered for conversion
/// or streamed to its file, and keeps `source_name` to word a warning if the payload
/// then runs short.
pub(super) struct PendingImage<'reader> {
    pub(super) source_name: String,
    pub(super) format: ImageFormat,
    pub(super) payload: AcceptedPayload<'reader>,
}

/// The payload of one accepted source: the evidence prefix already read, and the
/// rest still in its reader, held to the Image size limit however it is consumed.
///
/// Archive image discovery owns acquisition, so the limit and the distinction
/// between a source that ran short and an output that failed live here. Image file
/// emission only says where the bytes go (ADR-0030). The reader is borrowed, so a
/// payload cannot outlive the archive entry it reads from.
pub(super) struct AcceptedPayload<'reader> {
    prefix: Vec<u8>,
    rest: &'reader mut dyn Read,
    limit: ImageSizeLimit,
}

impl AcceptedPayload<'_> {
    /// Reads the whole payload into memory.
    ///
    /// At most one byte past the limit is read, which is all it takes to know the
    /// limit was exceeded, so the buffer never holds more than `limit + 1` bytes.
    /// The buffer doubles as it fills, as a `Vec` would, but its growth is capped at
    /// `limit + 1`. Growing copies the buffer, so the old and new allocations are
    /// briefly both live: uncapped doubling near the limit would peak at twice the
    /// limit, and capped it peaks at one and a half times.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadShortfall::Unreadable`] when the reader fails and
    /// [`PayloadShortfall::ExceedsLimit`] when the payload is larger than the limit.
    pub(super) fn buffer(self) -> Result<Vec<u8>, PayloadShortfall> {
        let Self {
            prefix: mut data,
            rest,
            limit,
        } = self;
        let ceiling = limit.bytes().saturating_add(1);
        loop {
            let held = data.len() as u64;
            if held >= ceiling {
                break;
            }
            if data.len() == data.capacity() {
                // Probe one byte before growing, so a payload that ends exactly at
                // the capacity is never copied into a larger buffer it does not need.
                let mut probe = [0; 1];
                if read_retrying(rest, &mut probe).map_err(PayloadShortfall::Unreadable)? == 0 {
                    break;
                }
                let target = (held.max(STREAM_CHUNK_SIZE as u64) * 2).min(ceiling);
                data.reserve_exact(usize::try_from(target - held).unwrap_or(usize::MAX));
                data.push(probe[0]);
                continue;
            }
            // Read no more than the capacity already there, so `read_to_end` never
            // grows the buffer itself; it returns once that capacity is full, or
            // early at the end of the payload.
            let room = ((data.capacity() - data.len()) as u64).min(ceiling - held);
            let read = Read::take(&mut *rest, room)
                .read_to_end(&mut data)
                .map_err(PayloadShortfall::Unreadable)?;
            if (read as u64) < room {
                break;
            }
        }
        if !limit.admits(data.len() as u64) {
            return Err(PayloadShortfall::ExceedsLimit(limit));
        }
        Ok(data)
    }

    /// Copies the whole payload into `output` one chunk at a time.
    ///
    /// Holds no more than one chunk beyond the prefix. A chunk that would take the
    /// payload past the limit is not written. Whatever was written before a failure
    /// stays in `output`, so a caller that claimed a file for it removes the file.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadCopyError::Source`] when the reader fails or the payload
    /// runs past the limit, and [`PayloadCopyError::Output`] when `output` fails.
    pub(super) fn copy_into(self, output: &mut dyn Write) -> Result<(), PayloadCopyError> {
        let Self {
            prefix,
            rest,
            limit,
        } = self;
        let mut copied = prefix.len() as u64;
        if !limit.admits(copied) {
            return Err(PayloadCopyError::Source(PayloadShortfall::ExceedsLimit(
                limit,
            )));
        }
        output
            .write_all(&prefix)
            .map_err(PayloadCopyError::Output)?;

        let mut chunk = vec![0; STREAM_CHUNK_SIZE];
        loop {
            let read = match rest.read(&mut chunk) {
                Ok(0) => return Ok(()),
                Ok(read) => read,
                // `Interrupted` asks for the read to be retried; `read_to_end` does
                // the same, so a buffered payload and a streamed one agree.
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(PayloadCopyError::Source(PayloadShortfall::Unreadable(
                        error,
                    )));
                }
            };
            copied += read as u64;
            if !limit.admits(copied) {
                return Err(PayloadCopyError::Source(PayloadShortfall::ExceedsLimit(
                    limit,
                )));
            }
            output
                .write_all(&chunk[..read])
                .map_err(PayloadCopyError::Output)?;
        }
    }
}

/// Reads once into `buffer`, retrying a read the reader reports as interrupted.
///
/// `Interrupted` asks for the read to be retried, as `read_to_end` does, so a byte
/// probed here is treated exactly like one read in bulk.
fn read_retrying(reader: &mut dyn Read, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

/// Why an accepted payload could not be acquired whole.
///
/// Either way nothing is written for the source and it warns: the document keeps
/// its other images.
#[derive(Debug)]
pub(super) enum PayloadShortfall {
    /// The archive reader failed partway through the payload.
    Unreadable(io::Error),
    /// The payload is larger than the Image size limit.
    ExceedsLimit(ImageSizeLimit),
}

impl PayloadShortfall {
    /// Words this shortfall as the warning fact for the source named `source_name`.
    pub(super) fn into_warning(self, source_name: &str) -> ImageWriteWarning {
        match self {
            Self::Unreadable(error) => {
                ImageWriteWarning::archive_image_acquisition_failed(source_name, error)
            }
            Self::ExceedsLimit(limit) => ImageWriteWarning::ImageSizeLimitExceeded {
                source_name: source_name.to_string(),
                limit,
            },
        }
    }

    /// Returns the discovery outcome a buffered source ends in after this shortfall.
    fn outcome<T>(&self) -> ArchiveImageDiscoveryOutcome<T> {
        match self {
            Self::Unreadable(_) => ArchiveImageDiscoveryOutcome::AcquisitionFailed,
            Self::ExceedsLimit(_) => ArchiveImageDiscoveryOutcome::ExceededSizeLimit,
        }
    }
}

impl fmt::Display for PayloadShortfall {
    /// Describes the shortfall for error context, such as a failed partial-file removal.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(error) => write!(formatter, "the source could not be read: {error}"),
            Self::ExceedsLimit(limit) => {
                write!(
                    formatter,
                    "the source is larger than the {limit} image size limit"
                )
            }
        }
    }
}

/// Why copying an accepted payload into its output stopped.
#[derive(Debug)]
pub(super) enum PayloadCopyError {
    /// The source ran short; a warning, not a document failure.
    Source(PayloadShortfall),
    /// Writing the output failed; this fails the document, as any output failure does.
    Output(io::Error),
}

/// Acquires, identifies and buffers one source under the Image write purpose it carries.
///
/// Runs [`identify_image`] and then reads an accepted source's whole payload into
/// memory, held to `limit`. Required covers come through here, as does every
/// image that has to be converted. A read failure returns `AcquisitionFailed`, and
/// a payload past the limit returns `ExceededSizeLimit`, each after the warning
/// facts identification already recorded and with one warning of its own.
pub(super) fn discover_image(
    source: &ArchiveImageSource,
    reader: &mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
    limit: ImageSizeLimit,
) -> DiscoveredImage {
    let DiscoveredImage {
        outcome,
        mut warnings,
    } = identify_image(source, reader, allowed_formats, limit);
    let outcome = match outcome {
        ArchiveImageDiscoveryOutcome::Accepted(pending) => match pending.payload.buffer() {
            Ok(data) => ArchiveImageDiscoveryOutcome::Accepted(AcceptedImage {
                source_name: pending.source_name,
                data,
                format: pending.format,
            }),
            Err(shortfall) => {
                let outcome = shortfall.outcome();
                warnings.push(shortfall.into_warning(&pending.source_name));
                outcome
            }
        },
        ArchiveImageDiscoveryOutcome::Completed => ArchiveImageDiscoveryOutcome::Completed,
        ArchiveImageDiscoveryOutcome::AcquisitionFailed => {
            ArchiveImageDiscoveryOutcome::AcquisitionFailed
        }
        ArchiveImageDiscoveryOutcome::ExceededSizeLimit => {
            ArchiveImageDiscoveryOutcome::ExceededSizeLimit
        }
    };
    DiscoveredImage { outcome, warnings }
}

/// Acquires and identifies one source under the Image write purpose it carries,
/// leaving an accepted source's payload unread past its evidence prefix.
///
/// Identification checks magic bytes, a normal source's path extension, then
/// declared MIME. The two decisions that differ by purpose are made here by
/// matching on the source: unidentified evidence completes a normal image
/// silently but continues a required cover as JPEG with a warning, and a
/// filtered format completes either one, warning only for a required cover.
/// Every source is read; unidentified or filtered sources consume at most 1,027
/// evidence bytes, and an accepted source's [`AcceptedPayload`] keeps that prefix
/// and the reader, held to `limit`, for the caller to buffer or stream. A failure
/// reading the prefix returns `AcquisitionFailed` with the existing warning facts.
pub(super) fn identify_image<'reader>(
    source: &ArchiveImageSource,
    reader: &'reader mut dyn Read,
    allowed_formats: &HashSet<ImageFormat>,
    limit: ImageSizeLimit,
) -> DiscoveredImage<PendingImage<'reader>> {
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

    DiscoveredImage {
        outcome: ArchiveImageDiscoveryOutcome::Accepted(PendingImage {
            source_name: source.diagnostic_name().to_string(),
            format,
            payload: AcceptedPayload {
                prefix: data,
                rest: reader,
                limit,
            },
        }),
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
    if is_bmp(data) {
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

/// Recognizes a BMP file header: the `BM` signature and a known DIB header size.
///
/// Two ASCII letters alone are not evidence: any text resource that begins with
/// "BM" would pass. The DIB header that follows the 14-byte file header opens with
/// its own size, and real bitmaps use only a handful of sizes (core, OS/2 and the
/// Windows `BITMAPINFOHEADER` family). A bitmap with an unusual header still
/// reaches its format through extension or declared-MIME evidence.
fn is_bmp(data: &[u8]) -> bool {
    data.starts_with(BMP_MAGIC)
        && data
            .get(BMP_DIB_HEADER_SIZE_OFFSET..BMP_DIB_HEADER_SIZE_OFFSET + 4)
            .map(|size| u32::from_le_bytes([size[0], size[1], size[2], size[3]]))
            .is_some_and(|size| BMP_DIB_HEADER_SIZES.contains(&size))
}

/// Recognizes an SVG document whose root element is `<svg`.
///
/// Searches the 1,024 bytes after an optional UTF-8 BOM, skipping leading
/// whitespace, the XML declaration and other processing instructions, comments,
/// and a doctype, then requires `<svg` followed by a delimiter. Requiring the root
/// rather than any occurrence matters for EPUBs: an XHTML page that wraps an image
/// in inline SVG (Calibre's title page, for one) has `<svg` near its start, and
/// because magic bytes outrank the declared MIME, it would otherwise be written out
/// as an SVG image. Matching is ASCII case-insensitive, as it was before the root
/// requirement, and a namespace-prefixed root such as `<svg:svg>` is not
/// recognized by bytes; it still reaches SVG through extension or declared MIME.
fn is_svg(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let window = &data[..data.len().min(SVG_SEARCH_LIMIT)];
    let Some(root) = skip_xml_prolog(window) else {
        return false;
    };

    starts_with_ignore_ascii_case(root, b"<svg")
        && matches!(
            root.get(4),
            None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>')
        )
}

/// Returns the bytes from the first element-like markup after an XML prolog.
///
/// Skips whitespace, `<? … ?>` processing instructions (the XML declaration among
/// them), `<!-- … -->` comments and one or more `<!DOCTYPE …>` declarations,
/// including a bracketed internal subset. Returns `None` when a construct is left
/// unterminated inside `data`, because what follows it cannot be known.
fn skip_xml_prolog(mut data: &[u8]) -> Option<&[u8]> {
    loop {
        data = data.trim_ascii_start();
        if data.starts_with(b"<?") {
            data = after(data, 2, b"?>")?;
        } else if data.starts_with(b"<!--") {
            data = after(data, 4, b"-->")?;
        } else if starts_with_ignore_ascii_case(data, b"<!DOCTYPE") {
            data = after_doctype(data)?;
        } else {
            return Some(data);
        }
    }
}

/// Returns the bytes after the first `terminator` found at or beyond `start`.
fn after<'a>(data: &'a [u8], start: usize, terminator: &[u8]) -> Option<&'a [u8]> {
    let body = data.get(start..)?;
    body.windows(terminator.len())
        .position(|candidate| candidate == terminator)
        .map(|index| &body[index + terminator.len()..])
}

/// Returns the bytes after a doctype declaration's closing `>`.
///
/// A `>` inside a quoted public or system identifier, or inside the bracketed
/// internal subset, does not close the declaration.
fn after_doctype(data: &[u8]) -> Option<&[u8]> {
    let mut quote = None;
    let mut subset_depth = 0usize;
    for (index, &byte) in data.iter().enumerate() {
        match (quote, byte) {
            (Some(open), _) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, b'[') => subset_depth += 1,
            (None, b']') => subset_depth = subset_depth.saturating_sub(1),
            (None, b'>') if subset_depth == 0 => return Some(&data[index + 1..]),
            (None, _) => {}
        }
    }
    None
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
