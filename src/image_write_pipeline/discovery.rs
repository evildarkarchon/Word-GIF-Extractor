//! Archive image discovery inside the Image write pipeline.

use std::collections::HashSet;

use crate::image_format::{FormatConfidence, FormatFallbackPolicy, ImageFormat, ImageFormatSource};

use super::{AcceptedImage, ArchiveImageSource, ImageWriteWarning};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiscoveryPurpose {
    NormalImages,
    RequiredEpubCover,
}

impl DiscoveryPurpose {
    /// Returns the Image format fallback policy for this discovery purpose.
    fn fallback_policy(self) -> FormatFallbackPolicy {
        match self {
            DiscoveryPurpose::NormalImages => FormatFallbackPolicy::SkipUnknown,
            DiscoveryPurpose::RequiredEpubCover => FormatFallbackPolicy::DefaultCoverToJpeg,
        }
    }

    /// Returns whether a filtered format must produce a cover warning.
    fn warns_when_filtered(self) -> bool {
        self == DiscoveryPurpose::RequiredEpubCover
    }
}

/// Accepted images and phase-ordered warnings from Archive image discovery.
pub(super) struct DiscoveredImages {
    pub(super) images: Vec<AcceptedImage>,
    pub(super) warnings: Vec<ImageWriteWarning>,
}

/// Accepts safe, identifiable, requested normal-image sources for writing.
pub(super) fn discover_normal_images(
    sources: Vec<ArchiveImageSource>,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImages {
    let mut discovery = DiscoveryAccumulator::new(allowed_formats);

    for source in sources {
        if !is_safe_archive_path(&source.source_name) {
            continue;
        }

        discovery.accept(
            source.data,
            Some(source.source_name),
            source.mime,
            DiscoveryPurpose::NormalImages,
        );
    }

    discovery.finish()
}

/// Accepts the single required EPUB cover source for writing.
pub(super) fn discover_required_cover(
    data: Vec<u8>,
    mime: String,
    allowed_formats: &HashSet<ImageFormat>,
) -> DiscoveredImages {
    let mut discovery = DiscoveryAccumulator::new(allowed_formats);

    discovery.accept(data, None, Some(mime), DiscoveryPurpose::RequiredEpubCover);

    discovery.finish()
}

struct DiscoveryAccumulator<'a> {
    allowed_formats: &'a HashSet<ImageFormat>,
    images: Vec<AcceptedImage>,
    warnings: Vec<ImageWriteWarning>,
}

impl<'a> DiscoveryAccumulator<'a> {
    /// Starts one discovery phase for the configured requested Image formats.
    fn new(allowed_formats: &'a HashSet<ImageFormat>) -> Self {
        Self {
            allowed_formats,
            images: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Applies Image format evidence, warning, and filter policy to one source.
    ///
    /// `purpose` keeps cover fallback and filtered-cover warning behavior coupled
    /// so callers cannot construct an invalid combination of those policies.
    fn accept(
        &mut self,
        data: Vec<u8>,
        source_name: Option<String>,
        mime: Option<String>,
        purpose: DiscoveryPurpose,
    ) {
        let Some(identified) = ImageFormat::identify_source(ImageFormatSource {
            data: &data,
            source_name: source_name.as_deref(),
            mime: mime.as_deref(),
            fallback_policy: purpose.fallback_policy(),
        }) else {
            return;
        };

        match identified.confidence {
            FormatConfidence::ExtensionFallback => {
                if let Some(source_name) = &source_name {
                    self.warnings.push(ImageWriteWarning::ExtensionFallback {
                        source_name: source_name.clone(),
                        format: identified.format,
                    });
                }
            }
            FormatConfidence::CoverDefault => {
                self.warnings.push(ImageWriteWarning::CoverDefaultToJpeg {
                    mime: mime.clone().unwrap_or_default(),
                });
            }
            FormatConfidence::Magic | FormatConfidence::MimeFallback => {}
        }

        if !self.allowed_formats.contains(&identified.format) {
            if purpose.warns_when_filtered() {
                self.warnings
                    .push(ImageWriteWarning::UnsupportedCoverFormat {
                        format: identified.format,
                    });
            }
            return;
        }

        self.images.push(AcceptedImage {
            data,
            format: identified.format,
        });
    }

    /// Finishes discovery while preserving source order for images and warnings.
    fn finish(self) -> DiscoveredImages {
        DiscoveredImages {
            images: self.images,
            warnings: self.warnings,
        }
    }
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
