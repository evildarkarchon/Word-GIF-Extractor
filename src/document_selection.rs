//! Document selection for turning requested input paths into extraction work.

mod discovery;
mod document_identity;
mod progress;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::epub_declarations::EpubDeclarations;
use crate::extraction_run_observation::{
    EpubMetadataPurpose, ExtractionRunObservation, ExtractionRunObserver,
};

use self::document_identity::{DedupeKey, DocumentIdentity, EpubFilterTerms};
use self::progress::{
    DocumentDiscoveryProgress, DocumentSelectionLifecycle, EpubDeduplicationCheck, EpubFilterCheck,
};

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
    Docx { path: PathBuf },
    Epub(EpubCandidate),
}

/// Private pre-eligibility EPUB that the EPUB filtering and deduplication phases check.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EpubCandidate {
    path: PathBuf,
    epub_declarations: Option<EpubDeclarations>,
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
            Some("epub") => Some(Self::Epub(EpubCandidate {
                path,
                epub_declarations: None,
            })),
            _ => None,
        }
    }
}

/// Selects documents for extraction while reporting live progress snapshots and diagnostics.
///
/// Selection owns document discovery, EPUB declaration filtering, EPUB dedupe,
/// Document identity, and per-document output placement. Returned documents are
/// already eligible for extraction; adapters should not re-check selection
/// filters. Missing inputs, requested-root inspection failures, and unreadable
/// EPUB declarations are reported as structured, non-fatal diagnostics through
/// the informational observer.
///
/// Selection reports into the Extraction run observation stream directly. The
/// observer is informational: callbacks cannot cancel selection or alter which
/// documents are returned.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    observer: &mut impl ExtractionRunObserver,
) -> Vec<SelectedDocument> {
    let mut lifecycle = DocumentSelectionLifecycle::new(observer);

    let candidates =
        discovery::discover_documents(options.inputs, options.recursive, &mut lifecycle);
    let filtered = if !options.epub_filter.is_empty() {
        filter_epub_files(candidates, options.epub_filter, &mut lifecycle)
    } else {
        candidates
    };
    let deduplicated = deduplicate_epubs_by_declarations(filtered, &mut lifecycle);

    deduplicated
        .into_iter()
        .map(|candidate| selected_document_from_candidate(candidate, options.output))
        .collect()
}

/// Splits candidates into the EPUBs a declaration phase checks and every other document.
///
/// Both groups keep their encounter order, and the EPUB group is typed so the phases
/// iterating it never meet a non-EPUB candidate.
fn partition_epubs(files: Vec<DocumentCandidate>) -> (Vec<EpubCandidate>, Vec<DocumentCandidate>) {
    // Separate EPUB files from other document types.
    let mut epub_files = Vec::new();
    let mut other_files = Vec::new();
    for candidate in files {
        match candidate {
            DocumentCandidate::Epub(epub) => epub_files.push(epub),
            other => other_files.push(other),
        }
    }
    (epub_files, other_files)
}

/// Filters EPUB files by title and creator declarations while passing non-EPUB files through.
fn filter_epub_files(
    files: Vec<DocumentCandidate>,
    filter: &EpubFilter,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    let (epub_files, other_files) = partition_epubs(files);
    let total = epub_files.len();
    let terms = EpubFilterTerms::new(filter);

    lifecycle.filtering(!epub_files.is_empty(), filter, total, |progress| {
        let mut matching_epubs = Vec::new();

        for EpubCandidate { path, .. } in epub_files {
            let outcome = match EpubDeclarations::acquire(&path) {
                Ok(declarations)
                    if DocumentIdentity::of_epub_declarations(Some(&declarations), &path)
                        .matches(&terms) =>
                {
                    matching_epubs.push(DocumentCandidate::Epub(EpubCandidate {
                        path,
                        epub_declarations: Some(declarations),
                    }));
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
/// Non-EPUB files are passed through unchanged. EPUBs without a non-blank creator or
/// title declaration are deduplicated by filename.
fn deduplicate_epubs_by_declarations(
    files: Vec<DocumentCandidate>,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    let (epub_files, other_files) = partition_epubs(files);
    let total = epub_files.len();

    lifecycle.deduplicating(!epub_files.is_empty(), total, |progress| {
        // Track the Document identity keys already seen. Keys are case-insensitive,
        // and declared keys never equal filename keys.
        let mut seen: HashMap<DedupeKey, PathBuf> = HashMap::new();
        let mut unique_epubs = Vec::new();

        for EpubCandidate {
            path,
            mut epub_declarations,
        } in epub_files
        {
            if epub_declarations.is_none() {
                match EpubDeclarations::acquire(&path) {
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

            let key = DocumentIdentity::of_epub_declarations(epub_declarations.as_ref(), &path)
                .dedupe_key();

            // Only add if we haven't seen this combination before.
            let outcome = if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(key)
            {
                entry.insert(path.clone());
                unique_epubs.push(DocumentCandidate::Epub(EpubCandidate {
                    path,
                    epub_declarations,
                }));
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

/// Builds one selected document from a filtered and deduplicated candidate.
fn selected_document_from_candidate(
    candidate: DocumentCandidate,
    global_output: Option<&Path>,
) -> SelectedDocument {
    match candidate {
        DocumentCandidate::Docx { path } => {
            let output_dir = resolve_output_dir(&path, global_output);
            let identity = DocumentIdentity::of_path(&path);
            SelectedDocument::Docx(SelectedDocx::new(
                path,
                output_dir,
                identity.base_name(),
                identity.display_name(),
            ))
        }
        DocumentCandidate::Epub(EpubCandidate {
            path,
            epub_declarations,
        }) => {
            let output_dir = resolve_output_dir(&path, global_output);
            // Selection fixes the run identity from retained declarations only;
            // extraction-time declaration retries cannot revise this fallback.
            let identity =
                DocumentIdentity::of_epub_declarations(epub_declarations.as_ref(), &path);

            SelectedDocument::Epub(SelectedEpub::new(
                path,
                output_dir,
                identity.base_name(),
                identity.display_name(),
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

#[cfg(test)]
mod tests;
