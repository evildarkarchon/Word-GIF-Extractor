# 04 — Delete the validating produced-outcome constructor

**What to build:** A valid produced Extraction run outcome has one definition, and it is the one
production uses: the outcome accumulator. Extraction run presentation's tests build every produced
outcome through the accumulator, so they never hold an outcome production could not produce. The
validating produced-outcome constructor, which only tests still call, is deleted together with its
own validation test. Command-line behaviour does not change.

Presentation's produced-outcome test helper keeps its signature, so its fifteen callers keep their
arguments. It builds the outcome by folding tallies into the accumulator:

- a shape with more emitted images than documents is split across that many documents' tallies;
- a covers outcome records covers and finishes with cover intent;
- conversion and GIF-routing facts come from seeding Applicable outcome facts and recording the
  matching roles.

Presentation's opaque-warning test entry point is unaffected.

With the constructor gone, the observation module's test file holds only the accumulator tests that
ticket 03 moved there.

**Blocked by:** 03 — Outcome classification is tested at the outcome accumulator (it removes the
last run-test callers of the constructor)

**Status:** done

Spec: `.scratch/emitted-image-tally/spec.md` (ADR-0017). User stories 18, 29, 31–34, and the
"Extraction run presentation's produced-outcome test helper" testing decision.

## Acceptance criteria

- [x] Presentation's produced-outcome test helper builds outcomes through the accumulator. All fifteen callers are unchanged, and every presentation assertion is unchanged
- [x] The validating produced-outcome constructor and its validation test are deleted. Nothing in the crate calls it
- [x] The Extraction run outcome and produced-output types keep their variants, fields and accessors. Extraction run presentation's production code is not edited
- [x] Comments about the deleted constructor (its "not leftover" note and doc references to it from the accumulator) are removed or rewritten, and each is called out in the change description
- [x] ADR-0006's tripwire-placement and validating-constructor paragraphs get status notes pointing to ADR-0017
- [x] All four superseded ADRs now carry their ADR-0017 notes (ADR-0004, ADR-0006, ADR-0007, ADR-0016). The glossary entries for the Emitted image tally and Document extraction facts match the code
- [x] ADR-0003 holds: every added item is crate-private and nothing new is public
- [x] Nothing under `tests/` was edited at any point in this feature. Check the whole feature's diff, not just this ticket's
- [x] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass

## Comments

- ADR-0006's tripwire-placement note already landed with ticket 02 (b26a971). Only its validating-constructor note remains for this ticket.
- The helper has fourteen call sites, not fifteen: the count above included the definition. All fourteen are unchanged.
