# 05 — Cover the outcome-classification branches nothing reaches yet

Status: done

Blocked by: 04

Spec: `.scratch/extraction-run-document-extraction-seam/spec.md` (ADR-0016)

## What to build

With the run reachable through its inner function and scripted Document extraction, add run tests
for Extraction run outcome classification branches that no test reaches today. This is kept
separate from the move so new coverage never blurs the evidence that moved tests kept their
assertions.

Start by identifying which classification branches are unreached (cover-only versus images,
Applicable outcome facts, failed-document counts, purpose merging). At minimum cover:

- a produced Covers outcome
- the mixed-purpose merge of Document extraction facts across documents

## Acceptance criteria

- [x] Unreached classification branches are identified and listed under a `## Comments` heading in this ticket before tests are written
- [x] A test reaches a produced Covers outcome through the inner function
- [x] A test reaches the mixed-purpose merge through the inner function
- [x] New tests assert only on the returned Extraction run outcome and the ordered observations, not on the accumulator's internals
- [x] New tests land in their own commit, with no edits to tests moved in 03 or 04
- [x] No production code changes; `cargo fmt --check`, `cargo clippy` and `cargo test` all pass

## Comments

Classification branches of the Extraction run outcome, checked against every run test through
`run_with` and the one on-disk run test, before any new test was written. "Reached" means a run test
drives the branch and asserts on its result.

Already reached, so not re-covered here:

- Single-document classification: Images when the run is not cover-only, Covers no-output for a
  cover-only EPUB that emitted nothing, Images for a cover-only run whose EPUB fell back to normal
  images.
- A produced Covers outcome from one EPUB, though only as a side assertion in
  `cover_only_run_skips_requested_docx_and_diagnoses_it`, whose subject is the skipped DOCX. That
  test checks kind, emitted images and documents with output, not the whole outcome.
- Applicable conversion facts at zero and non-zero totals for one document, GIF routing for one
  document, and applicable GIF routing omitted when nothing was routed.
- One failed document, in Images no-output and in produced Images.

Unreached:

1. **Mixed-purpose merge.** No run folds documents with different output purposes. Every
   multi-document run test folds normal images with normal images, and no multi-document run is
   cover-only. So nothing shows that one document's normal images make a cover-only run's output
   Images whatever the order. A "last document wins" merge would pass every current test.
2. **Nothing emitted as the identity of the merge in a cover-only run.** No cover-only run has a
   document that emitted nothing beside one that emitted a cover. Such a run should still be
   Covers, with one document with output out of two.
3. **A produced Covers outcome asserted as a whole.** Its conversion, GIF-routing and failure facts
   are never pinned for a Covers outcome.
4. **Cross-document sums of conversion and GIF-routing totals.** No non-zero total is ever summed
   across documents: only one document ever converts or routes anything. Documents with output are
   counted against a document that emitted nothing only in single-document runs.
5. **More than one failed document.** The failed-document count never goes above one.
6. **A failed document in a cover-only run.** Covers no-output is never paired with a failure
   count.

Each gets one run test through the inner function. Applicable conversion in a cover-only run is
left out: it adds no branch, since the conversion fact group is seeded the same way whatever the
cover intent.

After writing the tests, item 2 turned out not to be a branch the outcome can show. Finishing asks
only whether normal images were included, so a cover-only run is Covers whether the merged purpose
is covers-only or nothing-emitted. Which one the merge settles on is invisible from the outcome. The
test for it now pins item 3 and the document-with-output count instead, and says so. Item 6 is a
combination of the kind and the failure count, which finishing computes independently, rather than
a code branch of its own.

| Item | Test |
| --- | --- |
| 1 | `cover_run_merging_covers_with_fallback_images_classifies_as_images_in_either_order` |
| 3 (and 2, which it cannot tell apart) | `cover_only_run_with_a_written_cover_and_an_empty_epub_produces_covers` |
| 4 | `produced_outcome_sums_conversion_and_gif_routing_totals_across_documents` |
| 5 | `every_failed_document_is_counted_in_the_outcome` |
| 6 | `failed_epub_in_cover_only_run_is_counted_in_cover_no_output` |

No production code changes, so every new test passed when written. Instead, each was checked against
temporary production mutations, all reverted before commit. In every case only new tests failed:

- A merge where the later document's purpose wins failed the mixed-purpose test only.
- Counting every folded document as a document with output failed the Covers, sums and
  failure-count tests.
- Overwriting conversion and routed-GIF totals instead of summing them failed the sums test.
- Recording a failure as a flag rather than a count failed the failure-count test.

- Landed in a0ae201, merged through PR #68.
