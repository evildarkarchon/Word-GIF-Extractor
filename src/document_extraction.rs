//! Per-document extraction policy, dispatch, and outcomes.

mod docx;
mod epub;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::document_selection::SelectedDocument;
use crate::emitted_image_tally::EmittedImageTally;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWriteResult, ImageWriteWarning};

/// The per-run choice to extract a required EPUB cover, and its fallback.
///
/// There is no variant for "extract normal images": that is the absence of a
/// cover policy, so a run seeking no covers carries no value here at all. See
/// ADR-0010 — the alternative three-way enum is what let cover intent reach a
/// document kind that has no covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EpubCoverPolicy {
    /// Extract one required cover and emit nothing when there is none.
    CoverOnly,
    /// Extract one required cover, falling back to normal images when there is none.
    CoverThenNormalImages,
}

/// Immutable Document extraction module configured for one Extraction run.
pub(crate) struct DocumentExtraction {
    cover_policy: Option<EpubCoverPolicy>,
    image_write_pipeline: ImageWritePipeline,
}

impl DocumentExtraction {
    /// Binds Document extraction and Image write policy for every selected document.
    pub(crate) fn new(
        cover_policy: Option<EpubCoverPolicy>,
        image_write_pipeline: ImageWritePipeline,
    ) -> Self {
        Self {
            cover_policy,
            image_write_pipeline,
        }
    }

    /// Returns whether this module is configured to extract EPUB covers.
    pub(crate) fn is_epub_cover_extraction_configured(&self) -> bool {
        self.cover_policy.is_some()
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
                let (path, placement) = document.into_extraction_parts();
                docx::process_file(&path, &placement, &self.image_write_pipeline)
            }
            SelectedDocument::Epub(document) => {
                epub::extract(document, self.cover_policy, &self.image_write_pipeline)
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

/// Opaque facts retained by one completed or failed Document extraction.
///
/// The facts are the document's Emitted image tally and its ordered Document
/// extraction warnings, and nothing derived from them: what the document's output
/// was for is read off the tally's normal-image and cover totals by whoever needs
/// it (ADR-0017). Only the warnings are translated at this seam, so callers do not
/// depend on inner pipeline warning types.
#[derive(Debug)]
pub(crate) struct DocumentExtractionFacts {
    tally: EmittedImageTally,
    warnings: Vec<DocumentExtractionWarning>,
}

impl DocumentExtractionFacts {
    /// Hands over the pipeline's tally and translates its warnings at the Document extraction seam.
    fn from_image_write_result(result: ImageWriteResult) -> Self {
        Self {
            tally: result.tally,
            warnings: result
                .warnings
                .into_iter()
                .map(DocumentExtractionWarning::from_image_write_warning)
                .collect(),
        }
    }

    /// Builds facts from a tally and Document extraction warnings, for tests that script an outcome.
    ///
    /// Nothing is bypassed by assembling the value directly: a tally can only grow
    /// by recording images, so it carries no invariant left to check, and the
    /// warnings come from [`DocumentExtractionWarning::fabricated`], which keeps
    /// their wording owned by the production translation (ADR-0017).
    #[cfg(test)]
    pub(crate) fn fabricated(
        tally: EmittedImageTally,
        warnings: Vec<DocumentExtractionWarning>,
    ) -> Self {
        Self { tally, warnings }
    }

    /// Returns every image this document emitted before the outcome ended, by purpose and role.
    pub(crate) fn get_tally(&self) -> EmittedImageTally {
        self.tally
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

    /// Builds a warning from a fabricated Image write warning, for tests that script one.
    ///
    /// Delegates to the production translation so the wording stays owned here
    /// and no test has to restate it to obtain a warning.
    #[cfg(test)]
    pub(crate) fn fabricated(warning: ImageWriteWarning) -> Self {
        Self::from_image_write_warning(warning)
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

    /// Builds an error from a fabricated underlying cause, for tests that script a failure.
    ///
    /// Delegates to the production conversion so a scripted failure seals its
    /// source exactly as an extracted one does.
    #[cfg(test)]
    pub(crate) fn fabricated(source: anyhow::Error) -> Self {
        Self::from_source(source)
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
