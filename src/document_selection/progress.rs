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

use crate::epub_declarations::EpubDeclarationSource;
use crate::extraction_run_observation::{
    DocumentDiscoveryScope, EpubDeclarationPurpose, ExtractionRunObservation, ExtractionRunObserver,
};

use super::document_identity::{DocumentIdentity, EpubFilterTerms};
use super::{EpubCandidate, EpubFilter};

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
    /// Every call opens the phase: the caller decides whether discovery runs at
    /// all, and returns before calling this when no input was requested. A
    /// normally returning body always emits one initial and one final
    /// observation; panic unwinding intentionally emits no synthetic final
    /// observation. The body takes a progress reporter rather than items because
    /// discovery has no known total (ADR-0012).
    pub(super) fn discovering<R>(
        &mut self,
        scope: DocumentDiscoveryScope,
        body: impl FnOnce(&mut DocumentDiscoveryProgress<'_>) -> R,
    ) -> R {
        self.observer
            .on_observation(ExtractionRunObservation::DiscoveringDocuments {
                scope,
                discovered: 0,
            });

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

        self.observer
            .on_observation(ExtractionRunObservation::DocumentDiscoveryFinished {
                scope,
                discovered,
            });

        result
    }

    /// Filters EPUB candidates by title and creator declarations and returns the matches.
    ///
    /// The phase owns its whole activation rule: it is active exactly when `filter`
    /// requests something and there is at least one EPUB. An empty filter returns
    /// `epubs` unchanged, and an empty filter or an empty EPUB list emits nothing.
    ///
    /// Every EPUB's declarations are acquired from `declarations`, and the EPUB is kept
    /// only when its Document identity matches the filter terms. A kept EPUB retains
    /// the declarations it was matched with, for deduplication to reuse (ADR-0002).
    /// `filter` is taken raw rather than as terms, because the running observation
    /// reports the user's spelling, not the case-folded terms.
    ///
    /// An active phase emits one initial running observation, one running observation
    /// after each EPUB, and one finished observation. An EPUB's unreadable-declarations
    /// diagnostic precedes the running observation that records it. Panic unwinding
    /// intentionally skips the final observation.
    pub(super) fn filtering(
        &mut self,
        filter: &EpubFilter,
        epubs: Vec<EpubCandidate>,
        declarations: &dyn EpubDeclarationSource,
    ) -> Vec<EpubCandidate> {
        if filter.is_empty() || epubs.is_empty() {
            return epubs;
        }
        let total = epubs.len();
        let terms = EpubFilterTerms::new(filter);
        let running = |checked, matching| ExtractionRunObservation::FilteringEpubs {
            title: filter.title.clone(),
            author: filter.author.clone(),
            checked,
            total,
            matching,
        };

        self.observer.on_observation(running(0, 0));
        let mut matching_epubs = Vec::new();
        for (index, EpubCandidate { path, .. }) in epubs.into_iter().enumerate() {
            match declarations.acquire(&path) {
                Ok(declarations)
                    if DocumentIdentity::of_epub_declarations(Some(&declarations), &path)
                        .matches(&terms) =>
                {
                    matching_epubs.push(EpubCandidate {
                        path,
                        epub_declarations: Some(declarations),
                    });
                }
                Ok(_) => {} // File doesn't match filter, skip.
                Err(error) => {
                    // Filtering cannot accept an EPUB whose requested declarations are unreadable.
                    self.observer.on_observation(
                        ExtractionRunObservation::UnreadableEpubDeclarations {
                            path,
                            purpose: EpubDeclarationPurpose::Filtering,
                            detail: error.to_string(),
                        },
                    );
                }
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
    scope: DocumentDiscoveryScope,
    discovered: usize,
}

impl DocumentDiscoveryProgress<'_> {
    /// Emits one inspection failure at its encounter position inside active discovery.
    pub(super) fn discovery_failed(&mut self, path: PathBuf, detail: String) {
        self.observer
            .on_observation(ExtractionRunObservation::DocumentDiscoveryFailed { path, detail });
    }

    /// Records one discovered document and emits the resulting running observation.
    pub(super) fn document_discovered(&mut self) {
        self.discovered += 1;
        self.observer
            .on_observation(ExtractionRunObservation::DiscoveringDocuments {
                scope: self.scope,
                discovered: self.discovered,
            });
    }
}
