//! Shared acquisition of payload-free facts declared by EPUB documents.

use epub::doc::{DocError, EpubDoc};
use std::fmt;
use std::path::{Path, PathBuf};

/// Complete payload-free facts declared by one readable EPUB document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpubDeclarations {
    title: Option<String>,
    creator: Option<String>,
    cover_id: Option<String>,
    resources: Vec<EpubResourceDeclaration>,
}

impl EpubDeclarations {
    /// Acquires all declaration facts needed by Document selection and Document extraction.
    ///
    /// Resource payloads are not read; ADR-0001 reserves those reads for the
    /// independently opened EPUB resource archive.
    pub(crate) fn acquire(path: &Path) -> Result<Self, EpubDeclarationError> {
        let doc = EpubDoc::new(path).map_err(EpubDeclarationError::new)?;
        let title = doc.mdata("title").map(|metadata| metadata.value.clone());
        let creator = doc.mdata("creator").map(|metadata| metadata.value.clone());
        let cover_id = doc.get_cover_id();
        let resources = doc
            .resources
            .iter()
            .map(|(id, item)| EpubResourceDeclaration::new(id, &item.path, &item.mime))
            .collect();

        Ok(Self {
            title,
            creator,
            cover_id,
            resources,
        })
    }

    /// Returns the declared title when present.
    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the declared creator when present.
    pub(crate) fn creator(&self) -> Option<&str> {
        self.creator.as_deref()
    }

    /// Returns the declared cover resource identifier when present.
    pub(crate) fn cover_id(&self) -> Option<&str> {
        self.cover_id.as_deref()
    }

    /// Returns every declared resource without acquiring its archive payload.
    pub(crate) fn resources(&self) -> &[EpubResourceDeclaration] {
        &self.resources
    }
}

/// Payload-free declaration of one EPUB manifest resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpubResourceDeclaration {
    id: String,
    path: PathBuf,
    mime: String,
}

impl EpubResourceDeclaration {
    /// Captures one payload-free resource declaration from an EPUB manifest.
    pub(crate) fn new(
        id: impl Into<String>,
        path: impl Into<PathBuf>,
        mime: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            mime: mime.into(),
        }
    }

    /// Returns the manifest identifier used by cover declarations and spine references.
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Returns the manifest path before archive payload resolution.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the media type declared by the EPUB manifest.
    pub(crate) fn mime(&self) -> &str {
        &self.mime
    }
}

/// Failure to acquire declarations from an EPUB document.
#[derive(Debug)]
pub(crate) struct EpubDeclarationError {
    source: DocError,
}

impl EpubDeclarationError {
    /// Preserves the EPUB parser failure for workflow-specific translation.
    fn new(source: DocError) -> Self {
        Self { source }
    }
}

impl fmt::Display for EpubDeclarationError {
    /// Formats a presentation-free declaration acquisition failure.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Failed to open EPUB file: {}", self.source)
    }
}

impl std::error::Error for EpubDeclarationError {
    /// Returns the underlying EPUB parser error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(test)]
mod tests;
