//! Tests for Document selection.
//!
//! Discovery behaviour is asserted against a declared Document search surface
//! rather than a staged temporary directory: a link whose target is gone, a
//! directory that cannot be opened and an entry that stopped being a directory
//! are all values here. That the real surface reports those conditions the way
//! the operating system does is a separate question, answered by the conformance
//! tests beside `FilesystemSearchSurface` itself.
//!
//! The tests that still stage disk are the ones whose subject is EPUB
//! declarations, which reach the world through their own acquisition path.

use super::*;
use crate::document_search_surface::{FilesystemSearchSurface, InspectedKind};
use crate::extraction_run_observation::DocumentDiscoveryScope;
use crate::test_support::{
    InMemorySearchSurface, RecordingRunObserver, temp_test_dir,
    write_epub_with_descriptive_declarations,
};
use std::fs;

/// Selects against a declared surface with default options and records the run.
fn select_against(
    surface: &InMemorySearchSurface,
    inputs: &[&str],
    recursive: bool,
) -> (Vec<SelectedDocument>, RecordingRunObserver) {
    let inputs: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
    let mut observer = RecordingRunObserver::default();
    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive,
            output: None,
            epub_filter: &EpubFilter::default(),
        },
        surface,
        &mut observer,
    );
    (selected, observer)
}

#[test]
fn select_documents_reports_scanning_through_its_public_interface() {
    let surface = InMemorySearchSurface::new()
        .with_directory("root")
        .with_file("root/book.docx")
        .with_file("root/notes.txt");

    let (selected, observer) = select_against(&surface, &["root"], false);

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
}

#[test]
fn select_documents_reports_missing_input_as_a_structured_diagnostic() {
    let surface = InMemorySearchSurface::new();

    let (selected, observer) = select_against(&surface, &["missing.docx"], false);

    assert!(selected.is_empty());
    assert_eq!(
        observer.selection_diagnostics(),
        vec![ExtractionRunObservation::MissingInput {
            path: PathBuf::from("missing.docx")
        }]
    );
}

#[test]
fn select_documents_reports_broken_requested_link_and_continues_to_supported_sibling() {
    let surface = InMemorySearchSurface::new()
        .with_link("broken-link", None)
        .with_file("notes.txt")
        .with_file("readable.docx");

    let (selected, observer) = select_against(
        &surface,
        &["broken-link", "notes.txt", "readable.docx"],
        false,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    // A link whose target is gone is an inspection failure, never a missing input:
    // the path is there, and only its target is not.
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0, .. },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1, .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1, .. },
        ] if path == Path::new("broken-link") && !detail.is_empty()
    ));
}

#[test]
fn select_documents_skips_an_inspectable_unsupported_object() {
    let surface = InMemorySearchSurface::new()
        .with_other("device")
        .with_file("readable.docx");

    let (selected, observer) = select_against(&surface, &["device", "readable.docx"], false);

    // An object that is neither a file nor a directory is skipped in silence;
    // it is not a failure to inspect and not a missing input.
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    assert!(observer.selection_diagnostics().is_empty());
}

/// Verifies that a nested inspection failure stays ordered inside active scanning.
#[test]
fn select_documents_reports_broken_nested_link_before_later_supported_input() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_link("requested/broken-link", None)
        .with_file("requested/notes.txt")
        .with_file("readable.docx");

    let (selected, observer) = select_against(&surface, &["requested", "readable.docx"], false);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0, .. },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1, .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1, .. },
        ] if path == Path::new("requested/broken-link") && !detail.is_empty()
    ));
}

/// Verifies recursive inspection diagnoses one broken nested link at encounter position.
#[test]
fn select_documents_reports_broken_nested_link_once_during_recursive_scanning() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_link("requested/broken-link", None)
        .with_file("readable.docx");

    let (selected, observer) = select_against(&surface, &["requested", "readable.docx"], true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
        ] if path == Path::new("requested/broken-link") && !detail.is_empty()
    ));
}

/// Verifies an all-failed recursive scan still finishes with an explicit zero count.
#[test]
fn select_documents_finishes_recursive_scanning_at_zero_after_failure() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_link("requested/broken-link", None);

    let (selected, observer) = select_against(&surface, &["requested"], true);

    assert!(selected.is_empty());
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
        ] if path == Path::new("requested/broken-link") && !detail.is_empty()
    ));
}

/// Verifies requested-root fallback and continuation when opening a directory fails.
#[test]
fn select_documents_reports_directory_open_failure_and_continues_to_supported_input() {
    // A directory that classifies but will not open is what a directory removed
    // between classification and opening leaves behind.
    let surface = InMemorySearchSurface::new()
        .with_listing_failure("unopenable")
        .with_file("readable.docx");

    let (selected, observer) = select_against(&surface, &["unopenable", "readable.docx"], false);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0, .. },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1, .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1, .. },
        ] if path == Path::new("unopenable") && !detail.is_empty()
    ));
}

/// Verifies recursive traversal reports an unopenable root and continues to a later input.
#[test]
fn select_documents_reports_recursive_root_traversal_failure_and_continues() {
    let surface = InMemorySearchSurface::new()
        .with_listing_failure("unopenable")
        .with_file("readable.docx");

    let (selected, observer) = select_against(&surface, &["unopenable", "readable.docx"], true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
        ] if path == Path::new("unopenable") && !detail.is_empty()
    ));
}

/// Verifies distinct recursive failures are each reported once in encounter order.
#[test]
fn select_documents_orders_distinct_recursive_failures_before_later_progress() {
    let surface = InMemorySearchSurface::new()
        .with_directory("first")
        .with_link("first/broken-link", None)
        .with_listing_failure("second-unopenable")
        .with_file("readable.docx");

    let (selected, observer) = select_against(
        &surface,
        &["first", "second-unopenable", "readable.docx"],
        true,
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("readable.docx"));
    assert!(matches!(
        observer.observations.as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 0
            },
            ExtractionRunObservation::DocumentDiscoveryFailed {
                path: first_path,
                detail: first_detail,
            },
            ExtractionRunObservation::DocumentDiscoveryFailed {
                path: second_path,
                detail: second_detail,
            },
            ExtractionRunObservation::DiscoveringDocuments {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
            ExtractionRunObservation::DocumentDiscoveryFinished {
                scope: DocumentDiscoveryScope::RecursiveDirectories,
                discovered: 1
            },
        ] if first_path == Path::new("first/broken-link")
            && !first_detail.is_empty()
            && second_path == Path::new("second-unopenable")
            && !second_detail.is_empty()
    ));
}

/// Verifies a broken recursive entry does not suppress a readable sibling candidate.
#[test]
fn select_documents_keeps_readable_nested_sibling_after_recursive_failure() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_link("requested/broken-link", None)
        .with_file("requested/readable.docx");

    let (selected, observer) = select_against(&surface, &["requested"], true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("requested/readable.docx"));
    assert!(matches!(
        observer.selection_diagnostics().as_slice(),
        [ExtractionRunObservation::DocumentDiscoveryFailed { path, detail }]
            if path == Path::new("requested/broken-link") && !detail.is_empty()
    ));
    assert!(matches!(
        observer.selection_progress().last(),
        Some(ExtractionRunObservation::DocumentDiscoveryFinished {
            scope: DocumentDiscoveryScope::RecursiveDirectories,
            discovered: 1
        })
    ));
}

/// Verifies that a supported nested file link remains eligible without recursive scanning.
#[test]
fn select_documents_keeps_nested_supported_file_link_eligible() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_directory("targets")
        .with_file("targets/target.docx")
        .with_link("requested/linked.docx", Some("targets/target.docx"));

    let (selected, observer) = select_against(&surface, &["requested"], false);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("requested/linked.docx"));
    assert!(observer.selection_diagnostics().is_empty());
    assert!(matches!(
        observer.selection_progress().as_slice(),
        [
            ExtractionRunObservation::DiscoveringDocuments { discovered: 0, .. },
            ExtractionRunObservation::DiscoveringDocuments { discovered: 1, .. },
            ExtractionRunObservation::DocumentDiscoveryFinished { discovered: 1, .. },
        ]
    ));
}

/// Verifies recursive scanning still admits a nested link to a supported regular file.
#[test]
fn select_documents_keeps_nested_supported_file_link_eligible_when_recursive() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_directory("targets")
        .with_file("targets/target.docx")
        .with_link("requested/linked.docx", Some("targets/target.docx"));

    let (selected, observer) = select_against(&surface, &["requested"], true);

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].get_path(), Path::new("requested/linked.docx"));
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
}

#[test]
fn select_documents_follows_requested_directory_link_during_recursive_scanning() {
    let surface = InMemorySearchSurface::new()
        .with_directory("target")
        .with_file("target/linked.docx")
        .with_link("requested-link", Some("target"));

    let (selected, observer) = select_against(&surface, &["requested-link"], true);

    // The discovered document is named under the requested link, not under the
    // target it resolves to, so output placement follows what the user asked for.
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].get_path(),
        Path::new("requested-link/linked.docx")
    );
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
}

/// Verifies recursive scanning does not widen scope through a nested directory link.
#[test]
fn select_documents_does_not_follow_nested_directory_link_when_recursive() {
    let surface = InMemorySearchSurface::new()
        .with_directory("requested")
        .with_directory("target")
        .with_file("target/outside.docx")
        .with_link("requested/nested-link", Some("target"));

    let (selected, observer) = select_against(&surface, &["requested"], true);

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
}

/// Verifies a branch is abandoned when its directory entry no longer names a directory.
#[test]
fn select_documents_abandons_a_recursive_branch_that_stopped_being_a_directory() {
    // The entry enumerated as a directory and inspects as a file, which is what a
    // directory replaced between enumeration and inspection leaves behind. Its
    // former contents must not be traversed using the stale classification.
    let surface = InMemorySearchSurface::new()
        .with_directory("root")
        .with_stale_directory("root/stale", InspectedKind::File)
        .with_file("root/stale/hidden.docx");

    let (selected, observer) = select_against(&surface, &["root"], true);

    assert!(selected.is_empty());
    assert!(observer.selection_diagnostics().is_empty());
}

/// Verifies a branch is abandoned once, without a second failure, when inspection fails.
#[test]
fn select_documents_abandons_a_recursive_branch_whose_inspection_failed() {
    let surface = InMemorySearchSurface::new()
        .with_directory("root")
        .with_stale_directory_inspection_failure("root/stale")
        .with_file("root/stale/hidden.docx");

    let (selected, observer) = select_against(&surface, &["root"], true);

    assert!(selected.is_empty());
    // One failed inspection is reported once; opening the same directory is not
    // attempted afterwards, so no second failure follows it.
    assert!(matches!(
        observer.selection_diagnostics().as_slice(),
        [ExtractionRunObservation::DocumentDiscoveryFailed { path, detail }]
            if path == Path::new("root/stale") && !detail.is_empty()
    ));
}

/// Verifies a traversal failure with no path is attributed to the nearest known directory.
#[test]
fn select_documents_attributes_a_pathless_failure_to_the_nearest_known_directory() {
    let surface = InMemorySearchSurface::new()
        .with_directory("root")
        .with_directory("root/branch")
        .with_pathless_listing_failure("root/branch/deep");

    let (selected, observer) = select_against(&surface, &["root"], true);

    assert!(selected.is_empty());
    // The failure knows its depth but not its path, so discovery names the
    // deepest directory it had already confirmed above that depth.
    assert!(matches!(
        observer.selection_diagnostics().as_slice(),
        [ExtractionRunObservation::DocumentDiscoveryFailed { path, detail }]
            if path == Path::new("root/branch") && !detail.is_empty()
    ));
}

#[test]
fn select_documents_keeps_scanning_silent_when_no_inputs_are_requested() {
    let surface = InMemorySearchSurface::new();

    let (selected, observer) = select_against(&surface, &[], false);

    assert!(selected.is_empty());
    assert!(observer.selection_progress().is_empty());
    assert!(observer.selection_diagnostics().is_empty());
}

#[test]
fn select_documents_respects_recursive_scanning_through_its_public_interface() {
    let surface = InMemorySearchSurface::new()
        .with_directory("root")
        .with_directory("root/nested")
        .with_file("root/root.docx")
        .with_file("root/nested/nested.epub")
        .with_file("root/ignored.txt");

    let (non_recursive, _) = select_against(&surface, &["root"], false);
    assert_eq!(non_recursive.len(), 1);

    let (recursive, observer) = select_against(&surface, &["root"], true);
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
}

#[test]
fn select_documents_skips_epub_filter_progress_when_no_epubs_are_selected() {
    let surface = InMemorySearchSurface::new().with_file("doc.docx");
    let filter = EpubFilter {
        title: Some("needle".to_string()),
        author: None,
    };
    let inputs = vec![PathBuf::from("doc.docx")];
    let mut observer = RecordingRunObserver::default();

    let selected = select_documents(
        DocumentSelectionOptions {
            inputs: &inputs,
            recursive: false,
            output: None,
            epub_filter: &filter,
        },
        &surface,
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
