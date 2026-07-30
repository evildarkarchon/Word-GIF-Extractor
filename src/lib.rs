//! Word Image Extractor - extraction of images from DOCX and EPUB files.
//!
//! This library treats DOCX and EPUB files as ZIP archives and extracts image files
//! matching specified formats. It owns the whole module tree, including the terminal
//! presentation of a run; `src/main.rs` links against it and adds nothing but
//! argument parsing.
//!
//! # Interface
//!
//! Every module below is crate-private, and the only items reachable from outside are
//! the four re-exported here:
//!
//! - [`Args`], the parsed command-line options,
//! - [`run_cli`], which performs one run and renders it,
//! - [`TerminalOutput`], the destination it renders into,
//! - [`Capture`], the readback for a destination that captured instead of printing.
//!
//! Everything else — the Document selection progress and diagnostic vocabulary, the
//! Extraction run observation stream and its observer, the run request, outcome and
//! fact types, the intake error and its notices, and the conversion target and policy
//! error — is crate-private. The observation stream has no consumer outside the
//! crate now that presentation lives inside it, so it is not duplicated into
//! presentation-neutral equivalents at the boundary.
//!
//! [`Args`] is re-exported from Extraction run intake rather than defined here. Intake
//! owns the type it takes as input, so no production module imports the crate root and
//! the fourteen flags are declared once. Its fields are crate-visible: outside the crate
//! it can only be parsed and handed to [`run_cli`].
//!
//! Nothing here should be widened to let a test reach in — in-crate tests stay in-crate
//! for exactly that reason, and a test whose subject lives in the library belongs in the
//! library with it. A test that needs to see what a run said takes
//! [`TerminalOutput::captured`] instead.

mod conversion;
mod document_extraction;
mod document_selection;
mod epub_declarations;
mod extraction_run;
mod extraction_run_intake;
mod extraction_run_presentation;
mod image_format;
mod image_write_pipeline;
#[cfg(test)]
mod test_support;

pub use crate::extraction_run_intake::Args;
pub use crate::extraction_run_presentation::{Capture, TerminalOutput, run_cli};
