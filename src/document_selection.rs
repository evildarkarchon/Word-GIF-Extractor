//! Document selection for turning requested input paths into extraction work.

mod progress;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::epub_declarations::EpubDeclarations;

pub use self::progress::{
    DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionPhaseStatus,
    DocumentSelectionProgress, DocumentSelectionScanScope, EpubMetadataPurpose,
};
use self::progress::{
    DocumentSelectionLifecycle, EpubDeduplicationCheck, EpubFilterCheck, ScanningProgress,
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

    /// Consumes the EPUB payload into extraction facts and its declaration snapshot.
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
    /// Input files or directories to scan.
    pub inputs: &'a [PathBuf],
    /// Whether directory inputs should be scanned recursively.
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
/// filters. Missing inputs and unreadable EPUB declarations are reported as
/// structured, non-fatal diagnostics through the informational observer.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    observer: &mut impl DocumentSelectionObserver,
) -> Vec<SelectedDocument> {
    let mut lifecycle = DocumentSelectionLifecycle::new(observer);

    for input_path in options.inputs {
        if !input_path.exists() {
            lifecycle.diagnostic(DocumentSelectionDiagnostic::MissingInput {
                path: input_path.clone(),
            });
        }
    }

    let candidates = collect_document_files(options.inputs, options.recursive, &mut lifecycle);
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

/// Checks if a candidate is an EPUB file.
fn is_epub(candidate: &DocumentCandidate) -> bool {
    matches!(candidate, DocumentCandidate::Epub { .. })
}

/// Collects all document files from the input paths.
fn collect_document_files(
    inputs: &[PathBuf],
    recursive: bool,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    let scope = if recursive && inputs.iter().any(|path| path.is_dir()) {
        DocumentSelectionScanScope::RecursiveDirectories
    } else {
        DocumentSelectionScanScope::RequestedInputs
    };

    lifecycle.scanning(!inputs.is_empty(), scope, |progress| {
        let mut files = Vec::new();

        for input_path in inputs {
            if !input_path.exists() {
                continue;
            }

            if input_path.is_file() {
                push_supported_document(input_path.to_path_buf(), &mut files, progress);
            } else if input_path.is_dir() {
                if recursive {
                    for entry in WalkDir::new(input_path).into_iter().flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            push_supported_document(path.to_path_buf(), &mut files, progress);
                        }
                    }
                } else if let Ok(entries) = fs::read_dir(input_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            push_supported_document(path, &mut files, progress);
                        }
                    }
                }
            }
        }

        files
    })
}

/// Adds a path to the selected candidate list when it is a supported document.
fn push_supported_document(
    path: PathBuf,
    files: &mut Vec<DocumentCandidate>,
    progress: &mut ScanningProgress<'_>,
) {
    let Some(candidate) = DocumentCandidate::from_path(path) else {
        return;
    };

    files.push(candidate);
    progress.document_discovered();
}

/// Filters EPUB files by title and creator declarations while passing non-EPUB files through.
fn filter_epub_files(
    files: Vec<DocumentCandidate>,
    filter: &EpubFilter,
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
            let outcome = match EpubDeclarations::acquire(&path) {
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
                    progress.diagnostic(DocumentSelectionDiagnostic::UnreadableEpubMetadata {
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
            if epub_declarations.is_none() {
                match EpubDeclarations::acquire(&path) {
                    Ok(declarations) => epub_declarations = Some(declarations),
                    Err(error) => {
                        progress.diagnostic(DocumentSelectionDiagnostic::UnreadableEpubMetadata {
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
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum RecordedDocumentSelectionFact {
        Progress(DocumentSelectionProgress),
        Diagnostic(DocumentSelectionDiagnostic),
    }

    #[derive(Default)]
    struct RecordingDocumentSelectionObserver {
        progress: Vec<DocumentSelectionProgress>,
        diagnostics: Vec<DocumentSelectionDiagnostic>,
        timeline: Vec<RecordedDocumentSelectionFact>,
    }

    impl DocumentSelectionObserver for RecordingDocumentSelectionObserver {
        fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
            self.timeline
                .push(RecordedDocumentSelectionFact::Progress(progress.clone()));
            self.progress.push(progress);
        }

        fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
            self.timeline
                .push(RecordedDocumentSelectionFact::Diagnostic(
                    diagnostic.clone(),
                ));
            self.diagnostics.push(diagnostic);
        }
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-document-selection-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    /// Writes a minimal EPUB whose declarations can be read by the production adapter.
    fn write_minimal_epub(path: &Path, author: &str, title: &str) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        archive
            .start_file("mimetype", options)
            .expect("mimetype entry should start");
        archive
            .write_all(b"application/epub+zip")
            .expect("mimetype should be writable");
        archive
            .start_file("META-INF/container.xml", options)
            .expect("container entry should start");
        archive
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .expect("container should be writable");
        archive
            .start_file("OEBPS/content.opf", options)
            .expect("OPF entry should start");
        archive
            .write_all(
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:creator>{author}</dc:creator>
  </metadata>
  <manifest></manifest>
  <spine></spine>
</package>"#
                )
                .as_bytes(),
            )
            .expect("OPF should be writable");
        archive.finish().expect("test EPUB should finish");
    }

    #[test]
    fn select_documents_reports_scanning_through_its_public_interface() {
        let temp_dir = temp_test_dir("public-scan-progress");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::write(temp_dir.join("book.docx"), []).expect("test DOCX should be writable");
        fs::write(temp_dir.join("notes.txt"), []).expect("ignored file should be writable");
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&temp_dir),
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(
            observer.progress,
            vec![
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RequestedInputs,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RequestedInputs,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RequestedInputs,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                },
            ]
        );
        assert!(observer.diagnostics.is_empty());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_reports_ordered_monotonic_phase_snapshots() {
        let temp_dir = temp_test_dir("ordered-phase-snapshots");
        let epub_path = temp_dir.join("book.epub");
        let docx_path = temp_dir.join("document.docx");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        write_minimal_epub(&epub_path, "Test Author", "Magic Book");
        fs::write(docx_path, []).expect("test DOCX should be writable");
        let filter = EpubFilter {
            title: Some("magic".to_string()),
            author: None,
        };
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&temp_dir),
                recursive: false,
                output: None,
                epub_filter: &filter,
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 2);
        let scan_counts: Vec<_> = observer
            .progress
            .iter()
            .filter_map(|progress| match progress {
                DocumentSelectionProgress::Scanning { discovered, .. } => Some(*discovered),
                _ => None,
            })
            .collect();
        let filter_counts: Vec<_> = observer
            .progress
            .iter()
            .filter_map(|progress| match progress {
                DocumentSelectionProgress::FilteringEpubs { checked, .. } => Some(*checked),
                _ => None,
            })
            .collect();
        let dedup_counts: Vec<_> = observer
            .progress
            .iter()
            .filter_map(|progress| match progress {
                DocumentSelectionProgress::DeduplicatingEpubs { checked, .. } => Some(*checked),
                _ => None,
            })
            .collect();
        let scan_statuses: Vec<_> = observer
            .progress
            .iter()
            .filter_map(|progress| match progress {
                DocumentSelectionProgress::Scanning { status, .. } => Some(*status),
                _ => None,
            })
            .collect();
        let filter_statuses: Vec<_> = observer
            .progress
            .iter()
            .filter_map(|progress| match progress {
                DocumentSelectionProgress::FilteringEpubs { status, .. } => Some(*status),
                _ => None,
            })
            .collect();
        let dedup_statuses: Vec<_> = observer
            .progress
            .iter()
            .filter_map(|progress| match progress {
                DocumentSelectionProgress::DeduplicatingEpubs { status, .. } => Some(*status),
                _ => None,
            })
            .collect();
        assert_eq!(scan_counts, vec![0, 1, 2, 2]);
        assert_eq!(filter_counts, vec![0, 1, 1]);
        assert_eq!(dedup_counts, vec![0, 1, 1]);
        assert_eq!(
            scan_statuses,
            vec![
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Finished,
            ]
        );
        assert_eq!(
            filter_statuses,
            vec![
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Finished,
            ]
        );
        assert_eq!(
            dedup_statuses,
            vec![
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Running,
                DocumentSelectionPhaseStatus::Finished,
            ]
        );

        let first_filter = observer
            .progress
            .iter()
            .position(|progress| {
                matches!(progress, DocumentSelectionProgress::FilteringEpubs { .. })
            })
            .expect("filter progress should be reported");
        let last_scan = observer
            .progress
            .iter()
            .rposition(|progress| matches!(progress, DocumentSelectionProgress::Scanning { .. }))
            .expect("scan progress should be reported");
        let first_dedup = observer
            .progress
            .iter()
            .position(|progress| {
                matches!(
                    progress,
                    DocumentSelectionProgress::DeduplicatingEpubs { .. }
                )
            })
            .expect("deduplication progress should be reported");
        let last_filter = observer
            .progress
            .iter()
            .rposition(|progress| {
                matches!(progress, DocumentSelectionProgress::FilteringEpubs { .. })
            })
            .expect("filter progress should be reported");
        assert!(last_scan < first_filter);
        assert!(last_filter < first_dedup);
        assert!(observer.diagnostics.is_empty());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_reports_missing_input_as_a_structured_diagnostic() {
        let missing = temp_test_dir("missing-input").join("missing.docx");
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&missing),
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert!(selected.is_empty());
        assert_eq!(
            observer.diagnostics,
            vec![DocumentSelectionDiagnostic::MissingInput { path: missing }]
        );
    }

    #[test]
    fn select_documents_keeps_scanning_silent_when_no_inputs_are_requested() {
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &[],
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert!(selected.is_empty());
        assert!(observer.progress.is_empty());
        assert!(observer.diagnostics.is_empty());
    }

    #[test]
    fn select_documents_reports_filtering_metadata_failure_and_skips_deduplication() {
        let temp_dir = temp_test_dir("filter-metadata-failure");
        let epub_path = temp_dir.join("invalid.epub");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::write(&epub_path, b"not an epub").expect("invalid EPUB should be writable");
        let filter = EpubFilter {
            title: Some("needle".to_string()),
            author: None,
        };
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&epub_path),
                recursive: false,
                output: None,
                epub_filter: &filter,
            },
            &mut observer,
        );

        assert!(selected.is_empty());
        assert!(matches!(
            observer.diagnostics.as_slice(),
            [DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                path,
                purpose: EpubMetadataPurpose::Filtering,
                detail,
            }] if path == &epub_path && !detail.is_empty()
        ));
        assert!(observer.progress.iter().any(|progress| matches!(
            progress,
            DocumentSelectionProgress::FilteringEpubs {
                checked: 1,
                total: 1,
                matching: 0,
                status: DocumentSelectionPhaseStatus::Finished,
                ..
            }
        )));
        assert!(!observer.progress.iter().any(|progress| matches!(
            progress,
            DocumentSelectionProgress::DeduplicatingEpubs { .. }
        )));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_orders_filter_diagnostic_before_progress_advances_and_finish() {
        let temp_dir = temp_test_dir("filter-diagnostic-order");
        let invalid_epub = temp_dir.join("invalid.epub");
        let valid_epub = temp_dir.join("valid.epub");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::write(&invalid_epub, b"not an epub").expect("invalid EPUB should be writable");
        write_minimal_epub(&valid_epub, "Test Author", "Magic Book");
        let inputs = vec![invalid_epub.clone(), valid_epub];
        let filter = EpubFilter {
            title: Some("magic".to_string()),
            author: None,
        };
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: false,
                output: None,
                epub_filter: &filter,
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        let filtering_facts: Vec<_> = observer
            .timeline
            .iter()
            .filter(|fact| {
                matches!(
                    fact,
                    RecordedDocumentSelectionFact::Progress(
                        DocumentSelectionProgress::FilteringEpubs { .. }
                    ) | RecordedDocumentSelectionFact::Diagnostic(
                        DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                            purpose: EpubMetadataPurpose::Filtering,
                            ..
                        }
                    )
                )
            })
            .collect();

        assert_eq!(filtering_facts.len(), 5);
        assert!(matches!(
            filtering_facts[0],
            RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::FilteringEpubs {
                checked: 0,
                matching: 0,
                status: DocumentSelectionPhaseStatus::Running,
                ..
            })
        ));
        assert!(matches!(
            filtering_facts[1],
            RecordedDocumentSelectionFact::Diagnostic(
                DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                    path,
                    purpose: EpubMetadataPurpose::Filtering,
                    ..
                }
            ) if path == &invalid_epub
        ));
        assert!(matches!(
            filtering_facts[2],
            RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::FilteringEpubs {
                checked: 1,
                matching: 0,
                status: DocumentSelectionPhaseStatus::Running,
                ..
            })
        ));
        assert!(matches!(
            filtering_facts[3],
            RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::FilteringEpubs {
                checked: 2,
                matching: 1,
                status: DocumentSelectionPhaseStatus::Running,
                ..
            })
        ));
        assert!(matches!(
            filtering_facts[4],
            RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::FilteringEpubs {
                checked: 2,
                matching: 1,
                status: DocumentSelectionPhaseStatus::Finished,
                ..
            })
        ));

        let filter_finished = observer
            .timeline
            .iter()
            .position(|fact| {
                matches!(
                    fact,
                    RecordedDocumentSelectionFact::Progress(
                        DocumentSelectionProgress::FilteringEpubs {
                            status: DocumentSelectionPhaseStatus::Finished,
                            ..
                        }
                    )
                )
            })
            .expect("filtering should finish");
        let dedup_started = observer
            .timeline
            .iter()
            .position(|fact| {
                matches!(
                    fact,
                    RecordedDocumentSelectionFact::Progress(
                        DocumentSelectionProgress::DeduplicatingEpubs { .. }
                    )
                )
            })
            .expect("deduplication should start for the matching EPUB");
        assert!(filter_finished < dedup_started);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn resolves_output_dir_uses_global_when_set() {
        let global = Path::new("/out");
        let resolved = resolve_output_dir(Path::new("subdir/doc.docx"), Some(global));
        assert_eq!(resolved, PathBuf::from("/out"));
    }

    #[test]
    fn resolves_output_dir_beside_file_in_subdir() {
        let resolved = resolve_output_dir(Path::new("subdir/doc.docx"), None);
        assert_eq!(resolved, PathBuf::from("subdir"));
    }

    #[test]
    fn resolves_output_dir_bare_filename_defaults_to_dot() {
        let resolved = resolve_output_dir(Path::new("doc.docx"), None);
        assert_eq!(resolved, PathBuf::from("."));
    }

    #[test]
    fn resolves_output_dir_absolute_input() {
        let input = Path::new("/books/sample.epub");
        let resolved = resolve_output_dir(input, None);
        assert_eq!(resolved, Path::new("/books"));
    }

    #[test]
    fn select_documents_respects_recursive_scanning_through_its_public_interface() {
        let temp_dir = temp_test_dir("collect-recursive");
        let nested = temp_dir.join("nested");
        fs::create_dir_all(&nested).expect("nested test directory should be creatable");
        fs::write(temp_dir.join("root.docx"), []).expect("root docx should be writable");
        fs::write(nested.join("nested.epub"), []).expect("nested epub should be writable");
        fs::write(temp_dir.join("ignored.txt"), []).expect("ignored file should be writable");

        let mut observer = RecordingDocumentSelectionObserver::default();
        let non_recursive = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&temp_dir),
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );
        assert_eq!(non_recursive.len(), 1);

        let mut observer = RecordingDocumentSelectionObserver::default();
        let recursive = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&temp_dir),
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );
        assert_eq!(recursive.len(), 2);
        assert!(observer.progress.iter().any(|progress| matches!(
            progress,
            DocumentSelectionProgress::Scanning {
                scope: DocumentSelectionScanScope::RecursiveDirectories,
                ..
            }
        )));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_skips_epub_filter_progress_when_no_epubs_are_selected() {
        let temp_dir = temp_test_dir("skip-empty-filter");
        let docx = temp_dir.join("doc.docx");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::write(&docx, []).expect("test DOCX should be writable");
        let filter = EpubFilter {
            title: Some("needle".to_string()),
            author: None,
        };
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&docx),
                recursive: false,
                output: None,
                epub_filter: &filter,
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert!(!observer.progress.iter().any(|progress| matches!(
            progress,
            DocumentSelectionProgress::FilteringEpubs { .. }
                | DocumentSelectionProgress::DeduplicatingEpubs { .. }
        )));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_deduplicates_matching_readable_epub_declarations() {
        let temp_dir = temp_test_dir("declaration-dedupe");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        let first = temp_dir.join("first.epub");
        let second = temp_dir.join("second.epub");
        write_minimal_epub(&first, "Shared Creator", "Shared Title");
        write_minimal_epub(&second, "Shared Creator", "Shared Title");

        let mut observer = RecordingDocumentSelectionObserver::default();
        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &[first.clone(), second],
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), first);
        assert!(observer.progress.iter().any(|progress| {
            matches!(
                progress,
                DocumentSelectionProgress::DeduplicatingEpubs {
                    duplicates_found: 1,
                    unique_remaining: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                }
            )
        }));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn declaration_deduplication_falls_back_to_filename_when_declarations_cannot_be_read() {
        let temp_dir = temp_test_dir("dedupe-fallback");
        let first_dir = temp_dir.join("first");
        let second_dir = temp_dir.join("second");
        fs::create_dir_all(&first_dir).expect("first test directory should be creatable");
        fs::create_dir_all(&second_dir).expect("second test directory should be creatable");
        let first = first_dir.join("book.epub");
        let second = second_dir.join("book.epub");
        fs::write(&first, b"not an epub").expect("first invalid epub should be writable");
        fs::write(&second, b"not an epub").expect("second invalid epub should be writable");

        let mut observer = RecordingDocumentSelectionObserver::default();
        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &[first.clone(), second],
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), first);
        assert!(observer.progress.iter().any(|progress| {
            matches!(
                progress,
                DocumentSelectionProgress::DeduplicatingEpubs {
                    duplicates_found: 1,
                    unique_remaining: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                }
            )
        }));
        assert_eq!(
            observer
                .diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic,
                    DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                        purpose: EpubMetadataPurpose::Deduplication,
                        ..
                    }
                ))
                .count(),
            2
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_uses_declaration_derived_display_name() {
        let temp_dir = temp_test_dir("selected-epub-identity");
        let epub_path = temp_dir.join("sample.epub");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        write_minimal_epub(&epub_path, "Tester", "Magic Test");
        let mut observer = RecordingDocumentSelectionObserver::default();
        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&epub_path),
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_display_name(), "Tester - Magic Test");

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn test_format_epub_base_name_both() {
        let result = format_epub_base_name(Some("Stephen King"), Some("The Shining"), "fallback");
        assert_eq!(result, "Stephen King - The Shining");
    }

    #[test]
    fn test_format_epub_base_name_title_only() {
        let result = format_epub_base_name(None, Some("The Shining"), "fallback");
        assert_eq!(result, "The Shining");
    }

    #[test]
    fn test_format_epub_base_name_author_only() {
        let result = format_epub_base_name(Some("Stephen King"), None, "fallback");
        assert_eq!(result, "Stephen King");
    }

    #[test]
    fn test_format_epub_base_name_neither() {
        let result = format_epub_base_name(None, None, "fallback");
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_format_epub_base_name_empty_strings() {
        let result = format_epub_base_name(Some("  "), Some(""), "fallback");
        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_format_epub_base_name_sanitizes() {
        let result = format_epub_base_name(Some("Author/Name"), Some("Title:Subtitle"), "fallback");
        assert_eq!(result, "Author_Name - Title_Subtitle");
    }
}
