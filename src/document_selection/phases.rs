//! Document selection's phases, their decisions, and every observation they emit.
//!
//! Document selection reports into the Extraction run observation stream
//! directly, and this module is the only place that reports. Each phase is one
//! method that does its whole job. Discovery runs a caller-supplied walk,
//! because it has no known total. EPUB filtering makes the filter match, and
//! EPUB deduplication makes the dedupe-key decision. Each EPUB phase acquires or
//! reuses declarations itself. Each EPUB phase owns its activation rule, while
//! discovery is kept silent by its caller returning before it when no input was
//! requested. A phase owns its counters, and every phase that opens emits one
//! initial running observation and exactly one finished observation. Each EPUB phase records every EPUB exactly
//! once, and emits that EPUB's diagnostics ahead of the running observation
//! that records it. One loop body per phase, written that way, is what holds
//! that order (ADR-0024).
//!
//! It defines no observation types; the Extraction run observation module owns
//! every type a fact is carried in. It does name each fact it emits, one method
//! per fact rather than one method taking any observation, so that a
//! non-diagnostic cannot reach the stream from here (ADR-0009, superseding ADR-0004's deliberate ignorance of which
//! diagnostic was being emitted). Each EPUB phase writes its declaration
//! purpose as a literal at its single emission site. The tests that pin each
//! phase's purpose keep that literal right, not the type system (ADR-0024).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::PathBuf;

use crate::epub_declarations::{EpubDeclarationSource, EpubDeclarations};
use crate::extraction_run_observation::{
    DocumentDiscoveryScope, EpubDeclarationPurpose, ExtractionRunObservation, ExtractionRunObserver,
};

use super::document_identity::{DedupeKey, DocumentIdentity, EpubFilterTerms};
use super::{EpubCandidate, EpubFilter};

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
    /// this fact belongs to no phase and is emitted whenever it occurs.
    pub(super) fn missing_input(&mut self, path: PathBuf) {
        self.observer
            .on_observation(ExtractionRunObservation::MissingInput { path });
    }

    /// Emits one requested input skipped for ineligibility, outside any phase.
    ///
    /// Eligibility is decided after discovery has closed and before filtering
    /// opens, so like the two facts around it this one belongs to no phase. It is
    /// the only diagnostic in this module that reports a decision
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
    /// discovery has no known total (ADR-0012, kept by ADR-0024).
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

    /// Deduplicates EPUB candidates by their declarations and returns the unique ones.
    ///
    /// Keeps the first EPUB under each duplicate key and skips every later one, naming
    /// the skipped EPUB and the one it duplicates in a Document selection diagnostic.
    /// The key compares the package's unique identifier, creator and title; an EPUB
    /// that declares too little to have one is never a duplicate (see
    /// [`DocumentIdentity::dedupe_key`]). With `keep_duplicates` no EPUB has a key,
    /// so every EPUB is kept.
    ///
    /// The phase is active exactly when there is at least one EPUB: an empty EPUB list
    /// emits nothing. It runs under `keep_duplicates` too, because it is where an EPUB
    /// no filter read gets the declarations its Document identity is built from.
    /// Declarations retained by filtering are reused, and only an EPUB without them is
    /// acquired from `declarations` (ADR-0002); an EPUB whose declarations cannot be
    /// read has no key and is kept.
    ///
    /// An active phase emits one initial running observation, one running observation
    /// after each EPUB, and one finished observation. An EPUB's unreadable-declarations
    /// or skipped-duplicate diagnostic precedes the running observation that records it.
    /// Panic unwinding intentionally skips the final observation.
    pub(super) fn deduplicating(
        &mut self,
        epubs: Vec<EpubCandidate>,
        declarations: &dyn EpubDeclarationSource,
        keep_duplicates: bool,
    ) -> Vec<EpubCandidate> {
        if epubs.is_empty() {
            return epubs;
        }
        let total = epubs.len();
        // Maps each duplicate key already seen to the path of the EPUB kept under it,
        // which the diagnostic for a later duplicate names.
        let mut seen: HashMap<DedupeKey, PathBuf> = HashMap::new();
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
        for (
            index,
            EpubCandidate {
                path,
                epub_declarations,
            },
        ) in epubs.into_iter().enumerate()
        {
            // ADR-0002: declarations retained by filtering are authoritative for the
            // run, so deduplication asks the source only when it has none. From here
            // on, `None` means selection could not read the declarations.
            let epub_declarations =
                match EpubDeclarations::retained_or_acquire(epub_declarations, &path, declarations)
                {
                    Ok(declarations) => Some(declarations),
                    Err(error) => {
                        self.observer.on_observation(
                            ExtractionRunObservation::UnreadableEpubDeclarations {
                                path: path.clone(),
                                purpose: EpubDeclarationPurpose::Deduplication,
                                detail: error.to_string(),
                            },
                        );
                        None
                    }
                };

            let key = if keep_duplicates {
                None
            } else {
                DocumentIdentity::of_epub_declarations(epub_declarations.as_ref(), &path)
                    .dedupe_key(
                        epub_declarations
                            .as_ref()
                            .and_then(EpubDeclarations::identifier),
                    )
            };

            match key.map(|key| seen.entry(key)) {
                Some(Entry::Occupied(kept)) => {
                    duplicates_found += 1;
                    self.observer
                        .on_observation(ExtractionRunObservation::SkippedDuplicateEpub {
                            path,
                            duplicate_of: kept.get().clone(),
                        });
                }
                Some(Entry::Vacant(vacant)) => {
                    vacant.insert(path.clone());
                    unique_epubs.push(EpubCandidate {
                        path,
                        epub_declarations,
                    });
                }
                // No key: this EPUB can be no other's duplicate, so it is always kept.
                None => unique_epubs.push(EpubCandidate {
                    path,
                    epub_declarations,
                }),
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

/// Phase-specific reporter for document discovery observations.
pub(super) struct DocumentDiscoveryProgress<'observer> {
    observer: &'observer mut dyn ExtractionRunObserver,
    scope: DocumentDiscoveryScope,
    discovered: usize,
}

impl DocumentDiscoveryProgress<'_> {
    /// Emits one inspection failure at its encounter position inside the discovery phase.
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
