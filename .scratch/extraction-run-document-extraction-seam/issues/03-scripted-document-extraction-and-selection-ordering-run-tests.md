# 03 — Scripted Document extraction, proven by moving the selection-ordering run tests

Status: ready-for-agent

Blocked by: 01, 02

Spec: `.scratch/extraction-run-document-extraction-seam/spec.md` (ADR-0016)

## What to build

A scripted Document extraction for the run's tests, living beside them (its only users) rather than
in shared test support:

- It wraps a real Document extraction built from real policies and delegates cover intent and
  Applicable outcome facts to it, so a test needing conversion to apply builds a policy with
  conversion, just as production does.
- It scripts extraction by document path: each path maps to one Document extraction outcome, taken
  out on use.
- Extracting a path twice panics; extracting an unscripted path panics — enforcing the request's
  "consumed exactly once".

Run tests call the run's inner function with three adapters plugged in: the in-memory Document
search surface and in-memory EPUB declaration source that Document selection's own tests already
use, and the scripted Document extraction. Document selection itself stays real, so the
interleaving of selection diagnostics with extraction observations is exercised by both modules
together.

Prove the adapter by moving the six run tests whose subject is selection and ordering:

- no selected documents → No documents outcome
- all requested inputs failed → exactly one No documents terminal observation
- nested discovery failure precedes later progress and extraction
- recursive discovery failure precedes later progress and extraction (retires the staged broken
  directory link that needs symlink privileges on Windows)
- selection diagnostic and completion precede extraction in one observation stream
- cover-only run skips a requested DOCX and diagnoses it

## Acceptance criteria

- [ ] The scripted adapter delegates cover intent and Applicable outcome facts to a real Document extraction
- [ ] The scripted adapter panics on a second extraction of a path and on an unscripted path
- [ ] Scripted outcomes are fabricated only through ticket 02's test entry points
- [ ] The six listed tests no longer write to the filesystem and call the inner function, not the public entry
- [ ] Their assertions are unchanged; where an assertion names a real path, the in-memory path takes its place without changing what is asserted
- [ ] No production code changes in this ticket
- [ ] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
