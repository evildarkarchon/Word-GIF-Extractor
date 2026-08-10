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
                let target = document.into_extraction_inputs();
                let placement = target.get_placement();
                docx::process_file(
                    target.get_source(),
                    placement.get_dir(),
                    placement.get_base_name(),
                    &self.image_write_pipeline,
                )
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

/// What one document's emitted output was for.
///
/// The three states are closed and mutually exclusive, which is what a
/// normal-image boolean could not express: a document that emitted nothing and a
/// document that emitted covers only both denied that boolean, so a caller could
/// not tell them apart. This type is owned by Document extraction rather than
/// reusing the run-level [`crate::extraction_run_observation::ExtractionOutputKind`],
/// because that type lives in a module that already depends on this one and
/// importing it back would close the cycle ADR-0004 removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentOutputPurpose {
    /// Every emitted file came from required-cover extraction.
    CoversOnly,
    /// At least one emitted file came from normal-image extraction.
    IncludedNormalImages,
    /// The document emitted no files at all.
    NothingEmitted,
}

impl DocumentOutputPurpose {
    /// Combines this classification with a later document's, most inclusive winning.
    ///
    /// `NothingEmitted` is the identity because a document that emitted nothing
    /// cannot change what a run's output was for, and `IncludedNormalImages`
    /// absorbs because one normal image anywhere in a run means the run did not
    /// produce covers only. The combination rule lives here, with the variants it
    /// orders; the accumulator that applies it across documents does not, so
    /// Document extraction stays stateless across documents.
    pub(crate) fn merged_with(self, later: Self) -> Self {
        match (self, later) {
            (Self::IncludedNormalImages, _) | (_, Self::IncludedNormalImages) => {
                Self::IncludedNormalImages
            }
            (Self::CoversOnly, _) | (_, Self::CoversOnly) => Self::CoversOnly,
            (Self::NothingEmitted, Self::NothingEmitted) => Self::NothingEmitted,
        }
    }
}

/// Opaque facts retained by one completed or failed Document extraction.
///
/// The value translates Image write pipeline accounting and warnings at the
/// Document extraction seam so callers do not depend on inner pipeline types.
#[derive(Debug)]
pub(crate) struct DocumentExtractionFacts {
    emitted_image_totals: EmittedImageTotals,
    output_purpose: DocumentOutputPurpose,
    warnings: Vec<DocumentExtractionWarning>,
}

impl DocumentExtractionFacts {
    /// Translates inner Image write facts at the Document extraction seam.
    ///
    /// A debug assertion pins the emitted-image partition where the pipeline's
    /// counts cross into Document extraction; see the comment on it. The
    /// translation itself is infallible.
    fn from_image_write_result(result: ImageWriteResult) -> Self {
        // Tripwire for a fabricated Image write result, not a runtime condition.
        // The pipeline itself can no longer violate this: `EmittedImageRole`
        // gives each emitted image exactly one role, so the counts it folds
        // satisfy the rule by construction. What remains reachable is
        // `ImageWriteResult::new`, which assembles a complete outcome from
        // already-produced facts and validates none of them. Checking here, at
        // the boundary those counts cross into Document extraction, is what
        // makes such a result trip on arrival rather than three modules away.
        //
        // Nothing reachable through Document extraction's interface can produce
        // a violating result, so there is deliberately no test for it. Firing it
        // would take hand-built pipeline counts reaching around that interface —
        // do not contort a test into doing so. The existing Extraction run test
        // pinning three emitted images as one converted, one conversion-skipped
        // and one GIF-routed is the boundary case that exercises it in debug
        // builds, at equality.
        //
        // The sum saturates rather than wrapping so the tripwire fails closed:
        // a build with debug assertions but no overflow checks would otherwise
        // wrap a bogus total back under the emitted count and stay silent.
        debug_assert!(
            result
                .counts
                .converted
                .saturating_add(result.counts.skipped)
                .saturating_add(result.counts.gifs_routed)
                <= result.counts.extracted,
            "Image write pipeline classified more images than it emitted: \
             converted {} + skipped {} + routed {} > emitted {}",
            result.counts.converted,
            result.counts.skipped,
            result.counts.gifs_routed,
            result.counts.extracted,
        );

        // "Nothing emitted" is read off the emitted count rather than from the
        // pipeline, which reports only whether the normal batch produced output.
        // The pipeline never emits a normal image and a required cover for the
        // same document, so a non-zero emitted count with no normal-image output
        // can only be required-cover output.
        let output_purpose = if result.counts.extracted == 0 {
            DocumentOutputPurpose::NothingEmitted
        } else if result.has_normal_image_output() {
            DocumentOutputPurpose::IncludedNormalImages
        } else {
            DocumentOutputPurpose::CoversOnly
        };
        Self {
            emitted_image_totals: EmittedImageTotals {
                emitted_images: result.counts.extracted,
                routed_gifs: result.counts.gifs_routed,
                converted_images: result.counts.converted,
                skipped_conversions: result.counts.skipped,
            },
            output_purpose,
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

    /// Returns what this document's emitted output was for.
    pub(crate) fn get_output_purpose(&self) -> DocumentOutputPurpose {
        self.output_purpose
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
