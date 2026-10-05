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

- [x] The scripted adapter delegates cover intent and Applicable outcome facts to a real Document extraction
- [x] The scripted adapter panics on a second extraction of a path and on an unscripted path
- [x] Scripted outcomes are fabricated only through ticket 02's test entry points
- [x] The six listed tests no longer write to the filesystem and call the inner function, not the public entry
- [x] Their assertions are unchanged; where an assertion names a real path, the in-memory path takes its place without changing what is asserted — except two file-existence assertions in the cover-only test; see Notes
- [x] No production code changes in this ticket
- [x] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass

## Notes

- `cover_only_run_skips_requested_docx_and_diagnoses_it` could not keep two of its assertions
  literally: it asserted that `Test Creator - Covered.jpg` existed and `sample.png` did not, and a
  scripted extraction writes no files. Those two assertions became one run-level assertion — the
  EPUB is the only document started, under its Document identity — and the scripted adapter panics
  if the DOCX reaches extraction. The written cover name stays checked on disk by
  `epub_identity_is_consistent_across_normal_and_cover_runs`. This departs from ADR-0016's
  "assertions unchanged" and is called out in the test itself.
- `all_failed_requested_inputs_reach_one_no_documents_terminal_observation` used paths containing a
  NUL byte to provoke inspection failures; broken links in the in-memory surface provoke the same
  `DocumentDiscoveryFailed` observation.
