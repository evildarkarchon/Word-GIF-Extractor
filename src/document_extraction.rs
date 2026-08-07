//! Per-document extraction policy, dispatch, and outcomes.

mod docx;
mod epub;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::document_selection::SelectedDocument;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWriteResult, ImageWriteWarning};

/// Valid per-run choices for normal images versus EPUB cover extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentExtractionPolicy {
    /// Extract normal document images.
    NormalImages,
    /// Extract one required EPUB cover, optionally falling back to normal images.
    EpubCover {
        /// Whether an EPUB without a usable cover should emit normal images.
        fallback_to_normal_images: bool,
    },
}

impl DocumentExtractionPolicy {
    /// Returns whether EPUB documents should use required-cover extraction.
    fn is_epub_cover_only(self) -> bool {
        matches!(self, Self::EpubCover { .. })
    }
}

/// Immutable Document extraction module configured for one Extraction run.
pub(crate) struct DocumentExtraction {
    policy: DocumentExtractionPolicy,
    image_write_pipeline: ImageWritePipeline,
}

impl DocumentExtraction {
    /// Binds Document extraction and Image write policy for every selected document.
    pub(crate) fn new(
        policy: DocumentExtractionPolicy,
        image_write_pipeline: ImageWritePipeline,
    ) -> Self {
        Self {
            policy,
            image_write_pipeline,
        }
    }

    /// Returns whether this module is configured to extract EPUB covers.
    pub(crate) fn is_epub_cover_extraction_configured(&self) -> bool {
        self.policy.is_epub_cover_only()
    }

    /// Reports which optional outcome fact groups the bound workflow permits.
    ///
    /// The Image write policy is the single owner of both facts. Asking it here,
    /// at the seam that already translates Image write accounting into run-facing
    /// values, is what lets the Extraction run seed aggregation without keeping a
    /// copy that construction would have to keep correct.
    pub(crate) fn applicable_outcome_facts(&self) -> ApplicableOutcomeFacts {
        ApplicableOutcomeFacts {
            conversion: self.image_write_pipeline.is_conversion_configured(),
            gif_destination: self
                .image_write_pipeline
                .gif_destination()
                .map(Path::to_path_buf),
        }
    }

    /// Consumes one selected document and dispatches its authoritative variant.
    ///
    /// Document-local errors are returned as failed outcomes so the Extraction
    /// run can continue processing later documents.
    pub(crate) fn extract(&self, document: SelectedDocument) -> DocumentExtractionOutcome {
        let result = match document {
            SelectedDocument::Docx(document) => {
                let (path, output_dir, base_name) = document.into_extraction_parts();
                docx::process_file(&path, &output_dir, &base_name, &self.image_write_pipeline)
            }
            SelectedDocument::Epub(document) => {
                epub::extract(document, self.policy, &self.image_write_pipeline)
            }
        };

        match result {
            Ok(result) => DocumentExtractionOutcome::Completed(
                DocumentExtractionFacts::from_image_write_result(result),
            ),
            Err(failure) => DocumentExtractionOutcome::Failed {
                facts: DocumentExtractionFacts::from_image_write_result(failure.partial),
                error: DocumentExtractionError::from_source(failure.error),
            },
        }
    }
}

/// Which optional fact groups an eventual Extraction run outcome may carry.
///
/// The value says only that much, plus where routed GIFs go when routing applies:
/// it carries no counts, no terminal wording, and no outcome classification. It
/// owns its destination rather than borrowing it, so it carries no lifetime.
#[derive(Debug)]
pub(crate) struct ApplicableOutcomeFacts {
    conversion: bool,
    gif_destination: Option<PathBuf>,
}

impl ApplicableOutcomeFacts {
    /// Returns whether the eventual outcome may carry conversion facts.
    pub(crate) fn is_conversion_applicable(&self) -> bool {
        self.conversion
    }

    /// Consumes the value into the destination that receives routed GIFs, if any.
    pub(crate) fn into_gif_destination(self) -> Option<PathBuf> {
        self.gif_destination
    }
}

/// The emitted-image counts retained by one Document extraction outcome.
///
/// The four counts are one value rather than four accessors because every
/// caller wants the whole counter shape: naming it once here is what stops a
/// caller from re-spelling it field by field, and what makes adding a fifth
/// counter a change to this type alone.
///
/// The converted, conversion-skipped and GIF-routed counts never together
/// exceed the emitted count, because the Image write pipeline places each
/// emitted image in exactly one of those roles.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EmittedImageTotals {
    emitted_images: usize,
    routed_gifs: usize,
    converted_images: usize,
    skipped_conversions: usize,
}

impl EmittedImageTotals {
    /// Returns the number of images successfully emitted before the outcome ended.
    pub(crate) fn get_emitted_images(self) -> usize {
        self.emitted_images
    }

    /// Returns the number of emitted GIFs routed to the configured destination.
    pub(crate) fn get_routed_gifs(self) -> usize {
        self.routed_gifs
    }

    /// Returns the number of images successfully converted before emission.
    pub(crate) fn get_converted_images(self) -> usize {
        self.converted_images
    }

    /// Returns the number of conversion attempts skipped while preserving source bytes.
    pub(crate) fn get_skipped_conversions(self) -> usize {
        self.skipped_conversions
    }
}

/// Opaque facts retained by one completed or failed Document extraction.
///
/// The value translates Image write pipeline accounting and warnings at the
/// Document extraction seam so callers do not depend on inner pipeline types.
#[derive(Debug)]
pub(crate) struct DocumentExtractionFacts {
    emitted_image_totals: EmittedImageTotals,
    has_normal_image_output: bool,
    warnings: Vec<DocumentExtractionWarning>,
}

impl DocumentExtractionFacts {
    /// Translates inner Image write facts at the Document extraction seam.
    fn from_image_write_result(result: ImageWriteResult) -> Self {
        let has_normal_image_output = result.has_normal_image_output();
        Self {
            emitted_image_totals: EmittedImageTotals {
                emitted_images: result.counts.extracted,
                routed_gifs: result.counts.gifs_routed,
                converted_images: result.counts.converted,
                skipped_conversions: result.counts.skipped,
            },
            has_normal_image_output,
            warnings: result
                .warnings
                .into_iter()
                .map(DocumentExtractionWarning::from_image_write_warning)
                .collect(),
        }
    }

    /// Returns the emitted, GIF-routed, converted and conversion-skipped counts together.
    pub(crate) fn get_emitted_image_totals(&self) -> EmittedImageTotals {
        self.emitted_image_totals
    }

    /// Returns whether any emitted file came from normal-image extraction.
    pub(crate) fn is_normal_image_output_present(&self) -> bool {
        self.has_normal_image_output
    }

    /// Returns ordered non-fatal warnings produced before the outcome ended.
    pub(crate) fn get_warnings(&self) -> &[DocumentExtractionWarning] {
        &self.warnings
    }
}

/// Opaque non-fatal warning exposed by Document extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentExtractionWarning {
    message: String,
}

impl DocumentExtractionWarning {
    /// Exhaustively translates one inner classification into stable Document extraction wording.
    fn from_image_write_warning(warning: ImageWriteWarning) -> Self {
        let message = match warning {
            ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name,
                detail,
            } => format!("Could not read archive resource '{source_name}': {detail}"),
            ImageWriteWarning::ExtensionFallback {
                source_name,
                format,
            } => format!(
                "Magic detection failed for {source_name}; falling back to .{} extension",
                format.extension()
            ),
            ImageWriteWarning::CoverDefaultToJpeg { mime } => format!(
                "Cover image MIME '{mime}' could not be identified; defaulting to .jpg extension."
            ),
            ImageWriteWarning::UnsupportedCoverFormat { format } => format!(
                "Cover image format '{}' not in allowed formats, skipping.",
                format.extension()
            ),
            ImageWriteWarning::ConversionSkipped { base_name, format } => format!(
                "Skipping conversion for {base_name} ({} format not supported for conversion)",
                format.extension()
            ),
            ImageWriteWarning::CoverConversionSkipped { format } => format!(
                "Cover image format '{}' not supported for conversion, skipping cover.",
                format.extension()
            ),
            ImageWriteWarning::ConversionFailed { base_name, detail } => {
                format!("Conversion failed for image in {base_name}: {detail}")
            }
            ImageWriteWarning::CoverConversionFailed { detail } => {
                format!("Cover conversion failed: {detail}")
            }
        };
        Self { message }
    }

    /// Returns the stable user-visible wording for this warning fact.
    pub fn get_message(&self) -> &str {
        &self.message
    }
}

/// Opaque document-local failure exposed by Document extraction.
#[derive(Debug)]
pub(crate) struct DocumentExtractionError {
    source: anyhow::Error,
}

impl DocumentExtractionError {
    /// Preserves the contextual source chain while sealing its concrete type.
    fn from_source(source: anyhow::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for DocumentExtractionError {
    /// Formats the preserved document-local error context.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for DocumentExtractionError {
    /// Returns the preserved underlying error chain.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Terminal result of extracting one selected document.
pub(crate) enum DocumentExtractionOutcome {
    /// Extraction completed with its retained document-level facts.
    Completed(DocumentExtractionFacts),
    /// Extraction failed after retaining document-level facts already produced.
    Failed {
        /// Document extraction facts produced before the failure.
        facts: DocumentExtractionFacts,
        /// Opaque contextual document-local error.
        error: DocumentExtractionError,
    },
}

#[cfg(test)]
mod tests;
