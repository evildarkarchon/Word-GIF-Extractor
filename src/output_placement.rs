//! Output placement: where one Selected document's images are written.
//!
//! Document selection fixes a placement when it selects a document, and the value
//! travels whole from the Selected document down to the Image write pipeline. It
//! lives in a module of its own because both ends use it and neither end otherwise
//! depends on the other; owning it in either would add that edge.

use std::path::{Path, PathBuf};

/// The output directory and base name one Selected document's images are written under.
///
/// The base name is the one the document's Document identity decided; the placement
/// carries it and never revises it. Keeping the pair in one value is what stops the
/// directory being passed where a source path was expected, or the two being
/// separated on their way to the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputPlacement {
    output_dir: PathBuf,
    base_name: String,
}

impl OutputPlacement {
    /// Builds a placement from an output directory and an identity-decided base name.
    ///
    /// Open to the whole crate on purpose: a placement is a plain value, and what
    /// only Document selection can grant is being a Selected document, not this.
    pub(crate) fn new(output_dir: impl Into<PathBuf>, base_name: impl Into<String>) -> Self {
        Self {
            output_dir: output_dir.into(),
            base_name: base_name.into(),
        }
    }

    /// Returns the directory images are written into.
    pub(crate) fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// Returns the base name output files are named after.
    pub(crate) fn base_name(&self) -> &str {
        &self.base_name
    }
}
