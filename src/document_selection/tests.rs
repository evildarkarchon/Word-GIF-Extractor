//! Tests for Document selection.

use super::*;
use crate::document_search_surface::FilesystemSearchSurface;
use crate::extraction_run_observation::DocumentDiscoveryScope;
use crate::test_support::{
    RecordingRunObserver, create_directory_link, create_file_symlink, remove_directory_link,
    remove_file_symlink, temp_test_dir, write_epub_with_descriptive_declarations,
};
use std::fs;

/// Removes one empty directory mid-discovery, then records the run timeline.
///
/// Selection reports into the run observation stream, so the delegate is the
/// shared recording observer rather than a selection-specific one.
struct RemoveDirectoryOnScanStartObserver {
    directory: Option<PathBuf>,
    recording: RecordingRunObserver,
}

impl RemoveDirectoryOnScanStartObserver {
    /// Creates an observer that removes one empty directory after root classification.
    fn new(directory: PathBuf) -> Self {
        Self {
            directory: Some(directory),
            recording: RecordingRunObserver::default(),
        }
    }
}

impl ExtractionRunObserver for RemoveDirectoryOnScanStartObserver {
    /// Removes the directory at the initial discovery observation, then records it.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        if matches!(
            observation,
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0, .. }
        ) && let Some(directory) = self.directory.take()
        {
            fs::remove_dir(directory)
                .expect("classified directory should be removable before opening");
        }
        self.recording.on_observation(observation);
    }
}

#[test]
fn select_documents_reports_scanning_through_its_public_interface() {
    let temp_dir = temp_test_dir("document-selection", "public-scan-progress");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(temp_dir.join("book.docx"), []).expect("test DOCX should be writable");
    fs::write(temp_dir.join("notes.txt"), []).expect("ignored file should be writable");
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&temp_dir),
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(
        observer.selection_progress(),
        vec![
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RequestedInputs,
                discovered: 0
            },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RequestedInputs,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RequestedInputs,
                discovered: 1
            },
        ]
    );
    assert!(observer.selection_diagnostics().is_empty());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_reports_ordered_monotonic_phase_snapshots() {
    let temp_dir = temp_test_dir("document-selection", "ordered-phase-snapshots");
    let epub_path = temp_dir.join("book.epub");
    let docx_path = temp_dir.join("document.docx");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    write_epub_with_descriptive_declarations(&epub_path, "Test Author", "Magic Book");
    fs::write(docx_path, []).expect("test DOCX should be writable");
    let filter = EpubFilter {
        title: Some("magic".to_string()),
        author: None,
    };
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&temp_dir),
            recursive: false,
            output: None,
            epub_filter: &filter,
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 2);
    // Each phase is read as (count, is_final), which is what the running and
    // finished variants together say: monotonic counts, one terminating entry.
    let discovery: Vec<_> = observer
        .selection_progress()
        .iter()
        .filter_map(|observation| match observation {
            ExtractionRunObservation::DiscoveringDocuments { discovered, .. } => {
                Some((*discovered, false))
            }
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered, .. } => {
                Some((*discovered, true))
            }
            _ => None,
        })
        .collect();
    let filtering: Vec<_> = observer
        .selection_progress()
        .iter()
        .filter_map(|observation| match observation {
            ExtractionRunObservation::FilteringEpubs { checked, .. } => Some((*checked, false)),
            ExtractionRunObservation::EpubFilteringFinished { checked, .. } => {
                Some((*checked, true))
            }
            _ => None,
        })
        .collect();
    let deduplicating: Vec<_> = observer
        .selection_progress()
        .iter()
        .filter_map(|observation| match observation {
            ExtractionRunObservation::DeduplicatingEpubs { checked, .. } => Some((*checked, false)),
            ExtractionRunObservation::EpubDeduplicationFinished { checked, .. } => {
                Some((*checked, true))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        discovery,
        vec![(0, false), (1, false), (2, false), (2, true)]
    );
    assert_eq!(filtering, vec![(0, false), (1, false), (1, true)]);
    assert_eq!(deduplicating, vec![(0, false), (1, false), (1, true)]);

    let is_filtering = |observation: &ExtractionRunObservation| {
        matches!(
            observation,
            ExtractionRunObservation::FilteringEpubs { .. }
                | ExtractionRunObservation::EpubFilteringFinished { .. }
        )
    };
    let first_filter = observer
        .selection_progress()
        .iter()
        .position(is_filtering)
        .expect("filter progress should be reported");
    let last_scan = observer
        .selection_progress()
        .iter()
        .rposition(|observation| {
            matches!(
                observation,
                ExtractionRunObservation::DiscoveringDocuments { .. }
                    | ExtractionRunObservation::DocumentDiscoveryFinished { .. }
            )
        })
        .expect("scan progress should be reported");
    let first_dedup = observer
        .selection_progress()
        .iter()
        .position(|observation| {
            matches!(
                observation,
                ExtractionRunObservation::DeduplicatingEpubs { .. }
                    | ExtractionRunObservation::EpubDeduplicationFinished { .. }
            )
        })
        .expect("deduplication progress should be reported");
    let last_filter = observer
        .selection_progress()
        .iter()
        .rposition(is_filtering)
        .expect("filter progress should be reported");
    assert!(last_scan < first_filter);
    assert!(last_filter < first_dedup);
    assert!(observer.selection_diagnostics().is_empty());

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_reports_missing_input_as_a_structured_diagnostic() {
    let missing = temp_test_dir("document-selection", "missing-input").join("missing.docx");
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&missing),
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert!(selected.is_empty());
    assert_eq!(
        observer.selection_diagnostics(),
        vec![ExtractionRunObservation::MissingInput { path: missing }]
    );
}

#[test]
fn select_documents_reports_broken_requested_link_and_continues_to_supported_sibling() {
    let temp_dir = temp_test_dir("document-selection", "broken-requested-link");
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
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0,
                .. },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1,
                .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1,
                .. },
        ] if path == &broken_link && !detail.is_empty()
    ));
    assert!(
        !observer
            .selection_diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                diagnostic,
                ExtractionRunObservation::MissingInput { path } if path == &broken_link
            ))
    );

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies that a nested inspection failure stays ordered inside active scanning.
#[test]
fn select_documents_reports_broken_nested_link_before_later_supported_input() {
    let temp_dir = temp_test_dir("document-selection", "broken-nested-link");
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
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0,
                .. },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1,
                .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1,
                .. },
        ] if path == &broken_link && !detail.is_empty()
    ));

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies recursive inspection diagnoses one broken nested link at encounter position.
#[test]
fn select_documents_reports_broken_nested_link_once_during_recursive_scanning() {
    let temp_dir = temp_test_dir("document-selection", "recursive-broken-nested-link");
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
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0 },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1 },
            ExtractionRunObservation::DocumentDiscoveryFinished { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1 },
        ] if path == &broken_link && !detail.is_empty()
    ));

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies an all-failed recursive scan still finishes with an explicit zero count.
#[test]
fn select_documents_finishes_recursive_scanning_at_zero_after_failure() {
    let temp_dir = temp_test_dir("document-selection", "recursive-zero-after-failure");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&requested_directory),
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert!(selected.is_empty());
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0 },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DocumentDiscoveryFinished { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0 },
        ] if path == &broken_link && !detail.is_empty()
    ));

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies requested-root fallback and continuation when opening a directory fails.
#[test]
fn select_documents_reports_directory_open_failure_and_continues_to_supported_input() {
    let temp_dir = temp_test_dir("document-selection", "directory-open-failure");
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
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.recording.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0,
                .. },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1,
                .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1,
                .. },
        ] if path == &removed_directory && !detail.is_empty()
    ));

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies recursive traversal reports a vanished root and continues to a later input.
#[test]
fn select_documents_reports_recursive_root_traversal_failure_and_continues() {
    let temp_dir = temp_test_dir("document-selection", "recursive-root-traversal-failure");
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
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.recording.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0 },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1 },
            ExtractionRunObservation::DocumentDiscoveryFinished { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1 },
        ] if path == &removed_directory && !detail.is_empty()
    ));

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies distinct recursive failures are each reported once in encounter order.
#[test]
fn select_documents_orders_distinct_recursive_failures_before_later_progress() {
    let temp_dir = temp_test_dir("document-selection", "ordered-recursive-failures");
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
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.recording.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0 },
            ExtractionRunObservation::DocumentDiscoveryFailed {
                    path: first_path,
                    detail: first_detail,
                },
            ExtractionRunObservation::DocumentDiscoveryFailed {
                    path: second_path,
                    detail: second_detail,
                },
            ExtractionRunObservation::DiscoveringDocuments { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1 },
            ExtractionRunObservation::DocumentDiscoveryFinished { scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1 },
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
    let temp_dir = temp_test_dir("document-selection", "recursive-readable-nested-sibling");
    let requested_directory = temp_dir.join("requested");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = requested_directory.join("broken-link");
    let readable_sibling = requested_directory.join("readable.docx");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");
    fs::write(&readable_sibling, []).expect("readable DOCX should be writable");
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&requested_directory),
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), readable_sibling);
    assert!(matches!(
        observer.selection_diagnostics().as_slice(),
        [ExtractionRunObservation::DocumentDiscoveryFailed { path, detail }]
            if path == &broken_link && !detail.is_empty()
    ));
    assert!(matches!(
        observer.selection_progress().last(),
        Some(ExtractionRunObservation::DocumentDiscoveryFinished {
            scope: DocumentDiscoveryScope::RecursiveDirectories,
            discovered: 1
        })
    ));

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies that a supported nested file link remains eligible without recursive scanning.
#[test]
fn select_documents_keeps_nested_supported_file_link_eligible() {
    let temp_dir = temp_test_dir("document-selection", "nested-supported-file-link");
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
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&requested_directory),
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), linked_document);
    assert!(observer.selection_diagnostics().is_empty());
    assert!(matches!(
        observer.selection_progress().as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0, .. },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1, .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1, .. },
        ]
    ));

    remove_file_symlink(&linked_document);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies recursive scanning still admits a nested link to a supported regular file.
#[test]
fn select_documents_keeps_nested_supported_file_link_eligible_when_recursive() {
    let temp_dir = temp_test_dir("document-selection", "recursive-nested-supported-file-link");
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
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&requested_directory),
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), linked_document);
    assert!(observer.selection_diagnostics().is_empty());
    assert!(matches!(
        observer.selection_progress().as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
        ]
    ));

    remove_file_symlink(&linked_document);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_follows_requested_directory_link_during_recursive_scanning() {
    let temp_dir = temp_test_dir("document-selection", "requested-directory-link");
    let target = temp_dir.join("target");
    let requested_link = temp_dir.join("requested-link");
    let linked_document = requested_link.join("linked.docx");
    fs::create_dir_all(&target).expect("link target should be creatable");
    fs::write(target.join("linked.docx"), []).expect("linked DOCX should be writable");
    create_directory_link(&target, &requested_link);
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&requested_link),
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), linked_document);
    assert!(observer.selection_diagnostics().is_empty());
    assert!(matches!(
        observer.selection_progress().as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
        ]
    ));

    remove_directory_link(&requested_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Verifies recursive scanning does not widen scope through a nested directory link.
#[test]
fn select_documents_does_not_follow_nested_directory_link_when_recursive() {
    let temp_dir = temp_test_dir("document-selection", "recursive-nested-directory-link");
    let requested_directory = temp_dir.join("requested");
    let target_directory = temp_dir.join("target");
    let nested_link = requested_directory.join("nested-link");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&target_directory).expect("link target should be creatable");
    fs::write(target_directory.join("outside.docx"), [])
        .expect("linked-directory DOCX should be writable");
    create_directory_link(&target_directory, &nested_link);
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&requested_directory),
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert!(selected.is_empty());
    assert!(observer.selection_diagnostics().is_empty());
    assert_eq!(
        observer.selection_progress(),
        vec![
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
        ]
    );

    remove_directory_link(&nested_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_keeps_scanning_silent_when_no_inputs_are_requested() {
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &[],
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert!(selected.is_empty());
    assert!(observer.selection_progress().is_empty());
    assert!(observer.selection_diagnostics().is_empty());
}

#[test]
fn select_documents_reports_filtering_metadata_failure_and_skips_deduplication() {
    let temp_dir = temp_test_dir("document-selection", "filter-metadata-failure");
    let epub_path = temp_dir.join("invalid.epub");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&epub_path, b"not an epub").expect("invalid EPUB should be writable");
    let filter = EpubFilter {
        title: Some("needle".to_string()),
        author: None,
    };
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&epub_path),
            recursive: false,
            output: None,
            epub_filter: &filter,
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert!(selected.is_empty());
    assert!(matches!(
        observer.selection_diagnostics().as_slice(),
        [ExtractionRunObservation::UnreadableEpubMetadata {
            path,
            purpose: EpubMetadataPurpose::Filtering,
            detail,
        }] if path == &epub_path && !detail.is_empty()
    ));
    assert!(
        observer
            .selection_progress()
            .iter()
            .any(|progress| matches!(
                progress,
                ExtractionRunObservation::EpubFilteringFinished {
                    checked: 1,
                    total: 1,
                    matching: 0,
                    ..
                }
            ))
    );
    assert!(
        !observer
            .selection_progress()
            .iter()
            .any(|progress| matches!(
                progress,
                ExtractionRunObservation::DeduplicatingEpubs { .. }
                    | ExtractionRunObservation::EpubDeduplicationFinished { .. }
            ))
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_orders_filter_diagnostic_before_progress_advances_and_finish() {
    let temp_dir = temp_test_dir("document-selection", "filter-diagnostic-order");
    let invalid_epub = temp_dir.join("invalid.epub");
    let valid_epub = temp_dir.join("valid.epub");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&invalid_epub, b"not an epub").expect("invalid EPUB should be writable");
    write_epub_with_descriptive_declarations(&valid_epub, "Test Author", "Magic Book");
    let inputs = vec![invalid_epub.clone(), valid_epub];
    let filter = EpubFilter {
        title: Some("magic".to_string()),
        author: None,
    };
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive: false,
            output: None,
            epub_filter: &filter,
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    let filtering_facts: Vec<_> = observer
        .observations
        .iter()
        .filter(|fact| {
            matches!(
                fact,
                ExtractionRunObservation::FilteringEpubs { .. }
                    | ExtractionRunObservation::EpubFilteringFinished { .. }
                    | ExtractionRunObservation::UnreadableEpubMetadata {
                        purpose: EpubMetadataPurpose::Filtering,
                        ..
                    }
            )
        })
        .collect();

    assert_eq!(filtering_facts.len(), 5);
    assert!(matches!(
        filtering_facts[0],
        ExtractionRunObservation::FilteringEpubs {
            checked: 0,
            matching: 0,
            ..
        }
    ));
    assert!(matches!(
        filtering_facts[1],
        ExtractionRunObservation::UnreadableEpubMetadata {
                path,
                purpose: EpubMetadataPurpose::Filtering,
                ..
            } if path == &invalid_epub
    ));
    assert!(matches!(
        filtering_facts[2],
        ExtractionRunObservation::FilteringEpubs {
            checked: 1,
            matching: 0,
            ..
        }
    ));
    assert!(matches!(
        filtering_facts[3],
        ExtractionRunObservation::FilteringEpubs {
            checked: 2,
            matching: 1,
            ..
        }
    ));
    assert!(matches!(
        filtering_facts[4],
        ExtractionRunObservation::EpubFilteringFinished {
            checked: 2,
            matching: 1,
            ..
        }
    ));

    let filter_finished = observer
        .observations
        .iter()
        .position(|fact| matches!(fact, ExtractionRunObservation::EpubFilteringFinished { .. }))
        .expect("filtering should finish");
    let dedup_started = observer
        .observations
        .iter()
        .position(|fact| {
            matches!(
                fact,
                ExtractionRunObservation::DeduplicatingEpubs { .. }
                    | ExtractionRunObservation::EpubDeduplicationFinished { .. }
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
    let temp_dir = temp_test_dir("document-selection", "collect-recursive");
    let nested = temp_dir.join("nested");
    fs::create_dir_all(&nested).expect("nested test directory should be creatable");
    fs::write(temp_dir.join("root.docx"), []).expect("root docx should be writable");
    fs::write(nested.join("nested.epub"), []).expect("nested epub should be writable");
    fs::write(temp_dir.join("ignored.txt"), []).expect("ignored file should be writable");

    let mut observer = RecordingRunObserver::default();
    let non_recursive = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&temp_dir),
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );
    assert_eq!(non_recursive.len(), 1);

    let mut observer = RecordingRunObserver::default();
    let recursive = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&temp_dir),
            recursive: true,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );
    assert_eq!(recursive.len(), 2);
    assert!(
        observer
            .selection_progress()
            .iter()
            .any(|progress| matches!(
                progress,
                ExtractionRunObservation::DiscoveringDocuments {
                    scope: DocumentDiscoveryScope::RecursiveDirectories,
                    ..
                } | ExtractionRunObservation::DocumentDiscoveryFinished {
                    scope: DocumentDiscoveryScope::RecursiveDirectories,
                    ..
                }
            ))
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_skips_epub_filter_progress_when_no_epubs_are_selected() {
    let temp_dir = temp_test_dir("document-selection", "skip-empty-filter");
    let docx = temp_dir.join("doc.docx");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    fs::write(&docx, []).expect("test DOCX should be writable");
    let filter = EpubFilter {
        title: Some("needle".to_string()),
        author: None,
    };
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&docx),
            recursive: false,
            output: None,
            epub_filter: &filter,
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert!(
        !observer
            .selection_progress()
            .iter()
            .any(|progress| matches!(
                progress,
                ExtractionRunObservation::FilteringEpubs { .. }
                    | ExtractionRunObservation::EpubFilteringFinished { .. }
                    | ExtractionRunObservation::DeduplicatingEpubs { .. }
                    | ExtractionRunObservation::EpubDeduplicationFinished { .. }
            ))
    );

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn select_documents_deduplicates_matching_readable_epub_declarations() {
    let temp_dir = temp_test_dir("document-selection", "declaration-dedupe");
    fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
    let first = temp_dir.join("first.epub");
    let second = temp_dir.join("second.epub");
    write_epub_with_descriptive_declarations(&first, "Shared Creator", "Shared Title");
    write_epub_with_descriptive_declarations(&second, "Shared Creator", "Shared Title");

    let mut observer = RecordingRunObserver::default();
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &[first.clone(), second],
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), first);
    assert!(observer.selection_progress().iter().any(|progress| {
        matches!(
            progress,
            ExtractionRunObservation::EpubDeduplicationFinished {
                duplicates_found: 1,
                unique_remaining: 1,
                ..
            }
        )
    }));

    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn declaration_deduplication_falls_back_to_filename_when_declarations_cannot_be_read() {
    let temp_dir = temp_test_dir("document-selection", "dedupe-fallback");
    let first_dir = temp_dir.join("first");
    let second_dir = temp_dir.join("second");
    fs::create_dir_all(&first_dir).expect("first test directory should be creatable");
    fs::create_dir_all(&second_dir).expect("second test directory should be creatable");
    let first = first_dir.join("book.epub");
    let second = second_dir.join("book.epub");
    fs::write(&first, b"not an epub").expect("first invalid epub should be writable");
    fs::write(&second, b"not an epub").expect("second invalid epub should be writable");

    let mut observer = RecordingRunObserver::default();
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &[first.clone(), second],
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
        &mut observer,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), first);
    assert!(observer.selection_progress().iter().any(|progress| {
        matches!(
            progress,
            ExtractionRunObservation::EpubDeduplicationFinished {
                duplicates_found: 1,
                unique_remaining: 1,
                ..
            }
        )
    }));
    assert_eq!(
        observer
            .selection_diagnostics()
            .iter()
            .filter(|diagnostic| matches!(
                diagnostic,
                ExtractionRunObservation::UnreadableEpubMetadata {
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
    let temp_dir = temp_test_dir("document-selection", "selected-epub-identity");
    let epub_path = temp_dir.join("sample.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_epub_with_descriptive_declarations(&epub_path, "Tester", "Magic Test");
    let mut observer = RecordingRunObserver::default();
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: std::slice::from_ref(&epub_path),
            recursive: false,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        &FilesystemSearchSurface,
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
