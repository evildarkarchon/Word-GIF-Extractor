# 01 — Image write pipeline records each emitted image into an Emitted image tally

**What to build:** Every image the Image write pipeline emits is recorded exactly once into one
Emitted image tally, under its Image write purpose (normal image or cover) and a destination-free
Emitted image role. An image write result becomes a tally plus warnings. The pipeline's four-count
type and its normal-image-output marker are deleted. Command-line behaviour does not change.

The tally lives in a new leaf module that imports nothing. It can:

- record one normal image with a role;
- record one cover with a role;
- combine with another tally by addition;
- read the normal-image total, the cover total, the emitted total (normal images plus covers,
  derived and never stored), and the converted, conversion-skipped and GIF-routed totals.

An empty tally is the starting value, and nothing else can change one. The tally's role has four
values: routed GIF, converted, conversion-skipped, preserved. The pipeline keeps its own Emitted
image role, which still carries the borrowed routed destination as ADR-0007 decided. The pipeline
maps that role onto the tally's role inside the one match that increments counts today, so the
mapping replaces the increments rather than adding a second counting site.

The normal-image visitor records normal images and the required-cover path records covers, because
each is statically bound to its Image write purpose. Both reach the one counting function, so the
purpose must arrive from the caller, not as a value passed into that function. No runtime purpose
value is introduced. A
required cover that is a routed GIF records as a routed cover. The tally does not police which roles
a cover may take: a conversion fallback completes without emitting, so a cover never records as
conversion-skipped.

Appending one result to another combines the tallies and concatenates warnings in order. Prepending
earlier facts onto a failure keeps its meaning. EPUB cover extraction's retry and fallback folding
therefore behave as before. Building a result from a tally and warnings stays crate-visible.
Production still calls it once inside the pipeline.

**This ticket is the expand step.** Document extraction's translation from a pipeline result builds
its existing emitted-image totals and three-way output purpose from the tally. Document extraction
facts, the outcome accumulator and every layer above them keep their current interfaces. Ticket 02
removes this bridge.

**Blocked by:** None — can start immediately

**Status:** ready-for-agent

Spec: `.scratch/emitted-image-tally/spec.md` (ADR-0017). User stories 1–9 and 30, and the
"Mechanical rewrites" testing decision.

## Acceptance criteria

- [ ] A new crate-private leaf module owns the Emitted image tally and its four-way role, with the interface above and nothing else. Every new item has a doc comment
- [ ] The tally module imports nothing from the crate. The pipeline, Document extraction and the Extraction run observation module may import it
- [ ] The role mapping (pipeline Emitted image role → tally role) stays in the one counting function, replacing its per-role increments. Each of that function's two callers records under its own statically known Image write purpose: the normal-image visitor records normal images, the required-cover path records covers. No runtime purpose value is introduced, and no second counting site exists
- [ ] Normal images and required covers record through separate operations. A routed-GIF required cover records as a routed cover and still counts toward the GIF-routed total
- [ ] The routed GIF destination stays bound to the routing decision inside the pipeline. The tally's role carries no destination
- [ ] An image write result is a tally plus warnings. The four-count type and the normal-image-output marker are deleted. Append and prepend keep their ordering semantics
- [ ] Document extraction derives its existing totals and output purpose from the tally. Its public-to-crate interface, the accumulator and the partition debug assertion are unchanged in this ticket
- [ ] Image write pipeline tests, EPUB and DOCX adapter tests, and EPUB cover extraction tests read tally totals instead of count fields, and no asserted value changes
- [ ] EPUB cover extraction's scripted-attempt helpers record a cover or normal images into a tally instead of hand-building counts. The ADR-0005 seam itself is unchanged
- [ ] The pipeline test that hand-builds results to check they carry their facts through the fold builds them from tallies
- [ ] The Extraction run tests' scripted-facts helpers build their pipeline results from tallies ("one cover written", "two normal images, one converted"). No run-test assertion changes
- [ ] Comments that name the deleted count type or marker, or that translate counts into "a written cover", are updated rather than dropped. Each removed or rewritten comment is called out in the change description
- [ ] ADR-0007's counts paragraph gets a status note pointing to ADR-0017
- [ ] Nothing under `tests/` is edited
- [ ] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
