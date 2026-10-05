# 02 — Document extraction facts carry the tally; the outcome accumulator folds tallies

**What to build:** Document extraction facts are a document's Emitted image tally plus its ordered
Document extraction warnings, with nothing derived from them. The outcome accumulator adds the
documents' tallies together. It reads "covers only / included normal images / nothing" off the
combined totals instead of merging a carried classification. This removes ticket 01's bridge and the
second spelling of the counts. Command-line behaviour does not change.

The accumulator:

- is seeded from Applicable outcome facts as today;
- folds each document's facts by adding its tally, and counts the document as having output when
  its tally emitted at least one image;
- records failures separately as today, so a failed document's partial tally still folds and its
  failure is still one extra fact;
- on finish, classifies output as covers exactly when the run's cover intent is set and the combined
  normal-image total is zero, and as images otherwise.

This rule is exactly equivalent to today's. A document that emitted both a normal image and a cover
would classify as images, which agrees with the old merge, so the pipeline's unwritten "never both"
rule stops mattering.

Deleted from Document extraction: the emitted-image totals type, the three-way output purpose and
its merge rule, the partition debug assertion, and the test-only facts entry point that took a
hand-built pipeline result. Added: a test-only constructor taking a tally and Document extraction
warnings. The translation from a pipeline result keeps only the warning translation and the
hand-over of the tally. The Document extraction warning and error test entry points stay exactly as
they are.

The Extraction run outcome and produced-output types keep their variants, fields and accessors.
Extraction run presentation's production code needs no change. Classification is still tested
through the run tests at this point. Ticket 03 moves those tests down.

**Blocked by:** 01 — Image write pipeline records each emitted image into an Emitted image tally

**Status:** ready-for-agent

Spec: `.scratch/emitted-image-tally/spec.md` (ADR-0017). User stories 10–17 and 19, and the
"Document extraction tests" testing decision.

## Acceptance criteria

- [ ] Document extraction facts hold exactly a tally and ordered Document extraction warnings, and expose them through accessors
- [ ] The emitted-image totals type, the output purpose type and its merge rule, the partition debug assertion, and the facts entry point that took a pipeline result are deleted
- [ ] A test-only (`cfg(test)`) constructor builds Document extraction facts from a tally and Document extraction warnings. Production visibility is otherwise unchanged
- [ ] The Document extraction warning and error test entry points are unchanged
- [ ] The accumulator adds tallies, counts a document as having output exactly when its tally emitted at least one image, and keeps failure recording separate
- [ ] Finish classifies output as covers exactly when cover intent is set and the combined normal-image total is zero
- [ ] The Extraction run outcome and produced-output types keep their variants, fields and accessors. Extraction run presentation's production code, the Extraction run, Document selection and the command-line entry point are not edited
- [ ] Deleted, because their subject is gone: the Document extraction test that ran fabricated facts through the production translation, and the test that tripped the partition guard with fabricated facts
- [ ] Each Document extraction test that extracts a real document asserts on the document's tally: failed extraction retains facts; DOCX, EPUB cover, EPUB cover conversion and EPUB cover retry warning bodies; EPUB cover output (now "one cover, no normal images"); EPUB cover fallback (normal images); normal policy extracts EPUB images; retained EPUB declarations; selection declaration failure is retried. The fabricated-error source-chain test is unaffected
- [ ] The Extraction run tests' scripted-facts helpers use the new test constructor. No run-test assertion changes
- [ ] Comments justifying deleted items (the partition assertion's explanation, the output purpose's cycle rationale, the accumulator's "why finishing needs no check" section) are removed or rewritten only where the code they describe is gone or now false. Each one is called out in the change description
- [ ] Status notes pointing to ADR-0017 are added to ADR-0004's output-purpose rationale, ADR-0007's assertion paragraph, and ADR-0016's fabrication-through-the-guard paragraph
- [ ] Nothing under `tests/` is edited
- [ ] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
