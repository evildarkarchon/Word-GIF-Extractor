# 01: Migrate immediate-child discovery to the shared search interaction

**What to build:** Non-recursive Document selection uses the shared directory-search interaction through both the real and in-memory adapters, preserving the documents selected and the Extraction run observations emitted today. Introduce the interaction needed by ADR-0012 while leaving recursive discovery operational through its existing interaction until ticket 02. This is the first independently verifiable migration, not a change to search policy.

**Blocked by:** None (can start immediately).

Status: ready-for-agent

- [x] Introduce crate-private search vocabulary that accommodates immediate-child and recursive search, ordered entries and failures, depth, optional failure paths, possible descent, and pruning. Migrate immediate-child discovery end to end; do not consolidate the two discovery consumption loops yet.
- [x] The real adapter retains direct directory listing. Immediate-child entries report no possible descent and require no additional metadata classification; no grandchildren are yielded. Do not substitute depth-limited recursive traversal.
- [x] Preserve directory-opening failures and per-entry failures, their attribution to the requested directory when an entry path is unavailable, and continuation to later readable entries and independent requested inputs.
- [x] Preserve requested directory links and the spelling of discovered paths beneath them, nested supported file links, broken-link diagnostics, supported-document acceptance, candidate origin, and requested-input sequencing. Followed and non-following inspection remain separate operations, with requested-input classification owned by Document discovery.
- [x] Adapt the in-memory adapter without losing declared encounter order, linked paths, listing failures, or unreadable entries. Keep recursive fixtures and behavior working during the transition.
- [x] Add a deterministic real-adapter regression that successfully inspects a directory, replaces it with a regular file, and starts immediate-child search. Assert a failure attributed to the requested root followed by exhaustion. Stage the race directly at the existing Document search surface seam, without a disk-mutating selection observation callback.
- [x] Real-adapter conformance coverage verifies immediate-child scope, no possible descent, link-path spelling, and retained underlying listing order in a controlled unchanged fixture. Do not claim ordering stability across independent filesystem executions.
- [x] Exercise observable behavior through Document selection with the in-memory adapter. Retain existing assertions, including diagnostics before subsequent progress, growing discovered counts, exactly one finished observation for an active phase, unsupported-entry silence, and inactive-discovery silence. Assert diagnostic kind, path, sequence, and continuation without requiring identical operating-system error-detail strings.
- [x] Keep document eligibility and reporting decisions in Document discovery and downstream selection decisions with their existing owners. Add no public exports, external dependencies, CLI options, sorting, or new link policies. Only underlying error-detail wording may change.
- [x] Preserve accurate comments, document new or substantially rewritten methods and non-obvious acquisition constraints, and update affected documentation to describe this transitional state consistently with ADR-0012. Run focused selection and real-adapter tests, the full test suite, repository formatting and build checks, and refresh the knowledge graph after code changes.

## Implementation

Completed immediate-child migration; recursive discovery remains on `traverse` for ticket 02. Validation passed: 48 focused selection tests, 7 real-adapter tests, the full suite (246 unit and 12 CLI integration tests), `cargo check`, `cargo fmt --check`, and `cargo build --release`. `graphify update .` refreshed the AST graph. Standards review's two documentation findings were fixed; spec review reported no findings.
