# 04 — Move the classification, failure and warning run tests

Status: ready-for-agent

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

- [ ] All ten listed tests call the inner function with the three test adapters and write nothing to disk
- [ ] Their assertions are unchanged — a needed assertion change is evidence of a behaviour change, not a test to fix
- [ ] No moved test restates wording owned by Document extraction or presentation
- [ ] The EPUB identity-consistency test still runs through the public entry against real files
- [ ] The command-line entry point's three run tests and the whole `tests/` suite are unedited
- [ ] No production code changes in this ticket
- [ ] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
