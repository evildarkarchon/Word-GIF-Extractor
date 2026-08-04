//! Progress lifecycle and diagnostics for Document selection.
//!
//! Document selection reports into the Extraction run observation stream
//! directly. This module owns the phase lifecycle — which phases are active,
//! their counters, and the guarantee that an active phase emits one initial
//! running observation and exactly one finished observation — but it owns no
//! vocabulary of its own.

use crate::extraction_run_observation::{
    DocumentDiscoveryScope, ExtractionRunObservation, ExtractionRunObserver,
};

use super::EpubFilter;

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

/// Sole internal authority for emitting Document selection observations.
pub(super) struct DocumentSelectionLifecycle<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
}

impl<'observer> DocumentSelectionLifecycle<'observer> {
    /// Creates one lifecycle authority for a complete Document selection call.
    pub(super) fn new(observer: &'observer mut dyn ExtractionRunObserver) -> Self {
        Self { observer }
    }

    /// Emits one selection-level diagnostic outside an active phase.
    ///
    /// Callers pass one of the non-fatal diagnostic variants; the lifecycle does
    /// not inspect which, because its only job is where the fact is emitted.
    pub(super) fn diagnostic(&mut self, diagnostic: ExtractionRunObservation) {
        self.observer.on_observation(diagnostic);
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
        if active {
            self.observer
                .on_observation(ExtractionRunObservation::DiscoveringDocuments {
                    scope,
                    discovered: 0,
                });
        }

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

        if active {
            self.observer
                .on_observation(ExtractionRunObservation::DocumentDiscoveryFinished {
                    scope,
                    discovered,
                });
        }

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
        if active {
            self.observer
                .on_observation(ExtractionRunObservation::FilteringEpubs {
                    title: filter.title.clone(),
                    author: filter.author.clone(),
                    checked: 0,
                    total,
                    matching: 0,
                });
        }

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
        if active {
            self.observer
                .on_observation(ExtractionRunObservation::DeduplicatingEpubs {
                    checked: 0,
                    total,
                    duplicates_found: 0,
                    unique_remaining: 0,
                });
        }

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
    /// Emits one diagnostic at its encounter position inside active discovery.
    pub(super) fn diagnostic(&mut self, diagnostic: ExtractionRunObservation) {
        if self.active {
            self.observer.on_observation(diagnostic);
        }
    }

    /// Records one discovered document and emits the resulting running observation.
    pub(super) fn document_discovered(&mut self) {
        self.discovered += 1;
        if self.active {
            self.observer
                .on_observation(ExtractionRunObservation::DiscoveringDocuments {
                    scope: self.scope,
                    discovered: self.discovered,
                });
        }
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
    /// Emits one diagnostic in its existing position within filtering progress.
    pub(super) fn diagnostic(&mut self, diagnostic: ExtractionRunObservation) {
        if self.active {
            self.observer.on_observation(diagnostic);
        }
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

        if self.active {
            self.observer
                .on_observation(ExtractionRunObservation::FilteringEpubs {
                    title: self.filter.title.clone(),
                    author: self.filter.author.clone(),
                    checked: self.checked,
                    total: self.total,
                    matching: self.matching,
                });
        }
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
    /// Emits one diagnostic in its existing position within deduplication progress.
    pub(super) fn diagnostic(&mut self, diagnostic: ExtractionRunObservation) {
        if self.active {
            self.observer.on_observation(diagnostic);
        }
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

        if self.active {
            self.observer
                .on_observation(ExtractionRunObservation::DeduplicatingEpubs {
                    checked: self.checked,
                    total: self.total,
                    duplicates_found: self.duplicates_found,
                    unique_remaining: self.unique_remaining,
                });
        }
    }
}
