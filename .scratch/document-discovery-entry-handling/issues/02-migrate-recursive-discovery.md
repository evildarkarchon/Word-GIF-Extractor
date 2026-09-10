# 02: Migrate recursive discovery to the shared search interaction

**What to build:** Recursive Document selection uses the same directory-search interaction introduced for immediate children, through both adapters, while retaining its existing consumption loop. CLI users receive the same recursive search scope, selected documents, failure recovery, and Extraction run observations. After this ticket, the Document search surface exposes one directory-search operation with immediate-child and recursive choices; loop consolidation remains for ticket 03.

**Blocked by:** 01: Migrate immediate-child discovery to the shared search interaction.

Status: ready-for-agent

- [x] Migrate recursive acquisition in the real and in-memory adapters and its Document discovery consumer to the shared interaction. Retain the existing recursive traversal mechanism, exclude the root as before, and preserve encounter order without sorting or introducing arbitrary depth settings.
- [x] Entries retain the enumeration-time possible-descent fact, and failures retain depth and optional path information. Keep followed inspection and decisions about candidate acceptance, origin, diagnostics, and pruning in Document discovery.
- [x] A branch enumerated as a directory is abandoned when later inspection fails or finds a file or another non-directory kind. Preserve acceptance of a supported file replacing a directory, avoid duplicate opening failures, and discard stale descendants including failures with no path. A path-only pending-work filter is insufficient.
- [x] Pathless failures outside discarded branches are attributed to the nearest known directory, falling back to the requested root as appropriate. Failures retain their position relative to progress and do not prevent readable siblings or independent inputs from being selected.
- [x] Preserve requested directory-link traversal and discovered link-path spelling, avoid descending into nested directory links, and retain eligibility of nested supported file links. Preserve broken-link behavior and failures from linked unopenable roots.
- [x] The in-memory adapter retains deterministic declarations of linked paths, stale directory classifications, inspection failures, and pathless failures. Selection-policy tests exercise production discovery decisions through Document selection, rather than copying policy into the adapter or testing private loop bookkeeping.
- [x] Retain all existing recursive selection assertions and real-adapter link conformance assertions while mechanically adapting their invocation where necessary. Verify declared encounter sequences preserve candidate order and the first-occurrence EPUB deduplication winner. Assert diagnostic kind, attributed path, ordering, and continuation; only underlying error-detail wording may differ.
- [x] Remove the superseded directory-search operations and obsolete interaction vocabulary after all callers and tests migrate. Keep the two discovery consumption loops separate for now, with immediate-child behavior and ticket 01's regression coverage still passing.
- [x] Retain the narrow library boundary, separate inspection operations, and existing downstream ownership. Introduce no public exports, external dependencies, new testing seams, CLI flags, or selection-policy changes.
- [x] Preserve accurate comments, document new or substantially rewritten methods and non-obvious pruning constraints, and update affected documentation to reflect one search operation backed by two acquisition mechanisms under ADR-0012. Run focused selection and real-adapter tests, the full test suite, repository formatting and build checks, and refresh the knowledge graph after code changes.

## Implementation

Completed recursive migration through `search(..., SearchScope::Recursive)` in both adapters and Document discovery; removed the superseded `traverse` operation while retaining both consumption loops. Added four Document selection regressions for stale-branch pruning, supported replacements, pathless diagnostic attribution and ordering, continuation, and recursive encounter order with the first EPUB deduplication winner. Updated ADR-0012 to reserve loop consolidation for ticket 03.

Validation passed: 52 focused selection tests, 7 real-adapter tests, the full suite (250 unit and 12 CLI integration tests), `cargo check`, `cargo fmt --check`, and `cargo build --release`. `graphify update .` refreshed the AST graph. Standards and spec reviews reported no findings.
