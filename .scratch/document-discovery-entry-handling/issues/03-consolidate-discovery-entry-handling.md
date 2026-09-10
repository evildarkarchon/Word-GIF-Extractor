# 03: Consolidate Document discovery entry-handling loops

**What to build:** Document discovery consumes both directory-search modes through one entry-handling loop, giving maintainers one place for inspection, supported-document acceptance, candidate origin, progress, and diagnostic decisions. Document selection and Extraction run observations remain behaviorally identical to the migrated implementation, completing ADR-0012 without replacing either acquisition mechanism.

**Blocked by:** 02: Migrate recursive discovery to the shared search interaction.

Status: ready-for-agent

- [x] Replace the separate immediate-child and recursive consumption loops with one loop over the shared directory-search interaction. Use its descent and failure-position information to preserve mode-specific consequences; do not move document policy into either adapter or add a new policy-testing seam.
- [x] Preserve requested-input classification, including the followed/non-following inspection distinction between genuinely missing paths and broken links. Directly requested supported documents remain discoverable and requested-input diagnostics precede initial discovery progress.
- [x] Preserve search scope, requested and nested link treatment, Source identity spelling, opening and per-entry failure attribution, continuation after failures, and recursive stale-branch pruning including pathless failures. Retain the deterministic directory-to-file regression and both adapters' conformance coverage from earlier tickets.
- [x] Preserve each search mechanism's encounter order and requested-input sequencing. Through Document selection, verify deterministic candidate ordering and the same first-occurrence EPUB deduplication winner under declared observations, without promising order across independent filesystem executions.
- [x] Preserve distinct candidate origins when the same document is directly requested and found beneath a requested directory. EPUB-only eligibility diagnostics still apply only to directly requested inputs.
- [x] Verify complete Extraction run observation sequences where order matters: requested-input diagnostics before initial discovery progress, discovery failures before subsequent progress, growing discovered counts, and exactly one finished observation for an active phase. Unsupported entries remain silent and inactive discovery emits no observations.
- [x] Retain existing assertions for downstream EPUB eligibility, filtering, declaration retention, deduplication, display identity, and Output placement. Do not weaken expected outcomes to accommodate the refactor or require identical operating-system error-detail strings. Only underlying error-detail wording may change.
- [x] The final architecture has one internal directory-search operation and one discovery consumption loop, retains direct listing and the existing recursive traversal in the real adapter, and preserves the narrow public library interface. Add no dependencies, new CLI options, sorting, link-following policies, or phase-reporter consolidation.
- [x] Preserve accurate comments; rewrite or remove comments only where the changed implementation makes them inaccurate, and identify those changes in the implementation summary. Document new or substantially rewritten methods and the non-obvious reasons for separate acquisition mechanisms and depth-aware pruning. Align affected domain and implementation documentation with the completed ADR-0012 design without changing unrelated ownership decisions.
- [x] Run focused Document selection and real-adapter tests, then the full test suite and repository formatting and build checks. Refresh the knowledge graph after implementation changes and confirm all three tickets' behavioral coverage remains intact.

## Implementation

Completed one entry-handling loop for both directory-search scopes, preserving followed inspection, candidate origin, depth-aware failure attribution, and stale-branch pruning. Both acquisition mechanisms and requested-input classification remain unchanged. Added Document selection coverage for complete classification/search observation ordering in both scopes and immediate-child encounter order with the first EPUB deduplication winner. Existing assertions from tickets 01 and 02 remain intact.

Updated the search-surface module comment because its separate-loop description became obsolete; moved the accurate nested-link inspection comment into the shared loop and extended discovery method documentation. Updated CONTEXT.md and ADR-0012 to describe the completed design.

Validation passed: 54 focused Document selection tests, 7 real-adapter tests, the full suite (252 unit and 12 CLI integration tests), cargo check, cargo fmt --check, cargo build --release, and cargo clippy --all-targets -- -D warnings. graphify update . refreshed the AST graph (community names were regenerated where membership changed). Independent standards and spec reviews reported zero findings.
