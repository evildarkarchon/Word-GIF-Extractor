//! Document identity: the one place Document selection decides who a document is.
//!
//! Duplicate recognition, output base name, display name and EPUB filter matching all
//! read the same creator and title declarations. They used to read them separately,
//! with three different answers to "was anything declared?": dedupe keyed on presence
//! alone, while display and naming required a non-blank value. The `epub` crate
//! reports an empty `<dc:title/>` as a present, empty value, so those answers
//! disagreed on real books. This module normalizes the declarations once, at
//! construction, and every question is then asked of the normalized value.
//!
//! The identity is built on demand from whatever declarations a candidate holds at the
//! moment it is needed, rather than stored on the candidate. Dedupe may acquire
//! declarations that filtering never saw, and a stored identity could fall out of step
//! with them; a derived one cannot.

use std::path::Path;

use crate::epub_declarations::EpubDeclarations;

use super::EpubFilter;

/// The identity Document selection assigns one document.
///
/// `Declared` holds at least one non-blank, trimmed declaration; any value that trims
/// to nothing was discarded at construction, so no query re-checks blankness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DocumentIdentity {
    /// Identity taken from a non-blank creator or title declaration.
    Declared {
        creator: Option<String>,
        title: Option<String>,
    },
    /// Identity taken from the document's path.
    Path {
        /// File name including its extension, shown as the display name.
        file_name: String,
        /// Output base name, already in its final form for this document kind.
        base_name: String,
    },
}

/// Key under which Document selection recognizes duplicate EPUBs.
///
/// Declared and path keys are separate variants, so a book whose declared title is
/// literally `book.epub` can never collide with an undeclared file named `book.epub`.
/// Both variants are case-insensitive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum DedupeKey {
    /// Case-folded creator and title; an absent half is the empty string.
    Declared { creator: String, title: String },
    /// Case-folded file name, extension included.
    Path { file_name: String },
}

/// EPUB filter terms case-folded once for a whole filtering phase.
///
/// Terms are used exactly as typed apart from case: leading or trailing spaces a user
/// put in `--title` or `--author` are deliberate and still have to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EpubFilterTerms {
    title: Option<String>,
    author: Option<String>,
}

impl EpubFilterTerms {
    /// Case-folds the requested title and author terms.
    pub(super) fn new(filter: &EpubFilter) -> Self {
        Self {
            title: filter.title.as_deref().map(str::to_lowercase),
            author: filter.author.as_deref().map(str::to_lowercase),
        }
    }
}

impl DocumentIdentity {
    /// Builds the path identity used for every DOCX.
    ///
    /// The base name is the raw file stem. Unlike an undeclared EPUB's, it is not
    /// sanitized; see [`DocumentIdentity::of_epub`] for why the two differ.
    pub(super) fn of_path(path: &Path) -> Self {
        Self::Path {
            file_name: file_name(path),
            base_name: file_stem(path),
        }
    }

    /// Builds an EPUB's identity from its creator and title declarations.
    ///
    /// A declaration counts only when it is non-blank after trimming; when neither
    /// counts, the identity falls back to the path. That fallback sanitizes the stem,
    /// which a DOCX's path identity does not do. The difference predates this module
    /// and is kept so every output name stays byte-for-byte unchanged.
    pub(super) fn of_epub(creator: Option<&str>, title: Option<&str>, path: &Path) -> Self {
        let creator = non_blank(creator);
        let title = non_blank(title);
        if creator.is_none() && title.is_none() {
            return Self::Path {
                file_name: file_name(path),
                base_name: sanitize_filename(&file_stem(path)),
            };
        }
        Self::Declared { creator, title }
    }

    /// Builds an EPUB's identity from retained declarations, when selection has them.
    ///
    /// `None` means the declarations could not be read, which is the same as declaring
    /// nothing: the identity comes from the path.
    pub(super) fn of_epub_declarations(
        declarations: Option<&EpubDeclarations>,
        path: &Path,
    ) -> Self {
        Self::of_epub(
            declarations.and_then(EpubDeclarations::creator),
            declarations.and_then(EpubDeclarations::title),
            path,
        )
    }

    /// Returns the case-insensitive key under which duplicates are recognized.
    pub(super) fn dedupe_key(&self) -> DedupeKey {
        match self {
            Self::Declared { creator, title } => DedupeKey::Declared {
                creator: creator.as_deref().unwrap_or_default().to_lowercase(),
                title: title.as_deref().unwrap_or_default().to_lowercase(),
            },
            Self::Path { file_name, .. } => DedupeKey::Path {
                file_name: file_name.to_lowercase(),
            },
        }
    }

    /// Returns the output filename stem for this document's images.
    ///
    /// A declared identity reads `creator - title`, or whichever one is declared,
    /// sanitized for use as a filename.
    pub(super) fn base_name(&self) -> String {
        match self {
            Self::Declared { creator, title } => {
                let raw_name = match (creator, title) {
                    (Some(creator), Some(title)) => format!("{creator} - {title}"),
                    (None, Some(title)) => title.clone(),
                    (Some(creator), None) => creator.clone(),
                    // Construction never builds a declared identity with neither half.
                    (None, None) => String::new(),
                };
                sanitize_filename(&raw_name)
            }
            Self::Path { base_name, .. } => base_name.clone(),
        }
    }

    /// Returns the name the run shows for this document while it is processed.
    ///
    /// A declared identity shows its sanitized base name; a path identity shows the
    /// file name with its extension.
    pub(super) fn display_name(&self) -> String {
        match self {
            Self::Declared { .. } => self.base_name(),
            Self::Path { file_name, .. } => file_name.clone(),
        }
    }

    /// Checks whether this identity satisfies every requested filter term.
    ///
    /// Each term is a case-insensitive substring match against the matching
    /// declaration. A path identity declares nothing, so it satisfies only a filter
    /// that requests nothing.
    pub(super) fn matches(&self, terms: &EpubFilterTerms) -> bool {
        let (creator, title) = match self {
            Self::Declared { creator, title } => (creator.as_deref(), title.as_deref()),
            Self::Path { .. } => (None, None),
        };
        term_matches(terms.title.as_deref(), title)
            && term_matches(terms.author.as_deref(), creator)
    }
}

/// Checks one case-folded filter term against one declaration.
fn term_matches(term: Option<&str>, declared: Option<&str>) -> bool {
    term.is_none_or(|term| declared.is_some_and(|value| value.to_lowercase().contains(term)))
}

/// Trims a declaration and discards it when nothing is left.
fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Returns the path's file name, or the whole path when it has none.
///
/// Every selection candidate has a supported extension and therefore a file name;
/// the fallback only keeps this total.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Returns the path's file stem, or `unknown` when it has none.
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Sanitizes declared document text for use as an output filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
