//! Word Image Extractor - extraction of images from DOCX and EPUB files.
//!
//! This library treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats. It owns the whole module tree; `src/main.rs` links
//! against it and adds only the terminal presentation the CLI needs.
//!
//! # Interface
//!
//! Every module below is crate-private, and the only items reachable from outside are
//! the ones re-exported at the root. That set is deliberately temporary and wider than
//! it will end up: terminal handling still lives in the binary, so the observation
//! vocabulary it renders has to cross the library boundary. Once Extraction run
//! presentation moves in here, the interface collapses to a single entry point and the
//! rest of this list goes back to being crate-private.
//!
//! Two things about the set are worth knowing before adding to it.
//!
//! Some types are public but not re-exported — the Extraction run request and the
//! produced-output facts. They appear inside re-exported signatures, so they have to be
//! public, but nothing outside needs to name them: the binary destructures the prepared
//! run and hands the request straight back. Leaving them unnameable is the narrower
//! choice, and re-exporting a type merely because it is already public is not a reason.
//!
//! [`Args`] is re-exported from Extraction run intake rather than defined here. Intake
//! owns the type it takes as input, so no production module imports the crate root and
//! the fourteen flags are declared once. Its fields are crate-visible: outside the crate
//! it can only be parsed and handed back to [`prepare_extraction_run`].
//!
//! [`ConversionFacts`], [`GifRoutingFacts`] and [`ExtractionRunOutcome::try_produced`]
//! are the one concession: they are published for the binary's terminal-summary tests,
//! which have to build a produced outcome to render it, and which cannot move in here
//! while the rendering they test is still in the binary. They are the whole of that
//! concession, and it closes when presentation moves. Nothing else should be widened to
//! let a test reach in — in-crate tests stay in-crate for exactly that reason, and a
//! test whose subject lives in the library belongs in the library with it.

mod conversion;
mod document_extraction;
mod document_selection;
mod epub_declarations;
mod extraction_run;
mod extraction_run_intake;
mod image_format;
mod image_write_pipeline;
#[cfg(test)]
mod test_support;

pub use crate::conversion::ConversionPolicyError;
pub use crate::document_extraction::DocumentExtractionWarning;
pub use crate::document_selection::{
    DocumentSelectionDiagnostic, DocumentSelectionPhaseStatus, DocumentSelectionProgress,
    DocumentSelectionScanScope, EpubFilter, EpubMetadataPurpose,
};
pub use crate::extraction_run::{
    ConversionFacts, ExtractionOutputKind, ExtractionRunObservation, ExtractionRunObserver,
    ExtractionRunOutcome, GifRoutingFacts, run as execute_extraction_run,
};
pub use crate::extraction_run_intake::{
    Args, ExtractionRunIntakeError, PreRunNotice, PreparedExtractionRun,
    prepare as prepare_extraction_run,
};
