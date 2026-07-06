//! Structured warning facts emitted during document extraction.

use crate::archive_image_discovery::ArchiveImageDiscoveryWarning;
use crate::image_format::ImageFormat;

/// Warning facts produced while the Image write pipeline prepares or writes images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageWriteWarning {
    /// A batch image could not be converted because its source format is unsupported.
    ConversionSkipped {
        /// Output base name used in the existing warning text.
        base_name: String,
        /// Source image format that could not be converted.
        format: ImageFormat,
    },
    /// A required cover image could not be converted because its source format is unsupported.
    CoverConversionSkipped {
        /// Source image format that could not be converted.
        format: ImageFormat,
    },
    /// A batch image conversion failed and the original bytes were written instead.
    ConversionFailed {
        /// Output base name used in the existing warning text.
        base_name: String,
        /// Conversion error message.
        message: String,
    },
    /// A required cover image conversion failed and the cover was skipped.
    CoverConversionFailed {
        /// Conversion error message.
        message: String,
    },
}

impl ImageWriteWarning {
    /// Formats this warning using the existing terminal wording.
    pub fn message(&self) -> String {
        match self {
            ImageWriteWarning::ConversionSkipped { base_name, format } => format!(
                "Skipping conversion for {} ({} format not supported for conversion)",
                base_name,
                format.extension()
            ),
            ImageWriteWarning::CoverConversionSkipped { format } => format!(
                "Cover image format '{}' not supported for conversion, skipping cover.",
                format.extension()
            ),
            ImageWriteWarning::ConversionFailed { base_name, message } => {
                format!("Conversion failed for image in {}: {}", base_name, message)
            }
            ImageWriteWarning::CoverConversionFailed { message } => {
                format!("Cover conversion failed: {}", message)
            }
        }
    }
}

/// Warning facts produced while extracting one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentExtractionWarning {
    /// Warning emitted by Archive image discovery.
    ArchiveImageDiscovery(ArchiveImageDiscoveryWarning),
    /// Warning emitted by the Image write pipeline.
    ImageWrite(ImageWriteWarning),
}

impl DocumentExtractionWarning {
    /// Formats this warning using the existing terminal wording.
    pub fn message(&self) -> String {
        match self {
            DocumentExtractionWarning::ArchiveImageDiscovery(warning) => warning.message(),
            DocumentExtractionWarning::ImageWrite(warning) => warning.message(),
        }
    }
}

impl From<ArchiveImageDiscoveryWarning> for DocumentExtractionWarning {
    fn from(warning: ArchiveImageDiscoveryWarning) -> Self {
        DocumentExtractionWarning::ArchiveImageDiscovery(warning)
    }
}

impl From<ImageWriteWarning> for DocumentExtractionWarning {
    fn from(warning: ImageWriteWarning) -> Self {
        DocumentExtractionWarning::ImageWrite(warning)
    }
}

/// Combines discovery and write warning facts in the order they are produced.
pub fn combine_document_warnings(
    discovery_warnings: Vec<ArchiveImageDiscoveryWarning>,
    write_warnings: Vec<ImageWriteWarning>,
) -> Vec<DocumentExtractionWarning> {
    discovery_warnings
        .into_iter()
        .map(DocumentExtractionWarning::from)
        .chain(
            write_warnings
                .into_iter()
                .map(DocumentExtractionWarning::from),
        )
        .collect()
}
