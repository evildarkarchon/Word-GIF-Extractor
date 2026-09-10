# Concentrate Document discovery entry handling

Status: ready-for-agent

## Problem Statement

Maintainers must currently reason about separate immediate-child and recursive paths to change how Document discovery inspects entries, accepts supported documents, and reports failures. The duplication makes behavior harder to navigate and creates multiple places for fixes to diverge. Recent corrections to linked-directory failures in the in-memory adapter demonstrate the maintenance cost of parallel search handling.

For CLI users, this work must preserve which documents are selected and what an Extraction run reports. A superficially simpler replacement of direct listing with depth-limited traversal would change behavior when a requested directory becomes a file between classification and search. Changes to encounter order could also change which EPUB survives deduplication.

## Solution

Give Document discovery one directory-search interface and one entry-handling loop, while retaining the two existing search mechanisms inside the real adapter. Immediate-child search continues to use direct directory listing; recursive search continues to use recursive traversal. Users receive the same selection behavior, and maintainers gain locality for inspection, candidate acceptance, progress, and diagnostic decisions.

This is a behavior-preserving architectural refactor governed by ADR-0012. Underlying error-detail wording may differ; no other behavior change is authorized by this spec.

## User Stories

1. As a CLI user, I want a non-recursive search to consider only immediate children, so that nested documents do not unexpectedly enter my Extraction run.
2. As a CLI user, I want a recursive search to retain its current scope, so that the same eligible documents remain discoverable.
3. As a CLI user, I want a directly requested supported document to remain discoverable, so that directory-search changes do not alter explicit inputs.
4. As a CLI user, I want a requested directory link to retain its current behavior, so that searching through that link still finds its documents.
5. As a CLI user, I want discovered paths beneath a requested directory link to retain the link's spelling, so that Source identity and derived Output placement remain consistent with my request.
6. As a CLI user, I want nested directory links to retain their current treatment, so that recursive search does not widen into linked directories.
7. As a CLI user, I want nested links to supported files to remain eligible, so that linked documents are not silently lost.
8. As a CLI user, I want broken links to remain distinguishable from genuinely missing requested paths, so that diagnostics explain what could not be inspected.
9. As a CLI user, I want a failed directory opening to remain a diagnostic, so that a search failure is not mistaken for an empty directory.
10. As a CLI user, I want immediate-child search to report failure when a previously classified directory becomes a file, so that a concurrent change is not silently hidden.
11. As a CLI user, I want discovery to continue after an unreadable directory entry, so that later readable documents remain available.
12. As a CLI user, I want independent readable inputs to be processed after another input fails, so that one failed search does not discard the rest of my request.
13. As a CLI user, I want recursive search to abandon a directory branch whose inspection fails or whose kind changes, so that discovery does not report duplicate failures or visit stale descendants.
14. As a CLI user, I want a traversal failure without its own path attributed to the nearest known directory, so that its diagnostic remains useful.
15. As a CLI user, I want each search mechanism's encounter order preserved, so that first-occurrence EPUB deduplication keeps choosing the same candidate under equivalent observations.
16. As a CLI user, I want requested inputs and directory-search hits to retain their distinct origins, so that EPUB-only eligibility diagnostics still apply only to inputs I named.
17. As a CLI user, I want Document selection progress and diagnostic ordering preserved, so that counts, phase completion, and explanations remain consistent.
18. As a CLI user, I want unsupported entries and inactive discovery to retain their existing silence, so that the refactor introduces no extra diagnostic noise.
19. As a maintainer, I want one entry-handling loop in the Document discovery module, so that inspection and reporting decisions have one place to change.
20. As a maintainer, I want one directory-search operation at the Document search surface seam, so that callers learn fewer interaction rules.
21. As a maintainer, I want the real adapter to preserve the two acquisition mechanisms, so that interface simplification does not depend on false behavioral equivalence.
22. As a maintainer, I want immediate-child entries represented without extra directory classification, so that the shared interface does not introduce new observations or failures.
23. As a maintainer, I want the in-memory adapter to exercise the same discovery decisions as production, so that policy tests verify the actual implementation through an existing seam.
24. As a maintainer, I want existing behavioral assertions to survive the refactor, so that test changes do not conceal selection regressions.
25. As a maintainer, I want the reason for retaining two search mechanisms recorded, so that future simplifications do not repeat the rejected substitution.

## Implementation Decisions

- Follow ADR-0012, which is accepted ahead of implementation. It supersedes only ADR-0008's decision to expose direct listing and recursive traversal as separate operations; the other ownership decisions in ADR-0008 remain in force.
- Modify the Document search surface interface to expose one directory-search operation with two choices: immediate children and recursive search. Do not add arbitrary depth settings or new user-facing options.
- Retain followed inspection and non-following inspection as distinct existing operations. Requested-input classification, including the second inspection used to distinguish a missing path from a broken link, stays in Document discovery.
- The real adapter retains direct directory listing for immediate-child search and the existing recursive traversal mechanism for recursive search. A depth-limited recursive traversal is not an acceptable replacement for direct listing.
- The shared search interaction yields entries and failures in encounter order. It retains sufficient depth and optional failure-path information for the existing diagnostic attribution rules. Opening failures and per-entry failures must retain their observable consequences, including continuation where currently supported.
- Entries describe whether the search may descend through them. Immediate-child entries have no descent and require no additional metadata classification to satisfy the interface. Recursive entries retain the enumeration-time fact that lets discovery abandon a stale directory branch after followed inspection.
- Preserve the ability to prune recursive pending work, including failures without their own paths inside a discarded branch. Do not use a path-only filter that cannot account for those failures.
- Replace the two directory-handling paths in Document discovery with one consumption loop. Keep supported-document acceptance, Candidate origin, progress emission, and diagnostic decisions in that module. The real adapter supplies observations; it does not decide document eligibility.
- Adapt the in-memory adapter to the same interface. Preserve its ability to declare linked paths, listing failures, unreadable entries, stale directory classifications, and failures with no path, without real disk setup for selection policy tests.
- Preserve the order each existing mechanism reports. Do not sort results or promise identical ordering across independent filesystem executions. Preserve requested-input sequencing and the existing ordering of diagnostics relative to discovery progress.
- Preserve all downstream Document selection behavior, including EPUB eligibility, filtering, declaration retention, deduplication, display identity, and Output placement. The refactor must not introduce another owner for those decisions.
- Only underlying error-detail wording may differ. Changes to selected documents, origin, link behavior, failure attribution, progress, or continuation require a separate decision rather than changed test expectations.
- Keep the library interface narrow under ADR-0003. The search vocabulary remains internal to the crate; no new public exports or external dependencies are needed.
- Preserve accurate comments. Rewrite comments only where the changed interface makes their current description inaccurate, and document non-obvious acquisition and pruning constraints on the resulting implementation.

## Testing Decisions

- The user already agreed to the existing test seams: Document selection for observable selection behavior, and the Document search surface for real-adapter conformance. No additional policy-testing seam is required.
- Prefer the highest applicable seam. Exercise Document discovery through Document selection using the in-memory adapter, asserting Selected documents and Extraction run observations. Do not add tests that merely reproduce the shared loop or assert its private bookkeeping.
- Retain existing behavioral assertions. Mechanical changes to invoke the new interface are acceptable; weakening expected outcomes to accommodate the refactor is not.
- Use existing selection tests as prior art for linked unopenable roots, requested and nested links, continuation after unreadable entries, stale directory branch pruning, pathless failure attribution, and recursive versus non-recursive scope.
- Preserve assertions on candidate origin where the same path is both directly requested and discovered through a directory. The two encounters must retain their distinct effects on eligibility diagnostics.
- Verify complete observation sequences where ordering matters: requested-input diagnostics before initial discovery progress, failures before subsequent progress, growing discovered counts, one finished observation for an active phase, and silence when discovery is inactive.
- Use the existing real-adapter conformance tests as prior art for operating-system-dependent behavior. Cover immediate-child scope, path spelling beneath requested directory links, encounter-order preservation, and the retained recursive link behavior.
- Add a deterministic real-adapter regression case for a directory replaced by a regular file after successful inspection but before immediate-child search. Assert a failure attributed to the requested root, followed by exhaustion, rather than an empty successful search. Stage the change directly at the existing search seam; do not restore an observer callback that mutates disk during selection.
- Check immediate-child search yields no grandchildren and reports no possible descent. Verify it retains the underlying listing order in a controlled, unchanged fixture without treating independent filesystem runs as a universal ordering guarantee. Use declared encounter sequences for deterministic selection-order assertions.
- Assert diagnostic kind, attributed path, placement in the observation sequence, and continuation. Do not require identical operating-system error-detail strings across adapters.
- Run the focused selection and real-adapter tests, then the full test suite. Complete repository formatting and build checks and refresh the knowledge graph after implementation changes.

## Out of Scope

- Replacing direct listing with depth-limited recursive traversal or building a new recursive search engine.
- Adding sorting, arbitrary depth limits, new CLI flags, or new link-following policies.
- Changing EPUB deduplication, eligibility, declaration acquisition, cover behavior, Image write policy, conversion, or Image file emission.
- Consolidating Document selection phase reporters or changing the Extraction run observation vocabulary.
- Widening the public library interface, introducing a general filesystem abstraction, or moving document policy into an adapter.
- Changing user-visible behavior beyond underlying error-detail wording.
- Implementing the refactor as part of publishing this spec. This artifact specifies future implementation work; the present request is synthesis and local issue publication only.

## Further Notes

ADR-0012 records the approved design and the reason two acquisition mechanisms survive behind one interface. In the examined locked traversal implementation, shallow traversal reclassifies the root, performs additional entry classification, and opens direct child directories before discarding work beyond the depth limit. The directory-to-file race is sufficient to refute full equivalence with direct listing; it is not a claim that the current application has a new defect.

The intended gain is depth and locality, not a promised line-count reduction. The caller learns one directory-search interaction, and one Document discovery implementation owns entry decisions, while the real adapter preserves behavior that actually differs between its two mechanisms.

The design, compatibility constraint, and test seams were explicitly confirmed in the preceding discussion. The user then restricted the immediate work to documenting the ADR, and subsequently requested this spec. Implementation has not been performed.
