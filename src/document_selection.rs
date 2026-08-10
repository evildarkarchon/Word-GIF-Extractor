//! Document selection for turning requested input paths into extraction work.

mod discovery;
mod progress;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::document_search_surface::DocumentSearchSurface;
use crate::epub_declarations::{EpubDeclarationSource, EpubDeclarations};
use crate::extraction_run_observation::{
    EpubMetadataPurpose, ExtractionRunObservation, ExtractionRunObserver,
};

use self::progress::{
    DocumentDiscoveryProgress, DocumentSelectionLifecycle, EpubDeduplicationCheck, EpubFilterCheck,
};

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

/// Filter criteria for selecting EPUB files by title and creator declarations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EpubFilter {
    /// Case-insensitive title substring required for an EPUB to be selected.
    pub title: Option<String>,
    /// Case-insensitive author substring required for an EPUB to be selected.
    pub author: Option<String>,
}

impl EpubFilter {
    /// Returns true if no EPUB declaration filter criteria are set.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.author.is_none()
    }
}

/// Immutable handoff produced by Document selection for one eligible document.
///
/// The authoritative variant is consumed by Document extraction exactly once.
#[derive(Debug)]
pub(crate) enum SelectedDocument {
    /// A selected DOCX document with DOCX-only extraction facts.
    Docx(SelectedDocx),
    /// A selected EPUB document with its optional declaration snapshot.
    Epub(SelectedEpub),
}

/// Opaque DOCX payload whose fields and construction belong to Document selection.
#[derive(Debug)]
pub(crate) struct SelectedDocx {
    path: PathBuf,
    output_dir: PathBuf,
    base_name: String,
    display_name: String,
}

/// Opaque EPUB payload whose fields and construction belong to Document selection.
#[derive(Debug)]
pub(crate) struct SelectedEpub {
    path: PathBuf,
    output_dir: PathBuf,
    base_name: String,
    display_name: String,
    epub_declarations: Option<EpubDeclarations>,
}

impl SelectedDocument {
    /// Returns the source path visible to the Extraction run before handoff.
    pub(crate) fn get_path(&self) -> &Path {
        match self {
            Self::Docx(document) => &document.path,
            Self::Epub(document) => &document.path,
        }
    }

    /// Returns the stable progress identity visible to the Extraction run.
    pub(crate) fn get_display_name(&self) -> &str {
        match self {
            Self::Docx(document) => &document.display_name,
            Self::Epub(document) => &document.display_name,
        }
    }
}

impl SelectedDocx {
    /// Creates a DOCX handoff after selection has established eligibility.
    fn new(path: PathBuf, output_dir: PathBuf, base_name: String, display_name: String) -> Self {
        Self {
            path,
            output_dir,
            base_name,
            display_name,
        }
    }

    /// Consumes the DOCX payload into the facts owned by Document extraction.
    pub(crate) fn into_extraction_parts(self) -> (PathBuf, PathBuf, String) {
        (self.path, self.output_dir, self.base_name)
    }
}

impl SelectedEpub {
    /// Creates an EPUB handoff after selection has established eligibility.
    fn new(
        path: PathBuf,
        output_dir: PathBuf,
        base_name: String,
        display_name: String,
        epub_declarations: Option<EpubDeclarations>,
    ) -> Self {
        Self {
            path,
            output_dir,
            base_name,
            display_name,
            epub_declarations,
        }
    }

    /// Transfers the EPUB handoff into the facts interpreted by the EPUB adapter.
    ///
    /// The selected identity and output placement remain fixed even when extraction must
    /// reacquire declarations because selection retained none.
    pub(crate) fn into_extraction_parts(
        self,
    ) -> (PathBuf, PathBuf, String, Option<EpubDeclarations>) {
        (
            self.path,
            self.output_dir,
            self.base_name,
            self.epub_declarations,
        )
    }
}

/// Options used to select documents for one extraction run.
pub struct DocumentSelectionOptions<'a> {
    /// Input files or directories to inspect.
    pub inputs: &'a [PathBuf],
    /// Whether directory inputs should be traversed recursively.
    pub recursive: bool,
    /// Optional output directory shared by every selected document.
    pub output: Option<&'a Path>,
    /// EPUB title and creator filter criteria.
    pub epub_filter: &'a EpubFilter,
}

/// Private pre-eligibility representation used during filtering and deduplication.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DocumentCandidate {
    Docx {
        path: PathBuf,
    },
    Epub {
        path: PathBuf,
        epub_declarations: Option<EpubDeclarations>,
    },
}

impl DocumentCandidate {
    /// Classifies a supported path into its private pre-eligibility variant.
    fn from_path(path: PathBuf) -> Option<Self> {
        match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("docx") => Some(Self::Docx { path }),
            Some("epub") => Some(Self::Epub {
                path,
                epub_declarations: None,
            }),
            _ => None,
        }
    }
}

/// Selects documents for extraction while reporting live progress snapshots and diagnostics.
///
/// Selection owns document discovery, EPUB declaration filtering, EPUB dedupe,
/// display identity, and per-document output placement. Returned documents are
/// already eligible for extraction; adapters should not re-check selection
/// filters. Missing inputs, requested-root inspection failures, and unreadable
/// EPUB declarations are reported as structured, non-fatal diagnostics through
/// the informational observer.
///
/// Selection reports into the Extraction run observation stream directly. The
/// observer is informational: callbacks cannot cancel selection or alter which
/// documents are returned.
///
/// Every observation of the world is made through `surface` and every EPUB
/// declaration through `declarations`. ADR-0008 keeps both as separate
/// parameters rather than fields on [`DocumentSelectionOptions`]: those are the
/// per-run policy choices, and neither a way of seeing the world nor a way of
/// reading declarations is one of them.
///
/// Substituting `declarations` cannot change ADR-0002's retention rule. This
/// function decides when a declaration is acquired and when a retained one is
/// reused; the source only answers.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    surface: &dyn DocumentSearchSurface,
    declarations: &dyn EpubDeclarationSource,
    observer: &mut impl ExtractionRunObserver,
) -> Vec<SelectedDocument> {
    let mut lifecycle = DocumentSelectionLifecycle::new(observer);

    let candidates =
        discovery::discover_documents(options.inputs, options.recursive, surface, &mut lifecycle);
    let filtered = if !options.epub_filter.is_empty() {
        filter_epub_files(
            candidates,
            options.epub_filter,
            declarations,
            &mut lifecycle,
        )
    } else {
        candidates
    };
    let deduplicated = deduplicate_epubs_by_declarations(filtered, declarations, &mut lifecycle);

    deduplicated
        .into_iter()
        .map(|candidate| selected_document_from_candidate(candidate, options.output))
        .collect()
}

/// Checks if a candidate is an EPUB file.
fn is_epub(candidate: &DocumentCandidate) -> bool {
    matches!(candidate, DocumentCandidate::Epub { .. })
}

/// Filters EPUB files by title and creator declarations while passing non-EPUB files through.
fn filter_epub_files(
    files: Vec<DocumentCandidate>,
    filter: &EpubFilter,
    declarations: &dyn EpubDeclarationSource,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    // Separate EPUB files from other document types.
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(is_epub);
    let total = epub_files.len();

    lifecycle.filtering(!epub_files.is_empty(), filter, total, |progress| {
        let mut matching_epubs = Vec::new();

        for candidate in epub_files {
            let DocumentCandidate::Epub { path, .. } = candidate else {
                continue;
            };
            let outcome = match declarations.acquire(&path) {
                Ok(declarations) if matches_filter(&declarations, filter) => {
                    matching_epubs.push(DocumentCandidate::Epub {
                        path,
                        epub_declarations: Some(declarations),
                    });
                    EpubFilterCheck::Matched
                }
                Ok(_) => EpubFilterCheck::Rejected, // File doesn't match filter, skip.
                Err(error) => {
                    // Filtering cannot accept an EPUB whose requested declarations are unreadable.
                    progress.diagnostic(ExtractionRunObservation::UnreadableEpubMetadata {
                        path,
                        purpose: EpubMetadataPurpose::Filtering,
                        detail: error.to_string(),
                    });
                    EpubFilterCheck::Rejected
                }
            };
            progress.record_check(outcome);
        }

        // Combine matching EPUBs with other document types.
        let mut result = matching_epubs;
        result.extend(other_files);
        result
    })
}

/// Deduplicates EPUB files based on their creator and title declarations.
///
/// Keeps the first occurrence of each unique (author, title) combination.
/// Non-EPUB files are passed through unchanged. EPUBs without those declarations are
/// deduplicated by filename.
fn deduplicate_epubs_by_declarations(
    files: Vec<DocumentCandidate>,
    declarations: &dyn EpubDeclarationSource,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(is_epub);
    let total = epub_files.len();

    lifecycle.deduplicating(!epub_files.is_empty(), total, |progress| {
        // Use a HashMap to track seen (author, title) combinations.
        // Key: (lowercase author, lowercase title) for case-insensitive deduplication.
        let mut seen: HashMap<(String, String), PathBuf> = HashMap::new();
        let mut unique_epubs = Vec::new();

        for candidate in epub_files {
            let DocumentCandidate::Epub {
                path,
                mut epub_declarations,
            } = candidate
            else {
                continue;
            };
            // ADR-0002: declarations retained by filtering are authoritative for the
            // run, so deduplication asks the source only when it has none.
            if epub_declarations.is_none() {
                match declarations.acquire(&path) {
                    Ok(declarations) => epub_declarations = Some(declarations),
                    Err(error) => {
                        progress.diagnostic(ExtractionRunObservation::UnreadableEpubMetadata {
                            path: path.clone(),
                            purpose: EpubMetadataPurpose::Deduplication,
                            detail: error.to_string(),
                        })
                    }
                }
            }

            let key = match epub_declarations.as_ref() {
                Some(declarations)
                    if declarations.creator().is_some() || declarations.title().is_some() =>
                {
                    epub_dedupe_key(declarations)
                }
                _ => filename_dedupe_key(&path),
            };

            // Only add if we haven't seen this combination before.
            let outcome = if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(key)
            {
                entry.insert(path.clone());
                unique_epubs.push(DocumentCandidate::Epub {
                    path,
                    epub_declarations,
                });
                EpubDeduplicationCheck::Unique
            } else {
                EpubDeduplicationCheck::Duplicate
            };
            progress.record_check(outcome);
        }

        // Combine unique EPUBs with other document types.
        let mut result = unique_epubs;
        result.extend(other_files);
        result
    })
}

/// Builds the case-insensitive dedupe key from retained EPUB declarations.
fn epub_dedupe_key(declarations: &EpubDeclarations) -> (String, String) {
    let creator_key = declarations
        .creator()
        .map(|value| value.trim().to_lowercase())
        .unwrap_or_default();
    let title_key = declarations
        .title()
        .map(|value| value.trim().to_lowercase())
        .unwrap_or_default();
    (creator_key, title_key)
}

/// Builds a filename fallback dedupe key for EPUBs without usable declarations.
fn filename_dedupe_key(path: &Path) -> (String, String) {
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    (String::new(), filename)
}

/// Checks whether title and creator declarations match the case-insensitive filter.
fn matches_filter(declarations: &EpubDeclarations, filter: &EpubFilter) -> bool {
    let title_matches = filter.title.as_ref().is_none_or(|f| {
        declarations
            .title()
            .is_some_and(|t| t.to_lowercase().contains(&f.to_lowercase()))
    });

    let author_matches = filter.author.as_ref().is_none_or(|f| {
        declarations
            .creator()
            .is_some_and(|a| a.to_lowercase().contains(&f.to_lowercase()))
    });

    title_matches && author_matches
}

/// Builds one selected document from a filtered and deduplicated candidate.
fn selected_document_from_candidate(
    candidate: DocumentCandidate,
    global_output: Option<&Path>,
) -> SelectedDocument {
    match candidate {
        DocumentCandidate::Docx { path } => {
            let output_dir = resolve_output_dir(&path, global_output);
            let base_name = fallback_base_name(&path);
            let display_name = fallback_display_name(&path);
            SelectedDocument::Docx(SelectedDocx::new(path, output_dir, base_name, display_name))
        }
        DocumentCandidate::Epub {
            path,
            epub_declarations,
        } => {
            let output_dir = resolve_output_dir(&path, global_output);
            let fallback_base_name = fallback_base_name(&path);
            let fallback_display_name = fallback_display_name(&path);
            let has_declaration_identity = epub_declarations.as_ref().is_some_and(|declarations| {
                declarations
                    .creator()
                    .is_some_and(|creator| !creator.trim().is_empty())
                    || declarations
                        .title()
                        .is_some_and(|title| !title.trim().is_empty())
            });
            let base_name = format_epub_base_name(
                epub_declarations
                    .as_ref()
                    .and_then(EpubDeclarations::creator),
                epub_declarations.as_ref().and_then(EpubDeclarations::title),
                &fallback_base_name,
            );
            // Selection fixes the run identity from retained declarations only;
            // extraction-time declaration retries cannot revise this fallback.
            let display_name = if has_declaration_identity {
                base_name.clone()
            } else {
                fallback_display_name
            };

            SelectedDocument::Epub(SelectedEpub::new(
                path,
                output_dir,
                base_name,
                display_name,
                epub_declarations,
            ))
        }
    }
}

/// Resolves the output directory for a single input file.
///
/// When `global_output` is set, all files use that directory. Otherwise images
/// are written beside the source file (its parent directory).
fn resolve_output_dir(input_path: &Path, global_output: Option<&Path>) -> PathBuf {
    match global_output {
        Some(dir) => dir.to_path_buf(),
        None => input_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

/// Builds the default output filename stem from a path.
fn fallback_base_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Builds a fallback display name from a path.
fn fallback_display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Formats a filename from EPUB creator and title declarations.
fn format_epub_base_name(author: Option<&str>, title: Option<&str>, fallback: &str) -> String {
    let author = author.map(|s| s.trim()).filter(|s| !s.is_empty());
    let title = title.map(|s| s.trim()).filter(|s| !s.is_empty());

    let raw_name = match (author, title) {
        (Some(a), Some(t)) => format!("{} - {}", a, t),
        (None, Some(t)) => t.to_string(),
        (Some(a), None) => a.to_string(),
        (None, None) => fallback.to_string(),
    };

    sanitize_filename(&raw_name)
}

#[cfg(test)]
mod tests;
