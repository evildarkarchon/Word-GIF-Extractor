//! Progress lifecycle and diagnostics for Document selection.
//!
//! Document selection reports into the Extraction run observation stream
//! directly. This module owns the phase lifecycle — which phases are active,
//! their counters, and the guarantee that an active phase emits one initial
//! running observation and exactly one finished observation — but it owns no
//! vocabulary of its own. The EPUB filtering and deduplication phases also own
//! their check loop, so each EPUB is recorded exactly once by construction.

use crate::extraction_run_observation::{
    DocumentDiscoveryScope, ExtractionRunObservation, ExtractionRunObserver,
};

use super::{EpubCandidate, EpubFilter};

/// Semantic result of checking one EPUB against a metadata filter.
pub(super) enum EpubFilterCheck {
    /// The EPUB matched and continues through selection as this candidate.
    Matched(EpubCandidate),
    Rejected,
}

/// Semantic result of checking one EPUB during metadata deduplication.
pub(super) enum EpubDeduplicationCheck {
    /// The EPUB is the first with its key and continues through selection as this candidate.
    Unique(EpubCandidate),
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
        DocumentSelectionDiagnostics::new(&mut *self.observer).report(diagnostic);
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

    /// Checks every EPUB against a metadata filter and returns the matching candidates.
    ///
    /// The phase is active exactly when there is at least one EPUB: an empty phase emits
    /// nothing. An active phase emits one initial running observation, one running
    /// observation after each check, and one finished observation. Diagnostics reported
    /// by `check` precede the running observation that records its outcome. Panic
    /// unwinding intentionally skips the final observation.
    pub(super) fn filtering(
        &mut self,
        filter: &EpubFilter,
        epubs: Vec<EpubCandidate>,
        mut check: impl FnMut(EpubCandidate, &mut DocumentSelectionDiagnostics<'_>) -> EpubFilterCheck,
    ) -> Vec<EpubCandidate> {
        let total = epubs.len();
        if total == 0 {
            return Vec::new();
        }
        let running = |checked, matching| ExtractionRunObservation::FilteringEpubs {
            title: filter.title.clone(),
            author: filter.author.clone(),
            checked,
            total,
            matching,
        };

        self.observer.on_observation(running(0, 0));
        let mut matching_epubs = Vec::new();
        for (index, epub) in epubs.into_iter().enumerate() {
            // A fresh reborrow per check lets the running observation after it use the observer again.
            let outcome = check(
                epub,
                &mut DocumentSelectionDiagnostics::new(&mut *self.observer),
            );
            if let EpubFilterCheck::Matched(epub) = outcome {
                matching_epubs.push(epub);
            }
            self.observer
                .on_observation(running(index + 1, matching_epubs.len()));
        }
        self.observer
            .on_observation(ExtractionRunObservation::EpubFilteringFinished {
                checked: total,
                total,
                matching: matching_epubs.len(),
            });

        matching_epubs
    }

    /// Checks every EPUB for a duplicate key and returns the unique candidates.
    ///
    /// The phase is active exactly when there is at least one EPUB: an empty phase emits
    /// nothing. An active phase emits one initial running observation, one running
    /// observation after each check, and one finished observation. Diagnostics reported
    /// by `check` precede the running observation that records its outcome. Panic
    /// unwinding intentionally skips the final observation.
    pub(super) fn deduplicating(
        &mut self,
        epubs: Vec<EpubCandidate>,
        mut check: impl FnMut(
            EpubCandidate,
            &mut DocumentSelectionDiagnostics<'_>,
        ) -> EpubDeduplicationCheck,
    ) -> Vec<EpubCandidate> {
        let total = epubs.len();
        if total == 0 {
            return Vec::new();
        }
        let running = |checked, duplicates_found, unique_remaining| {
            ExtractionRunObservation::DeduplicatingEpubs {
                checked,
                total,
                duplicates_found,
                unique_remaining,
            }
        };

        self.observer.on_observation(running(0, 0, 0));
        let mut unique_epubs = Vec::new();
        let mut duplicates_found = 0;
        for (index, epub) in epubs.into_iter().enumerate() {
            // A fresh reborrow per check lets the running observation after it use the observer again.
            match check(
                epub,
                &mut DocumentSelectionDiagnostics::new(&mut *self.observer),
            ) {
                EpubDeduplicationCheck::Unique(epub) => unique_epubs.push(epub),
                EpubDeduplicationCheck::Duplicate => duplicates_found += 1,
            }
            self.observer
                .on_observation(running(index + 1, duplicates_found, unique_epubs.len()));
        }
        self.observer
            .on_observation(ExtractionRunObservation::EpubDeduplicationFinished {
                checked: total,
                total,
                duplicates_found,
                unique_remaining: unique_epubs.len(),
            });

        unique_epubs
    }
}

/// Diagnostic-only reporter handed to phase bodies and per-EPUB checks.
///
/// It is the single place a Document selection diagnostic is emitted. Holding it
/// grants no way to emit or advance progress, so a check cannot record its own
/// outcome or disturb the phase's observation order.
pub(super) struct DocumentSelectionDiagnostics<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
}

impl<'observer> DocumentSelectionDiagnostics<'observer> {
    /// Wraps one observer reborrow for the duration of a single report site.
    fn new(observer: &'observer mut dyn ExtractionRunObserver) -> Self {
        Self { observer }
    }

    /// Emits one diagnostic at its encounter position.
    ///
    /// Callers pass one of the non-fatal diagnostic variants; ADR-0004 records why
    /// the parameter is the whole observation type rather than a narrower one.
    pub(super) fn report(&mut self, diagnostic: ExtractionRunObservation) {
        self.observer.on_observation(diagnostic);
    }
}

/// Phase-specific reporter for document discovery observations.
pub(super) struct DocumentDiscoveryProgress<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
    scope: DocumentDiscoveryScope,
    discovered: usize,
}

impl DocumentDiscoveryProgress<'_> {
    /// Emits one diagnostic at its encounter position inside active discovery.
    ///
    /// Discovery only reports while walking requested inputs, so it can never
    /// report during an inactive phase, which has none.
    pub(super) fn diagnostic(&mut self, diagnostic: ExtractionRunObservation) {
        DocumentSelectionDiagnostics::new(&mut *self.observer).report(diagnostic);
    }

    /// Records one discovered document and emits the resulting running observation.
    ///
    /// Like diagnostics, this only happens while walking requested inputs, so the
    /// phase is always active when it does.
    pub(super) fn document_discovered(&mut self) {
        self.discovered += 1;
        self.observer
            .on_observation(ExtractionRunObservation::DiscoveringDocuments {
                scope: self.scope,
                discovered: self.discovered,
            });
    }
}
