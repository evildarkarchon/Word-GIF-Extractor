//! Progress lifecycle and diagnostics for Document selection.

use std::path::PathBuf;

use super::EpubFilter;

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

/// Semantic result of checking one EPUB against a metadata filter.
pub(super) enum EpubFilterCheck {
    Matched,
    Rejected,
}

/// Semantic result of checking one EPUB during metadata deduplication.
pub(super) enum EpubDeduplicationCheck {
    Unique,
    Duplicate,
}

/// Sole internal authority for emitting Document selection observer facts.
pub(super) struct DocumentSelectionLifecycle<'observer> {
    observer: &'observer mut dyn DocumentSelectionObserver,
}

impl<'observer> DocumentSelectionLifecycle<'observer> {
    /// Creates one lifecycle authority for a complete Document selection call.
    pub(super) fn new(observer: &'observer mut dyn DocumentSelectionObserver) -> Self {
        Self { observer }
    }

    /// Emits one selection-level diagnostic outside an active phase.
    pub(super) fn diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        self.observer.on_document_selection_diagnostic(diagnostic);
    }

    /// Runs the scanning body with structurally managed lifecycle snapshots.
    ///
    /// An inactive phase still runs its body but emits nothing. A normally
    /// returning active body always emits one initial and one final snapshot;
    /// panic unwinding intentionally emits no synthetic final snapshot.
    pub(super) fn scanning<R>(
        &mut self,
        active: bool,
        scope: DocumentSelectionScanScope,
        body: impl FnOnce(&mut ScanningProgress<'_>) -> R,
    ) -> R {
        if active {
            self.observer
                .on_document_selection_progress(DocumentSelectionProgress::Scanning {
                    scope,
                    discovered: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                });
        }

        // Keep the reporter reborrow scoped so the final snapshot can use the observer again.
        let (result, discovered) = {
            let mut progress = ScanningProgress {
                observer: &mut *self.observer,
                active,
                scope,
                discovered: 0,
            };
            let result = body(&mut progress);
            (result, progress.discovered)
        };

        if active {
            self.observer
                .on_document_selection_progress(DocumentSelectionProgress::Scanning {
                    scope,
                    discovered,
                    status: DocumentSelectionPhaseStatus::Finished,
                });
        }

        result
    }

    /// Runs the EPUB filtering body with managed counters and lifecycle snapshots.
    ///
    /// An inactive phase remains silent while its body still computes the normal
    /// selection result. Panic unwinding intentionally skips the final snapshot.
    pub(super) fn filtering<R>(
        &mut self,
        active: bool,
        filter: &EpubFilter,
        total: usize,
        body: impl FnOnce(&mut EpubFilteringProgress<'_>) -> R,
    ) -> R {
        if active {
            self.observer.on_document_selection_progress(
                DocumentSelectionProgress::FilteringEpubs {
                    filter: filter.clone(),
                    checked: 0,
                    total,
                    matching: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                },
            );
        }

        // Keep the reporter reborrow scoped so the final snapshot can use the observer again.
        let (result, checked, matching) = {
            let mut progress = EpubFilteringProgress {
                observer: &mut *self.observer,
                active,
                filter,
                total,
                checked: 0,
                matching: 0,
            };
            let result = body(&mut progress);
            (result, progress.checked, progress.matching)
        };

        if active {
            // A final snapshot is only valid after every known work item reported an outcome.
            assert_eq!(checked, total, "filtering phase did not check every EPUB");
            self.observer.on_document_selection_progress(
                DocumentSelectionProgress::FilteringEpubs {
                    filter: filter.clone(),
                    checked,
                    total,
                    matching,
                    status: DocumentSelectionPhaseStatus::Finished,
                },
            );
        }

        result
    }

    /// Runs the EPUB deduplication body with managed counters and lifecycle snapshots.
    ///
    /// An inactive phase remains silent while its body still computes the normal
    /// selection result. Panic unwinding intentionally skips the final snapshot.
    pub(super) fn deduplicating<R>(
        &mut self,
        active: bool,
        total: usize,
        body: impl FnOnce(&mut EpubDeduplicationProgress<'_>) -> R,
    ) -> R {
        if active {
            self.observer.on_document_selection_progress(
                DocumentSelectionProgress::DeduplicatingEpubs {
                    checked: 0,
                    total,
                    duplicates_found: 0,
                    unique_remaining: 0,
                    status: DocumentSelectionPhaseStatus::Running,
                },
            );
        }

        // Keep the reporter reborrow scoped so the final snapshot can use the observer again.
        let (result, checked, duplicates_found, unique_remaining) = {
            let mut progress = EpubDeduplicationProgress {
                observer: &mut *self.observer,
                active,
                total,
                checked: 0,
                duplicates_found: 0,
                unique_remaining: 0,
            };
            let result = body(&mut progress);
            (
                result,
                progress.checked,
                progress.duplicates_found,
                progress.unique_remaining,
            )
        };

        if active {
            // A final snapshot is only valid after every known work item reported an outcome.
            assert_eq!(
                checked, total,
                "deduplication phase did not check every EPUB"
            );
            self.observer.on_document_selection_progress(
                DocumentSelectionProgress::DeduplicatingEpubs {
                    checked,
                    total,
                    duplicates_found,
                    unique_remaining,
                    status: DocumentSelectionPhaseStatus::Finished,
                },
            );
        }

        result
    }
}

/// Phase-specific reporter for document discovery observations.
pub(super) struct ScanningProgress<'observer> {
    observer: &'observer mut dyn DocumentSelectionObserver,
    active: bool,
    scope: DocumentSelectionScanScope,
    discovered: usize,
}

impl ScanningProgress<'_> {
    /// Records one discovered document and emits the resulting running snapshot.
    pub(super) fn document_discovered(&mut self) {
        self.discovered += 1;
        if self.active {
            self.observer
                .on_document_selection_progress(DocumentSelectionProgress::Scanning {
                    scope: self.scope,
                    discovered: self.discovered,
                    status: DocumentSelectionPhaseStatus::Running,
                });
        }
    }
}

/// Phase-specific reporter for EPUB filtering observations and diagnostics.
pub(super) struct EpubFilteringProgress<'phase> {
    observer: &'phase mut dyn DocumentSelectionObserver,
    active: bool,
    filter: &'phase EpubFilter,
    total: usize,
    checked: usize,
    matching: usize,
}

impl EpubFilteringProgress<'_> {
    /// Emits one diagnostic in its existing position within filtering progress.
    pub(super) fn diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        if self.active {
            self.observer.on_document_selection_diagnostic(diagnostic);
        }
    }

    /// Records one filtering outcome and derives the next monotonic snapshot.
    pub(super) fn record_check(&mut self, outcome: EpubFilterCheck) {
        assert!(
            self.checked < self.total,
            "filtering phase recorded more checks than EPUBs"
        );
        self.checked += 1;
        if matches!(outcome, EpubFilterCheck::Matched) {
            self.matching += 1;
        }

        if self.active {
            self.observer.on_document_selection_progress(
                DocumentSelectionProgress::FilteringEpubs {
                    filter: self.filter.clone(),
                    checked: self.checked,
                    total: self.total,
                    matching: self.matching,
                    status: DocumentSelectionPhaseStatus::Running,
                },
            );
        }
    }
}

/// Phase-specific reporter for EPUB deduplication observations and diagnostics.
pub(super) struct EpubDeduplicationProgress<'observer> {
    observer: &'observer mut dyn DocumentSelectionObserver,
    active: bool,
    total: usize,
    checked: usize,
    duplicates_found: usize,
    unique_remaining: usize,
}

impl EpubDeduplicationProgress<'_> {
    /// Emits one diagnostic in its existing position within deduplication progress.
    pub(super) fn diagnostic(&mut self, diagnostic: DocumentSelectionDiagnostic) {
        if self.active {
            self.observer.on_document_selection_diagnostic(diagnostic);
        }
    }

    /// Records one deduplication outcome and derives the next monotonic snapshot.
    pub(super) fn record_check(&mut self, outcome: EpubDeduplicationCheck) {
        assert!(
            self.checked < self.total,
            "deduplication phase recorded more checks than EPUBs"
        );
        self.checked += 1;
        match outcome {
            EpubDeduplicationCheck::Unique => self.unique_remaining += 1,
            EpubDeduplicationCheck::Duplicate => self.duplicates_found += 1,
        }

        if self.active {
            self.observer.on_document_selection_progress(
                DocumentSelectionProgress::DeduplicatingEpubs {
                    checked: self.checked,
                    total: self.total,
                    duplicates_found: self.duplicates_found,
                    unique_remaining: self.unique_remaining,
                    status: DocumentSelectionPhaseStatus::Running,
                },
            );
        }
    }
}
