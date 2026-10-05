//! Progress lifecycle and diagnostics for Document selection.
//!
//! Document selection reports into the Extraction run observation stream
//! directly. This module owns the phase lifecycle — which phases are active,
//! their counters, and the guarantee that an active phase emits one initial
//! running observation and exactly one finished observation. The EPUB filtering
//! and deduplication phases also own their check loop, so each EPUB is recorded
//! exactly once by construction.
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

use super::{EpubCandidate, EpubFilter};

/// Emits one observation only while its phase is active.
///
/// The silence gate for Document discovery's four phase-scoped emissions, so an
/// inactive discovery phase cannot be made to speak by one site forgetting to
/// check. The EPUB filtering and deduplication phases need no gate: the
/// lifecycle runs their check loop itself and returns before emitting anything
/// when there is no EPUB to check, which is exactly when they are inactive.
///
/// The lifecycle diagnostics are not phase-scoped at all — they are emitted
/// outside any phase, and `DocumentSelectionLifecycle` holds no active flag to
/// pass here.
///
/// The observation is built before the flag is tested, so a silent phase still
/// pays for constructing what it discards. That is bounded to two lifecycle
/// snapshots: discovery is inactive exactly when no input was requested, so no
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
    /// The EPUB matched and continues through selection as this candidate.
    Matched(EpubCandidate),
    Rejected,
}

/// Semantic result of checking one EPUB during declaration deduplication.
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

    /// Checks every EPUB against an EPUB filter and returns the matching candidates.
    ///
    /// The phase is active exactly when there is at least one EPUB: an empty phase emits
    /// nothing. An active phase emits one initial running observation, one running
    /// observation after each check, and one finished observation. Diagnostics reported
    /// by `check` precede the running observation that records its outcome, and carry
    /// the filtering purpose without `check` naming it. Panic unwinding intentionally
    /// skips the final observation.
    pub(super) fn filtering(
        &mut self,
        filter: &EpubFilter,
        epubs: Vec<EpubCandidate>,
        mut check: impl FnMut(EpubCandidate, &mut EpubDeclarationDiagnostics<'_>) -> EpubFilterCheck,
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
                &mut EpubDeclarationDiagnostics::new(
                    &mut *self.observer,
                    EpubDeclarationPurpose::Filtering,
                ),
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
    /// by `check` precede the running observation that records its outcome, and carry
    /// the deduplication purpose without `check` naming it. Panic unwinding
    /// intentionally skips the final observation.
    pub(super) fn deduplicating(
        &mut self,
        epubs: Vec<EpubCandidate>,
        mut check: impl FnMut(
            EpubCandidate,
            &mut EpubDeclarationDiagnostics<'_>,
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
                &mut EpubDeclarationDiagnostics::new(
                    &mut *self.observer,
                    EpubDeclarationPurpose::Deduplication,
                ),
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

/// Diagnostic-only reporter handed to each per-EPUB check.
///
/// Holding it grants no way to emit or advance progress, so a check cannot record
/// its own outcome or disturb the phase's observation order (ADR-0012). Its one
/// fact carries the purpose of the phase that created it, so a filtering check
/// cannot report a deduplication purpose or the reverse (ADR-0009).
pub(super) struct EpubDeclarationDiagnostics<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
    purpose: EpubDeclarationPurpose,
}

impl<'observer> EpubDeclarationDiagnostics<'observer> {
    /// Wraps one observer reborrow and its phase's purpose for a single check.
    fn new(
        observer: &'observer mut dyn ExtractionRunObserver,
        purpose: EpubDeclarationPurpose,
    ) -> Self {
        Self { observer, purpose }
    }

    /// Emits one unreadable-declarations fact at its position within the phase's progress.
    pub(super) fn declarations_unreadable(&mut self, path: PathBuf, detail: String) {
        self.observer
            .on_observation(ExtractionRunObservation::UnreadableEpubDeclarations {
                path,
                purpose: self.purpose,
                detail,
            });
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
