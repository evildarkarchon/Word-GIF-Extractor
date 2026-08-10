//! Progress lifecycle and diagnostics for Document selection.
//!
//! Document selection reports into the Extraction run observation stream
//! directly. This module owns the phase lifecycle — which phases are active,
//! their counters, and the guarantee that an active phase emits one initial
//! running observation and exactly one finished observation.
//!
//! It defines no observation types; the Extraction run observation module owns
//! every type a fact is carried in. It does name each fact it emits, one method
//! per fact rather than one method taking any observation, so that neither a
//! non-diagnostic nor a mismatched EPUB declaration purpose can reach the stream
//! from here (ADR-0009, superseding ADR-0004's deliberate ignorance of which
//! diagnostic was being emitted).

use std::path::PathBuf;

use crate::extraction_run_observation::{
    DocumentDiscoveryScope, EpubDeclarationPurpose, ExtractionRunObservation, ExtractionRunObserver,
};

use super::EpubFilter;

/// Emits one observation only while its phase is active.
///
/// The silence gate for ten of the module's twelve phase-scoped emissions, so
/// an inactive phase cannot be made to speak by one site forgetting to check.
/// Two sites stay inline rather than calling here: the final observations of
/// filtering and deduplication are preceded by a completeness assertion that
/// must share their guard, and splitting the two apart would test the same flag
/// twice and separate the assertion from the emission it justifies.
///
/// The two lifecycle diagnostics are not phase-scoped at all — they are emitted
/// before any phase opens, and `DocumentSelectionLifecycle` holds no active flag
/// to pass here.
///
/// The observation is built before the flag is tested, so a silent phase still
/// pays for constructing what it discards. That is bounded to four lifecycle
/// snapshots: a phase is inactive exactly when its work set is empty, so no
/// per-item emission is reachable while silent.
fn emit_when(
    observer: &mut dyn ExtractionRunObserver,
    active: bool,
    observation: ExtractionRunObservation,
) {
    if active {
        observer.on_observation(observation);
    }
}

/// Semantic result of checking one EPUB against an EPUB filter.
pub(super) enum EpubFilterCheck {
    Matched,
    Rejected,
}

/// Semantic result of checking one EPUB during declaration deduplication.
pub(super) enum EpubDeduplicationCheck {
    Unique,
    Duplicate,
}

/// Sole internal authority for emitting Document selection observations.
pub(super) struct DocumentSelectionLifecycle<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
}

impl<'observer> DocumentSelectionLifecycle<'observer> {
    /// Creates one lifecycle authority for a complete Document selection call.
    pub(super) fn new(observer: &'observer mut dyn ExtractionRunObserver) -> Self {
        Self { observer }
    }

    /// Emits one absent requested input, outside any phase and never silenced.
    ///
    /// Requested-input classification runs before the discovery phase opens, so
    /// this fact has no phase to be gated by; `DocumentSelectionLifecycle` holds
    /// no active flag, which is what keeps it that way.
    pub(super) fn missing_input(&mut self, path: PathBuf) {
        self.observer
            .on_observation(ExtractionRunObservation::MissingInput { path });
    }

    /// Emits one requested input skipped for ineligibility, outside any phase.
    ///
    /// Eligibility is decided after discovery has closed and before filtering
    /// opens, so like the two facts around it this one has no phase to be gated
    /// by. It is the only diagnostic in this module that reports a decision
    /// rather than a failure to observe; see ADR-0011.
    pub(super) fn skipped_non_epub_input(&mut self, path: PathBuf) {
        self.observer
            .on_observation(ExtractionRunObservation::SkippedNonEpubInput { path });
    }

    /// Emits one requested-input inspection failure, outside any phase and never silenced.
    ///
    /// Shares its name with the `DocumentDiscoveryProgress` method reporting the
    /// same fact: which type holds the method is the statement about where the
    /// fact lands, before the phase opens or at its encounter position within it.
    pub(super) fn discovery_failed(&mut self, path: PathBuf, detail: String) {
        self.observer
            .on_observation(ExtractionRunObservation::DocumentDiscoveryFailed { path, detail });
    }

    /// Runs the discovery body with structurally managed lifecycle observations.
    ///
    /// An inactive phase still runs its body but emits nothing. A normally
    /// returning active body always emits one initial and one final observation;
    /// panic unwinding intentionally emits no synthetic final observation.
    pub(super) fn discovering<R>(
        &mut self,
        active: bool,
        scope: DocumentDiscoveryScope,
        body: impl FnOnce(&mut DocumentDiscoveryProgress<'_>) -> R,
    ) -> R {
        emit_when(
            self.observer,
            active,
            ExtractionRunObservation::DiscoveringDocuments {
                scope,
                discovered: 0,
            },
        );

        // Keep the reporter reborrow scoped so the final observation can use the observer again.
        let (result, discovered) = {
            let mut progress = DocumentDiscoveryProgress {
                observer: &mut *self.observer,
                active,
                scope,
                discovered: 0,
            };
            let result = body(&mut progress);
            (result, progress.discovered)
        };

        emit_when(
            self.observer,
            active,
            ExtractionRunObservation::DocumentDiscoveryFinished { scope, discovered },
        );

        result
    }

    /// Runs the EPUB filtering body with managed counters and lifecycle observations.
    ///
    /// An inactive phase remains silent while its body still computes the normal
    /// selection result. Panic unwinding intentionally skips the final observation.
    pub(super) fn filtering<R>(
        &mut self,
        active: bool,
        filter: &EpubFilter,
        total: usize,
        body: impl FnOnce(&mut EpubFilteringProgress<'_>) -> R,
    ) -> R {
        emit_when(
            self.observer,
            active,
            ExtractionRunObservation::FilteringEpubs {
                title: filter.title.clone(),
                author: filter.author.clone(),
                checked: 0,
                total,
                matching: 0,
            },
        );

        // Keep the reporter reborrow scoped so the final observation can use the observer again.
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
            // A final observation is only valid after every known work item reported an outcome.
            assert_eq!(checked, total, "filtering phase did not check every EPUB");
            self.observer
                .on_observation(ExtractionRunObservation::EpubFilteringFinished {
                    checked,
                    total,
                    matching,
                });
        }

        result
    }

    /// Runs the EPUB deduplication body with managed counters and lifecycle observations.
    ///
    /// An inactive phase remains silent while its body still computes the normal
    /// selection result. Panic unwinding intentionally skips the final observation.
    pub(super) fn deduplicating<R>(
        &mut self,
        active: bool,
        total: usize,
        body: impl FnOnce(&mut EpubDeduplicationProgress<'_>) -> R,
    ) -> R {
        emit_when(
            self.observer,
            active,
            ExtractionRunObservation::DeduplicatingEpubs {
                checked: 0,
                total,
                duplicates_found: 0,
                unique_remaining: 0,
            },
        );

        // Keep the reporter reborrow scoped so the final observation can use the observer again.
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
            // A final observation is only valid after every known work item reported an outcome.
            assert_eq!(
                checked, total,
                "deduplication phase did not check every EPUB"
            );
            self.observer
                .on_observation(ExtractionRunObservation::EpubDeduplicationFinished {
                    checked,
                    total,
                    duplicates_found,
                    unique_remaining,
                });
        }

        result
    }
}

/// Phase-specific reporter for document discovery observations.
pub(super) struct DocumentDiscoveryProgress<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
    active: bool,
    scope: DocumentDiscoveryScope,
    discovered: usize,
}

impl DocumentDiscoveryProgress<'_> {
    /// Emits one inspection failure at its encounter position inside active discovery.
    pub(super) fn discovery_failed(&mut self, path: PathBuf, detail: String) {
        emit_when(
            self.observer,
            self.active,
            ExtractionRunObservation::DocumentDiscoveryFailed { path, detail },
        );
    }

    /// Records one discovered document and emits the resulting running observation.
    pub(super) fn document_discovered(&mut self) {
        self.discovered += 1;
        emit_when(
            self.observer,
            self.active,
            ExtractionRunObservation::DiscoveringDocuments {
                scope: self.scope,
                discovered: self.discovered,
            },
        );
    }
}

/// Phase-specific reporter for EPUB filtering observations and diagnostics.
pub(super) struct EpubFilteringProgress<'phase> {
    observer: &'phase mut dyn ExtractionRunObserver,
    active: bool,
    filter: &'phase EpubFilter,
    total: usize,
    checked: usize,
    matching: usize,
}

impl EpubFilteringProgress<'_> {
    /// Emits one unreadable-declarations fact in its position within filtering progress.
    ///
    /// The phase supplies its own purpose, so a filtering call site cannot report
    /// a deduplication purpose.
    pub(super) fn declarations_unreadable(&mut self, path: PathBuf, detail: String) {
        emit_when(
            self.observer,
            self.active,
            ExtractionRunObservation::UnreadableEpubDeclarations {
                path,
                purpose: EpubDeclarationPurpose::Filtering,
                detail,
            },
        );
    }

    /// Records one filtering outcome and derives the next monotonic observation.
    pub(super) fn record_check(&mut self, outcome: EpubFilterCheck) {
        assert!(
            self.checked < self.total,
            "filtering phase recorded more checks than EPUBs"
        );
        self.checked += 1;
        if matches!(outcome, EpubFilterCheck::Matched) {
            self.matching += 1;
        }

        emit_when(
            self.observer,
            self.active,
            ExtractionRunObservation::FilteringEpubs {
                title: self.filter.title.clone(),
                author: self.filter.author.clone(),
                checked: self.checked,
                total: self.total,
                matching: self.matching,
            },
        );
    }
}

/// Phase-specific reporter for EPUB deduplication observations and diagnostics.
pub(super) struct EpubDeduplicationProgress<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
    active: bool,
    total: usize,
    checked: usize,
    duplicates_found: usize,
    unique_remaining: usize,
}

impl EpubDeduplicationProgress<'_> {
    /// Emits one unreadable-declarations fact in its position within deduplication progress.
    ///
    /// The phase supplies its own purpose, so a deduplication call site cannot
    /// report a filtering purpose.
    pub(super) fn declarations_unreadable(&mut self, path: PathBuf, detail: String) {
        emit_when(
            self.observer,
            self.active,
            ExtractionRunObservation::UnreadableEpubDeclarations {
                path,
                purpose: EpubDeclarationPurpose::Deduplication,
                detail,
            },
        );
    }

    /// Records one deduplication outcome and derives the next monotonic observation.
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

        emit_when(
            self.observer,
            self.active,
            ExtractionRunObservation::DeduplicatingEpubs {
                checked: self.checked,
                total: self.total,
                duplicates_found: self.duplicates_found,
                unique_remaining: self.unique_remaining,
            },
        );
    }
}
