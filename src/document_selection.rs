//! Document selection for turning requested input paths into extraction work.

mod discovery;
mod progress;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::epub_declarations::EpubDeclarations;

#[cfg(test)]
use std::fs;

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
/// filters. Missing inputs, requested-root inspection failures, and unreadable
/// EPUB declarations are reported as structured, non-fatal diagnostics through
/// the informational observer.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    observer: &mut impl DocumentSelectionObserver,
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

/// Checks if a candidate is an EPUB file.
fn is_epub(candidate: &DocumentCandidate) -> bool {
    matches!(candidate, DocumentCandidate::Epub { .. })
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

    struct RemoveDirectoryOnScanStartObserver {
        directory: Option<PathBuf>,
        recording: RecordingDocumentSelectionObserver,
    }

    impl RemoveDirectoryOnScanStartObserver {
        /// Creates an observer that removes one empty directory after root classification.
        fn new(directory: PathBuf) -> Self {
            Self {
                directory: Some(directory),
                recording: RecordingDocumentSelectionObserver::default(),
            }
        }
    }

    impl DocumentSelectionObserver for RemoveDirectoryOnScanStartObserver {
        /// Removes the directory at the initial scan snapshot, then records the snapshot.
        fn on_document_selection_progress(&mut self, progress: DocumentSelectionProgress) {
            if matches!(
                progress,
                DocumentSelectionProgress::Scanning {
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }
            ) && let Some(directory) = self.directory.take()
            {
                fs::remove_dir(directory)
                    .expect("classified directory should be removable before opening");
            }
            self.recording.on_document_selection_progress(progress);
        }

        /// Records diagnostics emitted after the induced real-filesystem open failure.
        fn on_document_selection_diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
            self.recording.on_document_selection_diagnostic(diagnostic);
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

    /// Creates a directory link used to exercise the platform filesystem through selection.
    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link)
            .expect("test directory symlink should be creatable");
    }

    /// Creates a directory link without requiring Windows symbolic-link privileges.
    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return;
        }

        let output = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("Windows junction command should run");
        assert!(
            output.status.success(),
            "test directory link should be creatable: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Removes a directory link without following it into its target.
    #[cfg(unix)]
    fn remove_directory_link(link: &Path) {
        fs::remove_file(link).expect("test directory symlink should be removable");
    }

    /// Removes a Windows directory symlink or junction without following it.
    #[cfg(windows)]
    fn remove_directory_link(link: &Path) {
        fs::remove_dir(link).expect("test directory link should be removable");
    }

    /// Creates a file symlink used to preserve nested supported-file eligibility.
    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).expect("test file symlink should be creatable");
        true
    }

    /// Attempts to create a genuine Windows file symlink when the host permits it.
    #[cfg(windows)]
    fn create_file_symlink(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    /// Removes a file symlink without removing its target.
    fn remove_file_symlink(link: &Path) {
        fs::remove_file(link).expect("test file symlink should be removable");
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
    fn select_documents_reports_broken_requested_link_and_continues_to_supported_sibling() {
        let temp_dir = temp_test_dir("broken-requested-link");
        let removed_target = temp_dir.join("removed-target");
        let broken_link = temp_dir.join("broken-link");
        let unsupported_sibling = temp_dir.join("notes.txt");
        let readable_sibling = temp_dir.join("readable.docx");
        fs::create_dir_all(&removed_target).expect("link target should be creatable");
        create_directory_link(&removed_target, &broken_link);
        fs::remove_dir(&removed_target).expect("link target should be removable");
        fs::write(&unsupported_sibling, []).expect("unsupported sibling should be writable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        assert!(fs::symlink_metadata(&broken_link).is_ok());
        assert!(fs::metadata(&broken_link).is_err());
        let inputs = vec![
            broken_link.clone(),
            unsupported_sibling,
            readable_sibling.clone(),
        ];
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                }),
            ] if path == &broken_link && !detail.is_empty()
        ));
        assert!(!observer.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            DocumentSelectionDiagnostic::MissingInput { path } if path == &broken_link
        )));

        remove_directory_link(&broken_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies that a nested inspection failure stays ordered inside active scanning.
    #[test]
    fn select_documents_reports_broken_nested_link_before_later_supported_input() {
        let temp_dir = temp_test_dir("broken-nested-link");
        let requested_directory = temp_dir.join("requested");
        let removed_target = temp_dir.join("removed-target");
        let broken_link = requested_directory.join("broken-link");
        let unsupported_entry = requested_directory.join("notes.txt");
        let readable_sibling = temp_dir.join("readable.docx");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&removed_target).expect("link target should be creatable");
        create_directory_link(&removed_target, &broken_link);
        fs::remove_dir(&removed_target).expect("link target should be removable");
        fs::write(unsupported_entry, []).expect("unsupported entry should be writable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        let inputs = vec![requested_directory, readable_sibling.clone()];
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                }),
            ] if path == &broken_link && !detail.is_empty()
        ));

        remove_directory_link(&broken_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies recursive inspection diagnoses one broken nested link at encounter position.
    #[test]
    fn select_documents_reports_broken_nested_link_once_during_recursive_scanning() {
        let temp_dir = temp_test_dir("recursive-broken-nested-link");
        let requested_directory = temp_dir.join("requested");
        let removed_target = temp_dir.join("removed-target");
        let broken_link = requested_directory.join("broken-link");
        let readable_sibling = temp_dir.join("readable.docx");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&removed_target).expect("link target should be creatable");
        create_directory_link(&removed_target, &broken_link);
        fs::remove_dir(&removed_target).expect("link target should be removable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        let inputs = vec![requested_directory, readable_sibling.clone()];
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                }),
            ] if path == &broken_link && !detail.is_empty()
        ));

        remove_directory_link(&broken_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies an all-failed recursive scan still finishes with an explicit zero count.
    #[test]
    fn select_documents_finishes_recursive_scanning_at_zero_after_failure() {
        let temp_dir = temp_test_dir("recursive-zero-after-failure");
        let requested_directory = temp_dir.join("requested");
        let removed_target = temp_dir.join("removed-target");
        let broken_link = requested_directory.join("broken-link");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&removed_target).expect("link target should be creatable");
        create_directory_link(&removed_target, &broken_link);
        fs::remove_dir(&removed_target).expect("link target should be removable");
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&requested_directory),
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert!(selected.is_empty());
        assert!(matches!(
            observer.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Finished,
                }),
            ] if path == &broken_link && !detail.is_empty()
        ));

        remove_directory_link(&broken_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies requested-root fallback and continuation when opening a directory fails.
    #[test]
    fn select_documents_reports_directory_open_failure_and_continues_to_supported_input() {
        let temp_dir = temp_test_dir("directory-open-failure");
        let removed_directory = temp_dir.join("removed-before-open");
        let readable_sibling = temp_dir.join("readable.docx");
        fs::create_dir_all(&removed_directory).expect("requested directory should be creatable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        let inputs = vec![removed_directory.clone(), readable_sibling.clone()];
        let mut observer = RemoveDirectoryOnScanStartObserver::new(removed_directory.clone());

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.recording.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                }),
            ] if path == &removed_directory && !detail.is_empty()
        ));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies recursive traversal reports a vanished root and continues to a later input.
    #[test]
    fn select_documents_reports_recursive_root_traversal_failure_and_continues() {
        let temp_dir = temp_test_dir("recursive-root-traversal-failure");
        let removed_directory = temp_dir.join("removed-before-walk");
        let readable_sibling = temp_dir.join("readable.docx");
        fs::create_dir_all(&removed_directory).expect("requested directory should be creatable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        let inputs = vec![removed_directory.clone(), readable_sibling.clone()];
        let mut observer = RemoveDirectoryOnScanStartObserver::new(removed_directory.clone());

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.recording.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                }),
            ] if path == &removed_directory && !detail.is_empty()
        ));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies distinct recursive failures are each reported once in encounter order.
    #[test]
    fn select_documents_orders_distinct_recursive_failures_before_later_progress() {
        let temp_dir = temp_test_dir("ordered-recursive-failures");
        let first_directory = temp_dir.join("first");
        let removed_target = temp_dir.join("removed-target");
        let broken_link = first_directory.join("broken-link");
        let removed_directory = temp_dir.join("second-removed-before-walk");
        let readable_sibling = temp_dir.join("readable.docx");
        fs::create_dir_all(&first_directory).expect("first directory should be creatable");
        fs::create_dir_all(&removed_target).expect("link target should be creatable");
        fs::create_dir_all(&removed_directory).expect("second directory should be creatable");
        create_directory_link(&removed_target, &broken_link);
        fs::remove_dir(&removed_target).expect("link target should be removable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        let inputs = vec![
            first_directory,
            removed_directory.clone(),
            readable_sibling.clone(),
        ];
        let mut observer = RemoveDirectoryOnScanStartObserver::new(removed_directory.clone());

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: &inputs,
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.recording.timeline.as_slice(),
            [
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed {
                        path: first_path,
                        detail: first_detail,
                    }
                ),
                RecordedDocumentSelectionFact::Diagnostic(
                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed {
                        path: second_path,
                        detail: second_detail,
                    }
                ),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                }),
                RecordedDocumentSelectionFact::Progress(DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                }),
            ] if first_path == &broken_link
                && !first_detail.is_empty()
                && second_path == &removed_directory
                && !second_detail.is_empty()
        ));

        remove_directory_link(&broken_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies a broken recursive entry does not suppress a readable sibling candidate.
    #[test]
    fn select_documents_keeps_readable_nested_sibling_after_recursive_failure() {
        let temp_dir = temp_test_dir("recursive-readable-nested-sibling");
        let requested_directory = temp_dir.join("requested");
        let removed_target = temp_dir.join("removed-target");
        let broken_link = requested_directory.join("broken-link");
        let readable_sibling = requested_directory.join("readable.docx");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&removed_target).expect("link target should be creatable");
        create_directory_link(&removed_target, &broken_link);
        fs::remove_dir(&removed_target).expect("link target should be removable");
        fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&requested_directory),
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), readable_sibling);
        assert!(matches!(
            observer.diagnostics.as_slice(),
            [DocumentSelectionDiagnostic::DocumentDiscoveryFailed { path, detail }]
                if path == &broken_link && !detail.is_empty()
        ));
        assert!(matches!(
            observer.progress.last(),
            Some(DocumentSelectionProgress::Scanning {
                scope: DocumentSelectionScanScope::RecursiveDirectories,
                discovered: 1,
                status: DocumentSelectionPhaseStatus::Finished,
            })
        ));

        remove_directory_link(&broken_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies that a supported nested file link remains eligible without recursive scanning.
    #[test]
    fn select_documents_keeps_nested_supported_file_link_eligible() {
        let temp_dir = temp_test_dir("nested-supported-file-link");
        let requested_directory = temp_dir.join("requested");
        let target_directory = temp_dir.join("targets");
        let target = target_directory.join("target.docx");
        let linked_document = requested_directory.join("linked.docx");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&target_directory).expect("target directory should be creatable");
        fs::write(&target, []).expect("linked DOCX target should be writable");
        if !create_file_symlink(&target, &linked_document) {
            eprintln!("skipping file-symlink eligibility: Windows denied symlink creation");
            fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
            return;
        }
        assert!(
            fs::symlink_metadata(&linked_document)
                .expect("linked document should have link metadata")
                .file_type()
                .is_symlink(),
            "fixture must be a genuine file symlink"
        );
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&requested_directory),
                recursive: false,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), linked_document);
        assert!(observer.diagnostics.is_empty());
        assert!(matches!(
            observer.progress.as_slice(),
            [
                DocumentSelectionProgress::Scanning {
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                },
                DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                    ..
                },
                DocumentSelectionProgress::Scanning {
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                    ..
                },
            ]
        ));

        remove_file_symlink(&linked_document);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies recursive scanning still admits a nested link to a supported regular file.
    #[test]
    fn select_documents_keeps_nested_supported_file_link_eligible_when_recursive() {
        let temp_dir = temp_test_dir("recursive-nested-supported-file-link");
        let requested_directory = temp_dir.join("requested");
        let target_directory = temp_dir.join("targets");
        let target = target_directory.join("target.docx");
        let linked_document = requested_directory.join("linked.docx");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&target_directory).expect("target directory should be creatable");
        fs::write(&target, []).expect("linked DOCX target should be writable");
        if !create_file_symlink(&target, &linked_document) {
            eprintln!("skipping file-symlink eligibility: Windows denied symlink creation");
            fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
            return;
        }
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&requested_directory),
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), linked_document);
        assert!(observer.diagnostics.is_empty());
        assert!(matches!(
            observer.progress.as_slice(),
            [
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                },
            ]
        ));

        remove_file_symlink(&linked_document);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn select_documents_follows_requested_directory_link_during_recursive_scanning() {
        let temp_dir = temp_test_dir("requested-directory-link");
        let target = temp_dir.join("target");
        let requested_link = temp_dir.join("requested-link");
        let linked_document = requested_link.join("linked.docx");
        fs::create_dir_all(&target).expect("link target should be creatable");
        fs::write(target.join("linked.docx"), []).expect("linked DOCX should be writable");
        create_directory_link(&target, &requested_link);
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&requested_link),
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_path(), linked_document);
        assert!(observer.diagnostics.is_empty());
        assert!(matches!(
            observer.progress.as_slice(),
            [
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 1,
                    status: DocumentSelectionPhaseStatus::Finished,
                },
            ]
        ));

        remove_directory_link(&requested_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    /// Verifies recursive scanning does not widen scope through a nested directory link.
    #[test]
    fn select_documents_does_not_follow_nested_directory_link_when_recursive() {
        let temp_dir = temp_test_dir("recursive-nested-directory-link");
        let requested_directory = temp_dir.join("requested");
        let target_directory = temp_dir.join("target");
        let nested_link = requested_directory.join("nested-link");
        fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
        fs::create_dir_all(&target_directory).expect("link target should be creatable");
        fs::write(target_directory.join("outside.docx"), [])
            .expect("linked-directory DOCX should be writable");
        create_directory_link(&target_directory, &nested_link);
        let mut observer = RecordingDocumentSelectionObserver::default();

        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&requested_directory),
                recursive: true,
                output: None,
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );

        assert!(selected.is_empty());
        assert!(observer.diagnostics.is_empty());
        assert_eq!(
            observer.progress,
            vec![
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                },
                DocumentSelectionProgress::Scanning {
                    scope: DocumentSelectionScanScope::RecursiveDirectories,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Finished,
                },
            ]
        );

        remove_directory_link(&nested_link);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
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
