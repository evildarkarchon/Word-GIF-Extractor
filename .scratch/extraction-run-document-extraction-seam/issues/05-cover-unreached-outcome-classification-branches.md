# 05 — Cover the outcome-classification branches nothing reaches yet

Status: ready-for-agent

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

- [ ] Unreached classification branches are identified and listed under a `## Comments` heading in this ticket before tests are written
- [ ] A test reaches a produced Covers outcome through the inner function
- [ ] A test reaches the mixed-purpose merge through the inner function
- [ ] New tests assert only on the returned Extraction run outcome and the ordered observations, not on the accumulator's internals
- [ ] New tests land in their own commit, with no edits to tests moved in 03 or 04
- [ ] No production code changes; `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
