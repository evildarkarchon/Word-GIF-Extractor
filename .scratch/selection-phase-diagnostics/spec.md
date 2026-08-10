# Document selection phase diagnostics and the silence gate

Origin: Candidate C of `Architecture_review_2.html` ("Collapse the three Document selection
phase reporters into one"), stress-tested in a grilling session before any code was written.
The decision itself is `docs/adr/0009-report-each-selection-diagnostic-as-its-own-fact.md`;
this file records the scope negotiation that produced it, because the review file will keep
saying something different from what landed.

## What the review proposed

One phase reporter parameterised over running and finished observation constructors,
instantiated three times, removing "roughly two thirds" of the 307-line
`src/document_selection/progress.rs`. ADR-0004 had already scheduled part of it, calling the
three byte-identical `diagnostic` bodies "known duplication and a separate change".

## Where the review was wrong

Its "Before" panel claims all three reporters carry `observer · active · total · checked`.
`DocumentDiscoveryProgress` carries `observer · active · scope · discovered` — no `total`, no
`checked`, no `record_check`, and `discovering()` has no completeness assertion. There are not
three copies of one shape; there are two copies of one shape and one phase of a different kind.
Discovery is unbounded, so it has no denominator to assert against.

A call-site inventory produced the second correction. The three `diagnostic` bodies are
identical, but across all eight call sites only three variants are ever passed, and they do not
overlap: discovery's five sites pass `DocumentDiscoveryFailed` and nothing else; filtering's one
site and deduplication's one site pass `UnreadableEpubMetadata` differing only by a `purpose`
constant the phase already knows. The bodies are identical because they sit one notch above the
facts they carry.

## What was decided

The headline inverts. The three `diagnostic` bodies are made **non-identical** rather than
shared, by naming each method for the fact it reports. Deciding criterion throughout: concentrate
invariants, not line count.

| | decision |
| --- | --- |
| Per-fact diagnostic methods | **in** — makes the wrong observation and the wrong purpose compile errors |
| One free-function silence gate | **in** — twelve gated emission points, one gate |
| Rename `UnreadableEpubMetadata` / `EpubMetadataPurpose` to the glossary's declarations vocabulary | **in** — isolated commit |
| Generic reporter over running/finished constructors | **out** — needs an optional `total` (illegal state) or a tally trait that widens call-site types; its fourth-phase payoff is speculative |
| Four-closure collapse of the three lifecycle phase methods | **out** — shallow abstraction |
| Shared bounded-phase counter for the guards | **out** — one call site per guard, costs more lines than it absorbs |
| Folding discovery into a bounded reporter | **out** — no denominator |
| New tests | **out** — the mismatch is a compile error; all three inactive-phase silence paths are already pinned |

## Cost, honestly

`progress.rs` does not shrink by two thirds. It ends up about the same size or slightly larger;
the call sites in `discovery.rs` and `document_selection.rs` are what get shorter. Nothing in
`docs/`, `plans/`, `CONTEXT.md` or `README.md` plans a fourth selection phase, so no part of the
case rests on one arriving.

## Confirmed facts behind the above

- Reporter call sites: 10, across `discovery.rs` and `document_selection.rs` only.
- `DocumentSelectionLifecycle` is the portable handle — it crosses three function boundaries.
  The three phase reporters are closure-local, so the module's width is mostly invisible.
- `MissingInput`'s ungated emission is pinned by `document_selection/tests.rs:117` and by an
  exact-sequence assertion at `extraction_run/tests.rs:287`.
- `DocumentSelectionLifecycle` has no `active` field, so the two ungated lifecycle diagnostics
  cannot be routed through the silence gate by accident.
