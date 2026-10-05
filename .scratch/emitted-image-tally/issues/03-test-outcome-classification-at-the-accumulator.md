# 03 — Outcome classification is tested at the outcome accumulator

**What to build:** Extraction run outcome classification is tested where it lives. Tallies and
failures go into the outcome accumulator and an Extraction run outcome comes out, with no Document
extraction, search surface or EPUB declarations in sight. Run tests keep only what the run itself
decides: what it hands the fold, and sequencing. No production behaviour changes.

Applicable outcome facts gain a test-only constructor. Accumulator tests can then choose conversion
applicability and a GIF destination directly. The value carries no invariant to bypass.

The decision rule for each run test: a test moves down when its assertion is about what the outcome
says given some facts. It stays when its assertion is about what the run handed the fold, or about
sequencing. The spec's verdict table applies this rule to every run test and is authoritative.

- **Moves down to the accumulator:** document without images → image no-output; normal output →
  produced images; EPUB normal fallback → images; routed GIF retains count and destination; combined
  conversion and GIF-routing facts; failed document without output counted in no-output; cover-only
  run with a written cover and an empty EPUB → covers; covers merged with fallback images → images;
  conversion and GIF-routing totals summed across documents; failed EPUB in a cover-only run → cover
  no-output.
- **Stays as a run test**, and compares outcomes through accessors instead of a hand-built produced
  outcome. Each pins one thing the run does:
  - the run's cover intent reaches classification (an EPUB without a cover still yields cover
    no-output);
  - Applicable outcome facts seed the fold (requested conversion keeps valid zero totals);
  - a failed document's partial facts are folded;
  - every failure is recorded and matches the error observations.

Folding at the accumulator:

- The three moved no-output tests and the staying cover-intent run test's case become one compact
  case table that covers all four intent × failure combinations over zero facts. The run test
  itself still stays.
- The single-document combined-facts case and the single-document fallback case may fold into the
  multi-document tests they are special cases of, as long as the case each pins is still named.
- Because tallies combine by addition, the merge test needs only one order. A second order may stay
  as documentation.
- The combined-facts test's "partition guard at equality" rationale is gone with the guard. Drop it
  and call out the drop.

One new accumulator test: GIF routing applies, nothing was routed, and the outcome carries no
routing facts. Today only the staying partial-facts run test pins this, and only incidentally.

**Blocked by:** 02 — Document extraction facts carry the tally; the outcome accumulator folds tallies

**Status:** done

Spec: `.scratch/emitted-image-tally/spec.md` (ADR-0017). User stories 20–28, and the "Run-test
verdicts" and "Folding duplicates at the accumulator" testing decisions.

## Acceptance criteria

- [x] Applicable outcome facts have a `cfg(test)` constructor taking conversion applicability and an optional GIF destination, with a doc comment
- [x] Accumulator tests live beside the outcome types in the observation module's test file. They feed tallies and failures into the accumulator and assert on the finished outcome through accessors, or against an outcome built the same way
- [x] Every run test marked "Moves down" in the spec's verdict table is removed from the run tests, and its assertion exists at the accumulator. Each pinned case is still named, either in its own test or as a named case in a folded test
- [x] The four intent × failure combinations over zero facts are one case table
- [x] A new accumulator test pins that GIF routing applies, nothing is routed, and the outcome has no routing facts
- [x] The four staying run tests keep their subjects and compare outcomes through accessors. No run test builds a produced outcome by hand
- [x] Run tests marked "Unaffected" keep their assertions
- [x] ADR-0016's classification-test-level decision gets a status note pointing to ADR-0017
- [x] Removed or rewritten comments are called out in the change description
- [x] Nothing under `tests/` is edited
- [x] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
