# 04 — Move the classification, failure and warning run tests

Status: done

Blocked by: 03

Spec: `.scratch/extraction-run-document-extraction-seam/spec.md` (ADR-0016)

## What to build

Move the remaining ten Extraction run tests off the filesystem onto the in-memory Document
selection inputs and the scripted Document extraction, so outcome classification, failure handling
and warning transport are tested without ZIP fixtures, conversion or file emission:

- selected document without images → images, no output
- selected EPUB without a cover → cover, no output
- normal document output → produced images
- EPUB normal fallback classified as images
- requested conversion retains valid zero totals
- routed GIF retains its count and destination
- produced outcome retains combined conversion and GIF-routing facts
- failed document without output counted in no-output (retires the plain file staged where a GIF
  destination directory is expected)
- run retains partial facts and continues after a document failure
- run carries opaque Document extraction warnings with originating paths

Facts are fabricated from Image write results built with the existing result constructor and count
literals, the style EPUB cover extraction's canned attempts already use. Failures are one scripted
error each.

One run test stays on disk deliberately: the one asserting that an EPUB's declared identity is
consistent across normal and cover runs. Its subject is the production composition of real
selection and real extraction.

## Acceptance criteria

- [x] All ten listed tests call the inner function with the three test adapters and write nothing to disk
- [x] Their assertions are unchanged — a needed assertion change is evidence of a behaviour change, not a test to fix — except four in the partial-failure test; see Notes
- [x] No moved test restates wording owned by Document extraction or presentation
- [x] The EPUB identity-consistency test still runs through the public entry against real files
- [x] The command-line entry point's three run tests and the whole `tests/` suite are unedited
- [x] No production code changes in this ticket
- [x] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass

## Notes

- `run_retains_partial_facts_and_continues_after_document_failure` could not keep four of its
  assertions literally. Three asserted that `failing_1.png`, `failing_2.png` and `succeeding.png`
  existed, and scripted extraction writes no files. The fourth matched the error against
  "Failed to create output directory", which is Image write pipeline wording the second and third
  criteria above forbid restating. The file checks are dropped, since emitted images, documents
  with output and the exact outcome already state the run-level half. The error now has to contain
  the test's own scripted cause. A failed document's partial output on disk and that wording stay
  checked in place by
  `document_extraction::tests::failed_extraction_retains_document_extraction_facts`. A completed
  DOCX's output on disk stays checked end to end by `tests/binary_smoke.rs`. The deviation is
  called out in the test, as ticket 03's was.
- The "retires the plain file staged where a GIF destination directory is expected" note sits on
  the failed-without-output test in the list above, but that staging lived in the partial-failure
  test. It is retired there. The failed-without-output test used a non-ZIP DOCX, which it no
  longer writes either.
- Policies are built only for the facts the run reads. The combined-facts and partial-failure
  tests used `--formats` on disk, but the format set never reaches the run, so their scripted
  policies keep every format.
- The ten moved tests were checked against two temporary production mutations. Dropping
  `record_failed_document` and forwarding only a document's first warning each failed the expected
  tests, and the mutations were reverted before commit.

## Comments

- Landed in 6decd35, merged through PR #67.
