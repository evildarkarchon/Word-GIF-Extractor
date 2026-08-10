# Report each Document selection diagnostic as its own fact

Status: ready-for-agent

Spec: `.scratch/selection-phase-diagnostics/spec.md`
Decision: `docs/adr/0009-report-each-selection-diagnostic-as-its-own-fact.md`

Three commits, in this order. The emitted observation stream must be byte-identical after each
one, so every existing ordering, monotonicity and silence test passes unmodified — except the
mechanical rename in the first commit. A test that needs editing for any other reason is
evidence the refactor changed behaviour, not a test to fix.

## 1. Rename the EPUB metadata synonym

`UnreadableEpubMetadata` → `UnreadableEpubDeclarations`, `EpubMetadataPurpose` →
`EpubDeclarationPurpose`. Twenty-one occurrences across `extraction_run_observation.rs` (3),
`document_selection.rs` (5), `document_selection/tests.rs` (8),
`extraction_run_presentation.rs` (4) and `test_support.rs` (1). Rename only — nothing else in
this commit, so the edited assertions stay readable as a rename.

## 2. Per-fact diagnostic methods

Delete the four `diagnostic` methods and their `ExtractionRunObservation` parameter. Add:

| type | method | gated by `active`? |
| --- | --- | --- |
| `DocumentSelectionLifecycle` | `missing_input(path)` | no |
| `DocumentSelectionLifecycle` | `discovery_failed(path, detail)` | no |
| `DocumentDiscoveryProgress` | `discovery_failed(path, detail)` | yes |
| `EpubFilteringProgress` | `declarations_unreadable(path, detail)` — supplies `Filtering` | yes |
| `EpubDeduplicationProgress` | `declarations_unreadable(path, detail)` — supplies `Deduplication` | yes |

`discovery_failed` deliberately carries the same name on two types: the fact is identical, and
which type holds the method is the statement about where it lands. Update the eight call sites
in `discovery.rs` (L78, L83, L155, L175, L203, L213, L224) and `document_selection.rs` (L274,
L325); they stop constructing observations.

## 3. One silence gate

Add a crate-private `fn emit_when(observer: &mut dyn ExtractionRunObserver, active: bool,
observation: ExtractionRunObservation)` to `progress.rs` and route all twelve gated emission
points through it: the initial and final observation in each of `discovering`, `filtering` and
`deduplicating`; the three gated per-fact methods; `document_discovered`; and both
`record_check` bodies. The two ungated lifecycle methods do not use it.

## Out of scope

The generic reporter, the four-closure lifecycle collapse, and a shared bounded-phase counter
are all rejected in ADR-0009 with reasons — do not reintroduce them as "finishing the job". The
three reporter types and the three lifecycle phase methods stay. Both completeness guards stay
hard `assert!`. No new tests and no `progress/tests.rs`.

## Done when

`cargo test` passes at each commit, `cargo build --release` succeeds, and `graphify update .`
has run after the code commits.
