//! Purpose-specific semantic decisions for the Image write pipeline.

use anyhow::Error;

use crate::conversion::ConversionOutcome;
use crate::image_format::ImageFormat;

use super::{AcceptedImage, EmittedImageRole, ImageWritePolicy, ImageWriteWarning, PreparedImage};

/// Statically selected purpose for plural normal-image traversal.
#[derive(Debug, Clone, Copy)]
pub(super) struct NormalImages;

impl NormalImages {
    /// Prepares an accepted normal image, preserving its bytes when conversion cannot finish.
    ///
    /// Appends any conversion warning to the caller's conversion-warning buffer,
    /// keeping discovery warnings separate. The returned image owns its bytes
    /// and borrows only a routed GIF destination from `policy`; emission may still fail.
    pub(super) fn prepare<'policy>(
        &self,
        image: AcceptedImage,
        base_name: &str,
        policy: &'policy ImageWritePolicy,
        warnings: &mut Vec<ImageWriteWarning>,
    ) -> PreparedImage<'policy> {
        match prepare_shared(image, policy) {
            Ok(prepared) => prepared,
            Err(unprepared) => {
                let (format, warning) = match unprepared.problem {
                    ConversionProblem::Unsupported(format) => (
                        format,
                        ImageWriteWarning::ConversionSkipped {
                            base_name: base_name.to_string(),
                            format,
                        },
                    ),
                    ConversionProblem::Failed(error) => (
                        unprepared.original.format,
                        ImageWriteWarning::ConversionFailed {
                            base_name: base_name.to_string(),
                            detail: error.to_string(),
                        },
                    ),
                };
                warnings.push(warning);
                PreparedImage {
                    data: unprepared.original.data,
                    format,
                    role: EmittedImageRole::ConversionSkipped,
                }
            }
        }
    }
}

/// Statically selected purpose for one required EPUB cover attempt.
#[derive(Debug, Clone, Copy)]
pub(super) struct RequiredCover;

impl RequiredCover {
    /// Prepares an accepted cover or completes without an image when conversion cannot finish.
    ///
    /// Appends any conversion warning in attempt order. `None` is terminal cover
    /// completion, never an acquisition retry. Returned bytes are owned and only
    /// a routed GIF destination borrows `policy`; emission may still fail.
    pub(super) fn prepare<'policy>(
        &self,
        image: AcceptedImage,
        policy: &'policy ImageWritePolicy,
        warnings: &mut Vec<ImageWriteWarning>,
    ) -> Option<PreparedImage<'policy>> {
        match prepare_shared(image, policy) {
            Ok(prepared) => Some(prepared),
            Err(unprepared) => {
                warnings.push(match unprepared.problem {
                    ConversionProblem::Unsupported(format) => {
                        ImageWriteWarning::CoverConversionSkipped { format }
                    }
                    ConversionProblem::Failed(error) => ImageWriteWarning::CoverConversionFailed {
                        detail: error.to_string(),
                    },
                });
                None
            }
        }
    }
}

/// Why conversion could not prepare an image, before its purpose decides what to emit.
enum ConversionProblem {
    Unsupported(ImageFormat),
    Failed(Error),
}

/// Original bytes retained for a purpose-specific conversion decision without cloning them.
struct UnpreparedImage {
    original: AcceptedImage,
    problem: ConversionProblem,
}

/// Applies shared routing and conversion while leaving fallback and warnings to the purpose.
///
/// Returns owned prepared bytes borrowing only a GIF destination from `policy`,
/// or the original bytes with the conversion problem. The error is a preparation
/// fact for the purpose to resolve, not a document failure or acquisition retry.
fn prepare_shared<'policy>(
    image: AcceptedImage,
    policy: &'policy ImageWritePolicy,
) -> Result<PreparedImage<'policy>, UnpreparedImage> {
    // Routing is decided together with the destination it needs, so emission
    // never has to ask the policy a second question it could answer differently.
    let routed_destination = if image.format == ImageFormat::Gif {
        policy.gif_destination()
    } else {
        None
    };
    if let Some(destination) = routed_destination {
        return Ok(PreparedImage {
            data: image.data,
            format: image.format,
            role: EmittedImageRole::RoutedGif(destination),
        });
    }

    let Some(conversion) = &policy.conversion else {
        return Ok(PreparedImage {
            data: image.data,
            format: image.format,
            role: EmittedImageRole::Preserved,
        });
    };

    match conversion.convert(&image.data, image.format) {
        Ok(ConversionOutcome::Converted(data, format)) => Ok(PreparedImage {
            data,
            format,
            role: EmittedImageRole::Converted,
        }),
        Ok(ConversionOutcome::PreservedMatchingSource) => Ok(PreparedImage {
            data: image.data,
            format: image.format,
            role: EmittedImageRole::Preserved,
        }),
        Ok(ConversionOutcome::UnsupportedSource(format)) => Err(UnpreparedImage {
            original: image,
            problem: ConversionProblem::Unsupported(format),
        }),
        Err(error) => Err(UnpreparedImage {
            original: image,
            problem: ConversionProblem::Failed(error),
        }),
    }
}
