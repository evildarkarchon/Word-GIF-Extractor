//! Extraction run workflow for document discovery, dispatch, and aggregation.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::common::{ExtractionConfig, ExtractionCounts};
use crate::docx;
use crate::epub::{self, EpubFilter};

/// Supported document types.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DocumentType {
    Docx,
    Epub,
}

/// Options for one extraction run after CLI arguments have been normalized.
///
/// The CLI adapter owns parsing and validation. This module receives only the
/// workflow facts it needs to discover documents, dispatch them, and aggregate
/// the final outcome.
pub struct RunOptions<'a> {
    /// Input files or directories to scan.
    pub inputs: Vec<PathBuf>,
    /// Whether directory inputs should be scanned recursively.
    pub recursive: bool,
    /// Optional output directory shared by every input file.
    pub output: Option<PathBuf>,
    /// Image extensions accepted by the run.
    pub allowed_extensions: HashSet<&'static str>,
    /// Extract only cover images from EPUB files.
    pub cover_only: bool,
    /// Extract all EPUB images if a cover cannot be found.
    pub cover_fallback: bool,
    /// EPUB metadata filter criteria.
    pub epub_filter: EpubFilter,
    /// Image extraction and conversion behavior.
    pub extraction: ExtractionConfig<'a>,
}

/// Aggregated outcome from an extraction run.
///
/// The report keeps workflow facts behind the extraction run seam so callers do
/// not have to infer summary state from raw counters.
#[derive(Debug, Default, Clone, Copy)]
pub struct RunReport {
    /// Total write/conversion counts across every processed document.
    pub total_counts: ExtractionCounts,
    /// Number of documents that produced at least one output image.
    pub documents_with_output: usize,
    /// Whether any DOCX document produced output images.
    pub has_docx_images: bool,
    /// Whether this run was requested in cover-only mode.
    pub cover_only: bool,
    /// Number of documents selected for processing after filtering and dedupe.
    pub documents_to_process: usize,
}

/// Domain event emitted while an extraction run progresses.
///
/// Events describe extraction-run facts, not terminal UI commands. The CLI
/// adapter decides how to render them as progress bars or warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// Input path does not exist and will be skipped by discovery.
    InputWarning { path: PathBuf },
    /// Document discovery has started.
    ScanStarted { use_spinner: bool },
    /// A supported document was discovered.
    DocumentDiscovered { count: usize },
    /// Document discovery has finished.
    ScanFinished { count: usize },
    /// EPUB metadata filtering has started.
    EpubFilterStarted { description: String, total: usize },
    /// One EPUB has been checked against the filter.
    EpubFilterAdvanced,
    /// An EPUB could not be read while filtering.
    EpubFilterWarning { path: PathBuf, message: String },
    /// EPUB metadata filtering has finished.
    EpubFilterFinished { matching: usize },
    /// EPUB metadata deduplication has started.
    EpubDedupStarted { total: usize },
    /// One EPUB has been checked for deduplication.
    EpubDedupAdvanced,
    /// EPUB metadata deduplication has finished.
    EpubDedupFinished {
        duplicates_found: usize,
        unique_remaining: usize,
    },
    /// Document extraction has started.
    ExtractionStarted { total: usize, cover_only: bool },
    /// A document is about to be processed.
    DocumentStarted { path: PathBuf, display_name: String },
    /// A document failed to process; the run will continue.
    DocumentError { path: PathBuf, message: String },
    /// A document has finished processing, successfully or not.
    DocumentFinished { path: PathBuf },
}

/// Observer for extraction-run events.
pub trait RunObserver {
    /// Handles one event emitted by the extraction run.
    fn on_event(&mut self, event: RunEvent);
}

/// Executes one extraction run and returns its aggregate report.
///
/// Per-document failures are emitted as events and do not abort the run. Setup
/// failures that prevent the run from starting are returned as errors.
pub fn run(options: RunOptions<'_>, observer: &mut impl RunObserver) -> Result<RunReport> {
    for input_path in &options.inputs {
        if !input_path.exists() {
            observer.on_event(RunEvent::InputWarning {
                path: input_path.clone(),
            });
        }
    }

    let files = collect_document_files(&options.inputs, options.recursive, observer);

    let filtered_files = if !options.epub_filter.is_empty() {
        filter_epub_files(files, &options.epub_filter, observer)
    } else {
        files
    };

    let files_to_process = deduplicate_by_metadata(filtered_files, observer);
    let mut report = RunReport {
        cover_only: options.cover_only,
        documents_to_process: files_to_process.len(),
        ..RunReport::default()
    };

    if files_to_process.is_empty() {
        return Ok(report);
    }

    observer.on_event(RunEvent::ExtractionStarted {
        total: files_to_process.len(),
        cover_only: options.cover_only,
    });

    for path in &files_to_process {
        let is_docx = get_document_type(path) == Some(DocumentType::Docx);
        let display_name = display_name_for(path, options.cover_only);
        observer.on_event(RunEvent::DocumentStarted {
            path: path.clone(),
            display_name,
        });

        let effective_output = resolve_output_dir(path, options.output.as_deref());
        match process_file(
            path,
            &effective_output,
            &options.allowed_extensions,
            options.cover_only,
            options.cover_fallback,
            &options.epub_filter,
            &options.extraction,
        ) {
            Ok(counts) => report.record_document_result(counts, is_docx),
            Err(e) => observer.on_event(RunEvent::DocumentError {
                path: path.clone(),
                message: e.to_string(),
            }),
        }

        observer.on_event(RunEvent::DocumentFinished { path: path.clone() });
    }

    Ok(report)
}

impl RunReport {
    /// Records the counts for one successfully processed document.
    fn record_document_result(&mut self, counts: ExtractionCounts, is_docx: bool) {
        self.total_counts.extracted += counts.extracted;
        self.total_counts.gifs_routed += counts.gifs_routed;
        self.total_counts.converted += counts.converted;
        self.total_counts.skipped += counts.skipped;

        if counts.extracted > 0 {
            self.documents_with_output += 1;
            if is_docx {
                self.has_docx_images = true;
            }
        }
    }
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

/// Checks if a path is an EPUB file.
fn is_epub(path: &Path) -> bool {
    get_document_type(path) == Some(DocumentType::Epub)
}

/// Collects all document files from the input paths.
fn collect_document_files(
    inputs: &[PathBuf],
    recursive: bool,
    observer: &mut impl RunObserver,
) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // Use spinner for recursive directory scanning since we don't know total count upfront.
    let use_spinner = recursive && inputs.iter().any(|p| p.is_dir());
    observer.on_event(RunEvent::ScanStarted { use_spinner });

    for input_path in inputs {
        if !input_path.exists() {
            continue;
        }

        if input_path.is_file() && is_supported_document(input_path) {
            files.push(input_path.clone());
            observer.on_event(RunEvent::DocumentDiscovered { count: files.len() });
        } else if input_path.is_dir() {
            if recursive {
                for entry in WalkDir::new(input_path).into_iter().flatten() {
                    let path = entry.path();
                    if path.is_file() && is_supported_document(path) {
                        files.push(path.to_path_buf());
                        observer.on_event(RunEvent::DocumentDiscovered { count: files.len() });
                    }
                }
            } else if let Ok(entries) = fs::read_dir(input_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && is_supported_document(&path) {
                        files.push(path);
                        observer.on_event(RunEvent::DocumentDiscovered { count: files.len() });
                    }
                }
            }
        }
    }

    observer.on_event(RunEvent::ScanFinished { count: files.len() });
    files
}

/// Filters EPUB files by metadata.
///
/// Returns only the files that match the filter criteria while passing non-EPUB
/// files through unchanged.
fn filter_epub_files(
    files: Vec<PathBuf>,
    filter: &EpubFilter,
    observer: &mut impl RunObserver,
) -> Vec<PathBuf> {
    // Separate EPUB files from other document types.
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| is_epub(p));

    if epub_files.is_empty() {
        return other_files;
    }

    observer.on_event(RunEvent::EpubFilterStarted {
        description: filter.description(),
        total: epub_files.len(),
    });

    let mut matching_epubs = Vec::new();
    for path in epub_files {
        observer.on_event(RunEvent::EpubFilterAdvanced);
        match epub::check_filter_match(&path, filter) {
            Ok(true) => matching_epubs.push(path),
            Ok(false) => {} // File doesn't match filter, skip.
            Err(e) => {
                // Log error but continue searching.
                observer.on_event(RunEvent::EpubFilterWarning {
                    path,
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
fn deduplicate_by_metadata(files: Vec<PathBuf>, observer: &mut impl RunObserver) -> Vec<PathBuf> {
    let (epub_files, other_files): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| is_epub(p));

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

    for path in epub_files {
        observer.on_event(RunEvent::EpubDedupAdvanced);

        // Try to get metadata for deduplication.
        let key = match epub::get_metadata(&path) {
            Ok((author, title)) if author.is_some() || title.is_some() => {
                // Normalize: lowercase and trim, use empty string for None.
                let author_key = author
                    .as_deref()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                let title_key = title
                    .as_deref()
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                (author_key, title_key)
            }
            _ => {
                // Fallback to filename if we can't read metadata or if both author and title are missing.
                let filename = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                (String::new(), filename)
            }
        };

        // Only add if we haven't seen this combination before.
        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(key) {
            e.insert(path.clone());
            unique_epubs.push(path);
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

/// Processes a single file based on its type.
fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    epub_filter: &EpubFilter,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => {
            docx::process_file(input_path, output_base_dir, allowed_extensions, config)
        }
        Some(DocumentType::Epub) => epub::process_file(
            input_path,
            output_base_dir,
            allowed_extensions,
            cover_only,
            cover_fallback,
            epub_filter,
            config,
        ),
        None => {
            anyhow::bail!(
                "Unsupported file type: {}. Supported types: .docx, .epub",
                input_path.display()
            );
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

/// Builds the progress display name for a document.
fn display_name_for(path: &Path, cover_only: bool) -> String {
    // For EPUBs in cover-only mode, show the output filename (Author - Title).
    // Otherwise show the input filename.
    if cover_only && is_epub(path) {
        epub::get_base_name(path).unwrap_or_else(|_| fallback_display_name(path))
    } else {
        fallback_display_name(path)
    }
}

/// Builds a fallback display name from a path.
fn fallback_display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
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
            "word-image-extractor-extraction-run-{test_name}-{}-{nanos}",
            std::process::id()
        ))
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
        let files = vec![PathBuf::from("doc.docx"), PathBuf::from("notes.txt")];
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
        let result = deduplicate_by_metadata(vec![first.clone(), second], &mut observer);

        assert_eq!(result, vec![first]);
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
    fn run_report_records_counts_and_docx_output_state() {
        let mut report = RunReport::default();

        report.record_document_result(
            ExtractionCounts {
                extracted: 2,
                gifs_routed: 1,
                converted: 1,
                skipped: 0,
            },
            true,
        );
        report.record_document_result(
            ExtractionCounts {
                extracted: 0,
                gifs_routed: 0,
                converted: 0,
                skipped: 1,
            },
            false,
        );

        assert_eq!(report.total_counts.extracted, 2);
        assert_eq!(report.total_counts.gifs_routed, 1);
        assert_eq!(report.total_counts.converted, 1);
        assert_eq!(report.total_counts.skipped, 1);
        assert_eq!(report.documents_with_output, 1);
        assert!(report.has_docx_images);
    }
}
