//! Document selection for turning requested input paths into extraction work.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::epub;

/// Sanitizes document metadata for use as an output filename.
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

/// Filter criteria for selecting EPUB files by metadata.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EpubFilter {
    /// Case-insensitive title substring required for an EPUB to be selected.
    pub title: Option<String>,
    /// Case-insensitive author substring required for an EPUB to be selected.
    pub author: Option<String>,
}

impl EpubFilter {
    /// Returns true if no EPUB metadata filter criteria are set.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.author.is_none()
    }
}

/// Whether a Document selection phase is still running or has finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentSelectionPhaseStatus {
    /// The phase may emit later snapshots with greater progress.
    Running,
    /// The phase has emitted its final snapshot.
    Finished,
}

/// Scope of the scanning phase reported as Document selection progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentSelectionScanScope {
    /// Requested paths are inspected without recursive directory traversal.
    RequestedInputs,
    /// At least one requested directory is traversed recursively.
    RecursiveDirectories,
}

/// Immutable current-state snapshot for one live Document selection phase.
///
/// Phases are reported in scanning, optional filtering, then optional
/// deduplication order. A phase with no work is silent; every phase that runs
/// emits an initial running snapshot, monotonically advancing running snapshots,
/// and exactly one finished snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentSelectionProgress {
    /// Current document scanning state.
    Scanning {
        scope: DocumentSelectionScanScope,
        discovered: usize,
        status: DocumentSelectionPhaseStatus,
    },
    /// Current EPUB metadata filtering state.
    FilteringEpubs {
        filter: EpubFilter,
        checked: usize,
        total: usize,
        matching: usize,
        status: DocumentSelectionPhaseStatus,
    },
    /// Current EPUB metadata deduplication state.
    DeduplicatingEpubs {
        checked: usize,
        total: usize,
        duplicates_found: usize,
        unique_remaining: usize,
        status: DocumentSelectionPhaseStatus,
    },
}

/// Document selection use that could not read EPUB metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubMetadataPurpose {
    /// Metadata was needed to apply a requested EPUB filter.
    Filtering,
    /// Metadata was needed to deduplicate EPUBs before filename fallback.
    Deduplication,
}

/// Structured non-fatal fact produced while selecting documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentSelectionDiagnostic {
    /// A requested input path does not exist and was skipped.
    MissingInput { path: PathBuf },
    /// EPUB metadata could not be read for the stated selection purpose.
    UnreadableEpubMetadata {
        path: PathBuf,
        purpose: EpubMetadataPurpose,
        detail: String,
    },
}

/// Receives live Document selection progress and diagnostics.
///
/// The observer is informational: callbacks cannot cancel selection or alter
/// which documents are returned. Progress snapshots carry structured selection
/// facts, while diagnostics carry non-fatal facts without terminal wording.
pub trait DocumentSelectionObserver {
    /// Handles one immutable phase snapshot.
    fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress);

    /// Handles one structured non-fatal selection diagnostic.
    fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic);
}

/// Options used to select documents for one extraction run.
pub struct DocumentSelectionOptions<'a> {
    /// Input files or directories to scan.
    pub inputs: &'a [PathBuf],
    /// Whether directory inputs should be scanned recursively.
    pub recursive: bool,
    /// Optional output directory shared by every selected document.
    pub output: Option<&'a Path>,
    /// Whether EPUB display names should use metadata-derived base names.
    pub cover_only: bool,
    /// EPUB metadata filter criteria.
    pub epub_filter: &'a EpubFilter,
}

/// A document selected for extraction with document-level facts resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectedDocument {
    /// DOCX document selected for extraction.
    Docx {
        /// Source file path.
        path: PathBuf,
        /// Directory where extracted images should be written.
        output_dir: PathBuf,
        /// Output filename stem to use for extracted images.
        base_name: String,
        /// Progress display name for this document.
        display_name: String,
    },
    /// EPUB document selected for extraction.
    Epub {
        /// Source file path.
        path: PathBuf,
        /// Directory where extracted images should be written.
        output_dir: PathBuf,
        /// Output filename stem to use for extracted images.
        base_name: String,
        /// Progress display name for this document.
        display_name: String,
    },
}

impl SelectedDocument {
    /// Returns the source path for the selected document.
    pub fn path(&self) -> &Path {
        match self {
            SelectedDocument::Docx { path, .. } | SelectedDocument::Epub { path, .. } => path,
        }
    }

    /// Returns the output directory for the selected document.
    pub fn output_dir(&self) -> &Path {
        match self {
            SelectedDocument::Docx { output_dir, .. }
            | SelectedDocument::Epub { output_dir, .. } => output_dir,
        }
    }

    /// Returns the output filename stem for the selected document.
    pub fn base_name(&self) -> &str {
        match self {
            SelectedDocument::Docx { base_name, .. } | SelectedDocument::Epub { base_name, .. } => {
                base_name
            }
        }
    }

    /// Returns the progress display name for the selected document.
    pub fn display_name(&self) -> &str {
        match self {
            SelectedDocument::Docx { display_name, .. }
            | SelectedDocument::Epub { display_name, .. } => display_name,
        }
    }

    /// Returns whether this selected document is a DOCX file.
    pub fn is_docx(&self) -> bool {
        matches!(self, SelectedDocument::Docx { .. })
    }
}

/// Supported document types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentType {
    Docx,
    Epub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EpubMetadata {
    author: Option<String>,
    title: Option<String>,
}

impl EpubMetadata {
    /// Returns true when at least one metadata field is present.
    fn has_any_value(&self) -> bool {
        self.author.is_some() || self.title.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentCandidate {
    path: PathBuf,
    document_type: DocumentType,
    epub_metadata: Option<EpubMetadata>,
}

impl DocumentCandidate {
    /// Creates a document candidate without preloaded EPUB metadata.
    fn new(path: PathBuf, document_type: DocumentType) -> Self {
        Self {
            path,
            document_type,
            epub_metadata: None,
        }
    }
}

/// Selects documents for extraction while reporting live progress snapshots and diagnostics.
///
/// Selection owns document discovery, EPUB metadata filtering, EPUB dedupe,
/// display identity, and per-document output placement. Returned documents are
/// already eligible for extraction; adapters should not re-check selection
/// filters. Missing inputs and unreadable EPUB metadata are reported as
/// structured, non-fatal diagnostics through the informational observer.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    observer: &mut impl DocumentSelectionObserver,
) -> Vec<SelectedDocument> {
    for input_path in options.inputs {
        if !input_path.exists() {
            observer.on_document_selection_diagnostic(DocumentSelectionDiagnostic::MissingInput {
                path: input_path.clone(),
            });
        }
    }

    let candidates = collect_document_files(options.inputs, options.recursive, observer);
    let filtered = if !options.epub_filter.is_empty() {
        filter_epub_files(candidates, options.epub_filter, observer)
    } else {
        candidates
    };
    let deduplicated = deduplicate_by_metadata(filtered, observer);

    deduplicated
        .into_iter()
        .map(|candidate| {
            selected_document_from_candidate(candidate, options.output, options.cover_only)
        })
        .collect()
}

/// Determines the document type based on file extension.
fn get_document_type(path: &Path) -> Option<DocumentType> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
        .and_then(|ext| match ext.as_str() {
            "docx" => Some(DocumentType::Docx),
            "epub" => Some(DocumentType::Epub),
            _ => None,
        })
}

/// Checks if a path is a supported document type.
fn is_supported_document(path: &Path) -> bool {
    get_document_type(path).is_some()
}

/// Checks if a candidate is an EPUB file.
fn is_epub(candidate: &DocumentCandidate) -> bool {
    candidate.document_type == DocumentType::Epub
}

/// Collects all document files from the input paths.
fn collect_document_files(
    inputs: &[PathBuf],
    recursive: bool,
    observer: &mut impl DocumentSelectionObserver,
) -> Vec<DocumentCandidate> {
    let mut files = Vec::new();

    // A phase with no requested work stays silent by the observer lifecycle contract.
    if inputs.is_empty() {
        return files;
    }

    let scope = if recursive && inputs.iter().any(|path| path.is_dir()) {
        DocumentSelectionScanScope::RecursiveDirectories
    } else {
        DocumentSelectionScanScope::RequestedInputs
    };
    observer.on_document_selection_progress(DocumentSelectionProgress::Scanning {
        scope,
        discovered: 0,
        status: DocumentSelectionPhaseStatus::Running,
    });

    for input_path in inputs {
        if !input_path.exists() {
            continue;
        }

        if input_path.is_file() {
            push_supported_document(input_path.to_path_buf(), &mut files, scope, observer);
        } else if input_path.is_dir() {
            if recursive {
                for entry in WalkDir::new(input_path).into_iter().flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        push_supported_document(path.to_path_buf(), &mut files, scope, observer);
                    }
                }
            } else if let Ok(entries) = fs::read_dir(input_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        push_supported_document(path, &mut files, scope, observer);
                    }
                }
            }
        }
    }

    observer.on_document_selection_progress(DocumentSelectionProgress::Scanning {
        scope,
        discovered: files.len(),
        status: DocumentSelectionPhaseStatus::Finished,
    });
    files
}

/// Adds a path to the selected candidate list when it is a supported document.
fn push_supported_document(
    path: PathBuf,
    files: &mut Vec<DocumentCandidate>,
    scope: DocumentSelectionScanScope,
    observer: &mut impl DocumentSelectionObserver,
) {
    if !is_supported_document(&path) {
        return;
    }

    let document_type = get_document_type(&path).expect("supported document type should exist");
    files.push(DocumentCandidate::new(path, document_type));
    observer.on_document_selection_progress(DocumentSelectionProgress::Scanning {
        scope,
        discovered: files.len(),
        status: DocumentSelectionPhaseStatus::Running,
    });
}

/// Filters EPUB files by metadata while passing non-EPUB files through.
fn filter_epub_files(
    files: Vec<DocumentCandidate>,
    filter: &EpubFilter,
    observer: &mut impl DocumentSelectionObserver,
) -> Vec<DocumentCandidate> {
    // Separate EPUB files from other document types.
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(is_epub);

    if epub_files.is_empty() {
        return other_files;
    }

    observer.on_document_selection_progress(DocumentSelectionProgress::FilteringEpubs {
        filter: filter.clone(),
        checked: 0,
        total: epub_files.len(),
        matching: 0,
        status: DocumentSelectionPhaseStatus::Running,
    });

    let mut matching_epubs = Vec::new();
    let total = epub_files.len();
    for (index, mut candidate) in epub_files.into_iter().enumerate() {
        match read_epub_metadata(&candidate.path) {
            Ok(metadata) if matches_filter(&metadata, filter) => {
                candidate.epub_metadata = Some(metadata);
                matching_epubs.push(candidate);
            }
            Ok(_) => {} // File doesn't match filter, skip.
            Err(e) => {
                // Filtering cannot accept an EPUB whose requested metadata is unreadable.
                observer.on_document_selection_diagnostic(
                    DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                        path: candidate.path,
                        purpose: EpubMetadataPurpose::Filtering,
                        detail: e.to_string(),
                    },
                );
            }
        }

        observer.on_document_selection_progress(DocumentSelectionProgress::FilteringEpubs {
            filter: filter.clone(),
            checked: index + 1,
            total,
            matching: matching_epubs.len(),
            status: DocumentSelectionPhaseStatus::Running,
        });
    }

    observer.on_document_selection_progress(DocumentSelectionProgress::FilteringEpubs {
        filter: filter.clone(),
        checked: total,
        total,
        matching: matching_epubs.len(),
        status: DocumentSelectionPhaseStatus::Finished,
    });

    // Combine matching EPUBs with other document types.
    let mut result = matching_epubs;
    result.extend(other_files);
    result
}

/// Deduplicates EPUB files based on their metadata (author + title).
///
/// Keeps the first occurrence of each unique (author, title) combination.
/// Non-EPUB files are passed through unchanged. EPUBs without metadata are
/// deduplicated by filename.
fn deduplicate_by_metadata(
    files: Vec<DocumentCandidate>,
    observer: &mut impl DocumentSelectionObserver,
) -> Vec<DocumentCandidate> {
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(is_epub);

    if epub_files.is_empty() {
        return other_files;
    }

    observer.on_document_selection_progress(DocumentSelectionProgress::DeduplicatingEpubs {
        checked: 0,
        total: epub_files.len(),
        duplicates_found: 0,
        unique_remaining: 0,
        status: DocumentSelectionPhaseStatus::Running,
    });

    // Use a HashMap to track seen (author, title) combinations.
    // Key: (lowercase author, lowercase title) for case-insensitive deduplication.
    let mut seen: HashMap<(String, String), PathBuf> = HashMap::new();
    let mut unique_epubs = Vec::new();
    let mut duplicates_found = 0usize;
    let total = epub_files.len();

    for (index, mut candidate) in epub_files.into_iter().enumerate() {
        if candidate.epub_metadata.is_none() {
            match read_epub_metadata(&candidate.path) {
                Ok(metadata) => candidate.epub_metadata = Some(metadata),
                Err(error) => observer.on_document_selection_diagnostic(
                    DocumentSelectionDiagnostic::UnreadableEpubMetadata {
                        path: candidate.path.clone(),
                        purpose: EpubMetadataPurpose::Deduplication,
                        detail: error.to_string(),
                    },
                ),
            }
        }

        let key = match candidate.epub_metadata.as_ref() {
            Some(metadata) if metadata.has_any_value() => metadata.dedupe_key(),
            _ => filename_dedupe_key(&candidate.path),
        };

        // Only add if we haven't seen this combination before.
        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
            e.insert(candidate.path.clone());
            unique_epubs.push(candidate);
        } else {
            duplicates_found += 1;
        }

        observer.on_document_selection_progress(DocumentSelectionProgress::DeduplicatingEpubs {
            checked: index + 1,
            total,
            duplicates_found,
            unique_remaining: unique_epubs.len(),
            status: DocumentSelectionPhaseStatus::Running,
        });
    }

    observer.on_document_selection_progress(DocumentSelectionProgress::DeduplicatingEpubs {
        checked: total,
        total,
        duplicates_found,
        unique_remaining: unique_epubs.len(),
        status: DocumentSelectionPhaseStatus::Finished,
    });

    // Combine unique EPUBs with other document types.
    let mut result = unique_epubs;
    result.extend(other_files);
    result
}

impl EpubMetadata {
    /// Builds the case-insensitive dedupe key for EPUB metadata.
    fn dedupe_key(&self) -> (String, String) {
        let author_key = self
            .author
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default();
        let title_key = self
            .title
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default();
        (author_key, title_key)
    }
}

/// Builds a filename fallback dedupe key for EPUBs without usable metadata.
fn filename_dedupe_key(path: &Path) -> (String, String) {
    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    (String::new(), filename)
}

/// Reads EPUB metadata through the EPUB adapter.
fn read_epub_metadata(path: &Path) -> anyhow::Result<EpubMetadata> {
    let (author, title) = epub::get_metadata(path)?;
    Ok(EpubMetadata { author, title })
}

/// Checks if EPUB metadata matches the filter using case-insensitive substring match.
fn matches_filter(metadata: &EpubMetadata, filter: &EpubFilter) -> bool {
    let title_matches = filter.title.as_ref().is_none_or(|f| {
        metadata
            .title
            .as_deref()
            .is_some_and(|t| t.to_lowercase().contains(&f.to_lowercase()))
    });

    let author_matches = filter.author.as_ref().is_none_or(|f| {
        metadata
            .author
            .as_deref()
            .is_some_and(|a| a.to_lowercase().contains(&f.to_lowercase()))
    });

    title_matches && author_matches
}

/// Builds one selected document from a discovered and filtered candidate.
fn selected_document_from_candidate(
    candidate: DocumentCandidate,
    global_output: Option<&Path>,
    cover_only: bool,
) -> SelectedDocument {
    let output_dir = resolve_output_dir(&candidate.path, global_output);
    let fallback_base_name = fallback_base_name(&candidate.path);
    let display_name = fallback_display_name(&candidate.path);

    match candidate.document_type {
        DocumentType::Docx => SelectedDocument::Docx {
            path: candidate.path,
            output_dir,
            base_name: fallback_base_name,
            display_name,
        },
        DocumentType::Epub => {
            let base_name = format_epub_base_name(
                candidate
                    .epub_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.author.as_deref()),
                candidate
                    .epub_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.title.as_deref()),
                &fallback_base_name,
            );
            let display_name = if cover_only {
                base_name.clone()
            } else {
                display_name
            };

            SelectedDocument::Epub {
                path: candidate.path,
                output_dir,
                base_name,
                display_name,
            }
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

/// Formats a filename based on EPUB metadata (author and title).
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

    #[derive(Default)]
    struct RecordingDocumentSelectionObserver {
        progress: Vec<DocumentSelectionProgress>,
        diagnostics: Vec<DocumentSelectionDiagnostic>,
    }

    impl DocumentSelectionObserver for RecordingDocumentSelectionObserver {
        fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
            self.progress.push(progress);
        }

        fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
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

    /// Writes a minimal EPUB whose metadata can be read by the production adapter.
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
                cover_only: false,
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
                cover_only: false,
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
                cover_only: false,
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
                cover_only: false,
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
                cover_only: false,
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
                cover_only: false,
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
                cover_only: false,
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
                cover_only: false,
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
    fn deduplicate_by_metadata_falls_back_to_filename_when_metadata_cannot_be_read() {
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
                cover_only: false,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].path(), first);
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
    fn selected_epub_carries_base_name_and_cover_display_name() {
        let selected = selected_document_from_candidate(
            DocumentCandidate {
                path: PathBuf::from("sample.epub"),
                document_type: DocumentType::Epub,
                epub_metadata: Some(EpubMetadata {
                    author: Some("Tester".to_string()),
                    title: Some("Magic Test".to_string()),
                }),
            },
            None,
            true,
        );

        assert_eq!(selected.base_name(), "Tester - Magic Test");
        assert_eq!(selected.display_name(), "Tester - Magic Test");
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
