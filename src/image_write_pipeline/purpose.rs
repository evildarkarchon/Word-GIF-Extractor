//! Purpose-specific semantic decisions for the Image write pipeline.

use crate::image_format::ImageFormat;

use super::{ArchiveImageSource, ImageWriteWarning};

/// Purpose-selected continuation after unidentified Image format evidence.
pub(super) enum UnidentifiedFormatAction {
    ContinueWith(ImageFormat),
    Complete,
}

/// Purpose-selected continuation after unidentified evidence, with its warning fact.
///
/// The warning is a sibling of the action rather than part of one variant: whether a
/// purpose warns is independent of whether it continues, as `filtered_format` shows
/// by completing with a warning.
pub(super) struct UnidentifiedFormatDecision {
    pub(super) action: UnidentifiedFormatAction,
    pub(super) warning: Option<ImageWriteWarning>,
}

/// Private semantic boundary implemented by each real Image write purpose.
///
/// The trait holds only the decisions Archive image discovery makes partway
/// through its flow, which is why it is a seam with two adapters rather than a
/// closed enum: a required cover's "default unidentified evidence to JPEG" is a
/// continuation inside `discover_image`, which is shared by both visitors.
/// Conversion fallback is deliberately absent. Preparation reports it as a
/// value and each visitor, which already knows its purpose, resolves it; when
/// those two decisions lived here, every caller had to handle the other
/// purpose's outcome and the normal-image path carried an `unreachable!`.
pub(super) trait ImageWritePurpose {
    /// Decides how discovery proceeds when available evidence identifies no format.
    fn unidentified_format(&self, source: &ArchiveImageSource) -> UnidentifiedFormatDecision;

    /// Returns the warning fact, if any, for a source whose identified format is filtered out.
    ///
    /// Discovery always completes without emission here; only the warning varies.
    fn filtered_format(&self, format: ImageFormat) -> Option<ImageWriteWarning>;
}

/// Statically selected purpose for plural normal-image traversal.
#[derive(Debug, Clone, Copy)]
pub(super) struct NormalImages;

impl ImageWritePurpose for NormalImages {
    /// Completes unidentified normal-image discovery without a warning.
    fn unidentified_format(&self, _source: &ArchiveImageSource) -> UnidentifiedFormatDecision {
        UnidentifiedFormatDecision {
            action: UnidentifiedFormatAction::Complete,
            warning: None,
        }
    }

    /// Completes filtered normal-image discovery without a warning.
    fn filtered_format(&self, _format: ImageFormat) -> Option<ImageWriteWarning> {
        None
    }
}

/// Statically selected purpose for one required EPUB cover attempt.
#[derive(Debug, Clone, Copy)]
pub(super) struct RequiredCover;

impl ImageWritePurpose for RequiredCover {
    /// Defaults unidentified required-cover evidence to JPEG with the existing warning.
    fn unidentified_format(&self, source: &ArchiveImageSource) -> UnidentifiedFormatDecision {
        UnidentifiedFormatDecision {
            action: UnidentifiedFormatAction::ContinueWith(ImageFormat::Jpg),
            warning: Some(ImageWriteWarning::CoverDefaultToJpeg {
                mime: source.declared_mime().unwrap_or_default().to_string(),
            }),
        }
    }

    /// Completes a filtered required cover with the existing unsupported warning.
    fn filtered_format(&self, format: ImageFormat) -> Option<ImageWriteWarning> {
        Some(ImageWriteWarning::UnsupportedCoverFormat { format })
    }
}
