# Cover-run behaviour is untested across document kinds

Status: needs-triage

Found while implementing Candidate E (`docs/adr/0010-let-the-absence-of-a-cover-policy-be-the-absence-of-a-value.md`).
Not caused by it — the gap predates the change, which only made it visible.

## The gap

Until commit `6822b31` the crate had **no multi-document run test with mixed document kinds**.
Every multi-document fixture in `src/extraction_run/tests.rs` was DOCX-only; every EPUB run was a
single document. Cover-only behaviour had therefore never been observed against a document kind
that has no covers, in a real run.

That change added exactly one such test —
`docx_in_a_cover_only_run_still_emits_its_normal_images` in `src/extraction_run/tests.rs` —
because it deleted the only test holding up that one claim. It deliberately did **not** sweep the
rest of the space. This ticket is that sweep.

The nearest pre-existing test, `builds_validated_epub_cover_extraction_policy`
(`src/extraction_run_intake/tests.rs`), *is* a `--cover-only` run whose only input is a DOCX — but
its fixture DOCX is written with zero media entries, so it asserts `NoOutput(Covers)` and the
emission behaviour is unobservable. It pins the intake-built policy, not the run.

## Cases worth considering

Each is a mixed DOCX + EPUB run; none is currently covered.

- `--cover-only` where the EPUB **has** a cover — covered now by the new test. Confirms
  `DocumentOutputPurpose::merged_with` absorbs to `IncludedNormalImages`, so the run classifies as
  `Images` even though covers were sought.
- `--cover-only` where the EPUB has **no** cover. The DOCX still emits; the EPUB emits nothing.
  Outcome should still be `Images`, and the run should not report `NoOutput(Covers)`.
- `--cover-only --cover-fallback` with a coverless EPUB, so both kinds emit normal images.
- Ordering: `ExtractionRunObservation::ExtractionStarted { cover_only: true }` and the
  presentation wording "Extracting cover images" are emitted for a run that will mostly produce
  normal images. Whether that wording is right for a mixed run is a **presentation question, not
  a test question** — decide it before pinning it.
- Document-kind interleaving in observation order for a mixed input set.

## Why it is worth doing separately

`DocumentOutputPurpose::merged_with` (`src/document_extraction.rs`) encodes a real cross-document
rule — most-inclusive wins, `IncludedNormalImages` absorbs — and its absorbing case is only
reachable in a mixed run. One test now exercises one path through it. The rest of the rule is
pinned only by unit tests of the merge function itself, not by any run that actually mixes kinds.

## Cost

Low. No new helper is needed: `write_docx` and `write_epub_document`
(`src/test_support.rs`) are already imported side by side at the top of
`src/extraction_run/tests.rs`, and a mixed fixture is two writes into one temp dir plus two
positional args. `tests/support/mod.rs` has no EPUB builder, so keep this at the in-crate level
unless CLI-visible behaviour is actually in question (`AGENTS.md`: `tests/` covers the binary and
CLI-visible behavior only).

## Open question for triage

Whether the fourth bullet is a bug rather than a coverage gap. A `--cover-only` run over a folder
that is mostly DOCX announces "Extracting cover images" and then produces almost entirely normal
images. That may be fine — the user did ask for covers — but nobody has decided it on purpose.
