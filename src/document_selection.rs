//! Document selection for turning requested input paths into extraction work.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::epub;
use crate::extraction_run::{RunEvent, RunObserver};

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

    /// Returns a human-readable description of the filter for progress events.
    pub fn description(&self) -> String {
        match (&self.author, &self.title) {
            (Some(author), Some(title)) => format!("author '{}' and title '{}'", author, title),
            (Some(author), None) => format!("author '{}'", author),
            (None, Some(title)) => format!("title '{}'", title),
            (None, None) => String::new(),
        }
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

/// Selects documents for extraction and emits progress events for selection phases.
///
/// Selection owns document discovery, EPUB metadata filtering, EPUB dedupe,
/// display identity, and per-document output placement. Returned documents are
/// already eligible for extraction; adapters should not re-check selection
/// filters.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    observer: &mut impl RunObserver,
) -> Vec<SelectedDocument> {
    for input_path in options.inputs {
        if !input_path.exists() {
            observer.on_event(RunEvent::InputWarning {
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
    observer: &mut impl RunObserver,
) -> Vec<DocumentCandidate> {
    let mut files = Vec::new();

    // Use spinner for recursive directory scanning since we don't know total count upfront.
    let use_spinner = recursive && inputs.iter().any(|p| p.is_dir());
    observer.on_event(RunEvent::ScanStarted { use_spinner });

    for input_path in inputs {
        if !input_path.exists() {
            continue;
        }

        if input_path.is_file() {
            push_supported_document(input_path.to_path_buf(), &mut files, observer);
        } else if input_path.is_dir() {
            if recursive {
                for entry in WalkDir::new(input_path).into_iter().flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        push_supported_document(path.to_path_buf(), &mut files, observer);
                    }
                }
            } else if let Ok(entries) = fs::read_dir(input_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        push_supported_document(path, &mut files, observer);
                    }
                }
            }
        }
    }

    observer.on_event(RunEvent::ScanFinished { count: files.len() });
    files
}

/// Adds a path to the selected candidate list when it is a supported document.
fn push_supported_document(
    path: PathBuf,
    files: &mut Vec<DocumentCandidate>,
    observer: &mut impl RunObserver,
) {
    if !is_supported_document(&path) {
        return;
    }

    let document_type = get_document_type(&path).expect("supported document type should exist");
    files.push(DocumentCandidate::new(path, document_type));
    observer.on_event(RunEvent::DocumentDiscovered { count: files.len() });
}

/// Filters EPUB files by metadata while passing non-EPUB files through.
fn filter_epub_files(
    files: Vec<DocumentCandidate>,
    filter: &EpubFilter,
    observer: &mut impl RunObserver,
) -> Vec<DocumentCandidate> {
    // Separate EPUB files from other document types.
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(is_epub);

    if epub_files.is_empty() {
        return other_files;
    }

    observer.on_event(RunEvent::EpubFilterStarted {
        description: filter.description(),
        total: epub_files.len(),
    });

    let mut matching_epubs = Vec::new();
    for mut candidate in epub_files {
        observer.on_event(RunEvent::EpubFilterAdvanced);
        match read_epub_metadata(&candidate.path) {
            Ok(metadata) if matches_filter(&metadata, filter) => {
                candidate.epub_metadata = Some(metadata);
                matching_epubs.push(candidate);
            }
            Ok(_) => {} // File doesn't match filter, skip.
            Err(e) => {
                // Log error but continue searching.
                observer.on_event(RunEvent::EpubFilterWarning {
                    path: candidate.path,
                    message: e.to_string(),
                });
            }
        }
    }

    observer.on_event(RunEvent::EpubFilterFinished {
        matching: matching_epubs.len(),
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
    observer: &mut impl RunObserver,
) -> Vec<DocumentCandidate> {
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(is_epub);

    if epub_files.is_empty() {
        return other_files;
    }

    observer.on_event(RunEvent::EpubDedupStarted {
        total: epub_files.len(),
    });

    // Use a HashMap to track seen (author, title) combinations.
    // Key: (lowercase author, lowercase title) for case-insensitive deduplication.
    let mut seen: HashMap<(String, String), PathBuf> = HashMap::new();
    let mut unique_epubs = Vec::new();
    let mut duplicates_found = 0usize;

    for mut candidate in epub_files {
        observer.on_event(RunEvent::EpubDedupAdvanced);

        if candidate.epub_metadata.is_none() {
            candidate.epub_metadata = read_epub_metadata(&candidate.path).ok();
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
    }

    observer.on_event(RunEvent::EpubDedupFinished {
        duplicates_found,
        unique_remaining: unique_epubs.len(),
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct CollectingObserver {
        events: Vec<RunEvent>,
    }

    impl RunObserver for CollectingObserver {
        fn on_event(&mut self, event: RunEvent) {
            self.events.push(event);
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

    fn candidate(path: &str, document_type: DocumentType) -> DocumentCandidate {
        DocumentCandidate::new(PathBuf::from(path), document_type)
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
    fn collect_document_files_respects_recursive_flag() {
        let temp_dir = temp_test_dir("collect-recursive");
        let nested = temp_dir.join("nested");
        fs::create_dir_all(&nested).expect("nested test directory should be creatable");
        fs::write(temp_dir.join("root.docx"), []).expect("root docx should be writable");
        fs::write(nested.join("nested.epub"), []).expect("nested epub should be writable");
        fs::write(temp_dir.join("ignored.txt"), []).expect("ignored file should be writable");

        let mut observer = CollectingObserver::default();
        let non_recursive =
            collect_document_files(std::slice::from_ref(&temp_dir), false, &mut observer);
        assert_eq!(non_recursive.len(), 1);

        let mut observer = CollectingObserver::default();
        let recursive =
            collect_document_files(std::slice::from_ref(&temp_dir), true, &mut observer);
        assert_eq!(recursive.len(), 2);
        assert!(
            observer
                .events
                .iter()
                .any(|event| { matches!(event, RunEvent::ScanStarted { use_spinner: true }) })
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn filter_epub_files_passes_non_epubs_through_without_filter_events() {
        let files = vec![
            candidate("doc.docx", DocumentType::Docx),
            candidate("notes.txt", DocumentType::Docx),
        ];
        let filter = EpubFilter {
            title: Some("needle".to_string()),
            author: None,
        };
        let mut observer = CollectingObserver::default();

        let result = filter_epub_files(files.clone(), &filter, &mut observer);

        assert_eq!(result, files);
        assert!(observer.events.is_empty());
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

        let mut observer = CollectingObserver::default();
        let result = deduplicate_by_metadata(
            vec![
                DocumentCandidate::new(first.clone(), DocumentType::Epub),
                DocumentCandidate::new(second, DocumentType::Epub),
            ],
            &mut observer,
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, first);
        assert!(observer.events.iter().any(|event| {
            matches!(
                event,
                RunEvent::EpubDedupFinished {
                    duplicates_found: 1,
                    unique_remaining: 1
                }
            )
        }));

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
