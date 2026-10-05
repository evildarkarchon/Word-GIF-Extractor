# 02 — Fabricate Document extraction values in tests; presentation stops running the stack

Status: ready-for-agent

Blocked by: None — can start immediately

Spec: `.scratch/extraction-run-document-extraction-seam/spec.md` (ADR-0016)

## What to build

Document extraction gains test-only, crate-visible entry points that build its values through the
same private conversions production uses:

- Image write result → Document extraction facts
- Image write warning → Document extraction warning
- underlying cause → Document extraction error

Direct test constructors are deliberately not added: fabricated facts must pass through the
partition guard ADR-0007 kept, so a test cannot construct an outcome production could never
produce. The entry points exist only in test builds; production visibility of every conversion and
constructor is unchanged.

The first users are two Extraction run presentation tests that currently run the whole stack:

- The discovery-failure suspension test feeds presentation a recursive discovery start and a
  discovery failure directly, retiring the last observer that doubles as a filesystem hook (it
  deletes a directory from inside its callback), as ADR-0008 retired it from Document selection.
- The warning-presentation test feeds a Document extraction warning fabricated from an Image write
  warning, instead of writing and extracting a real DOCX to obtain one.

## Acceptance criteria

- [x] The three test entry points delegate to the existing private conversions; none constructs a value directly
- [x] Facts fabricated through the entry point are checked by the partition guard
- [x] The entry points are compiled only in test builds; production visibility is unchanged
- [x] The discovery-failure suspension test no longer touches the filesystem; its suspension assertion is unchanged
- [x] The warning-presentation test no longer writes or extracts a DOCX; its prefix and suspension assertions are unchanged, and it restates no warning wording owned by Document extraction
- [x] No other presentation test, and nothing under `tests/`, is edited
- [x] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
