# 03 — The required cover is a single direct call taking one request type

**What to build:** The EPUB adapter writes a required cover with one direct call to the Image write pipeline, passing the request, one Archive image source and one acquisition result of the same `Result` shape normal-image traversal uses. The call returns the existing required-cover outcome: retry or completed, each carrying its Image write result, or an Image write failure that keeps partial facts when emission fails. "Exactly one source" is now a property of the call, so the required-cover visitor, its disposition enum, its exactly-one-source guard and both runtime protocol errors ("no source", "more than one source") are deleted. The two field-identical request types merge under the existing normal-images request name, with one purpose-neutral constructor taking the output directory and base name. The name avoids "placement" (see ADR-0008).

Disposition rules are unchanged:
- an unavailable source or a read failure gives retry, with the acquisition warning;
- unidentified, filtered and conversion-declined covers complete;
- an emitted cover completes and uses singular output naming.

This is a behaviour-preserving refactor: every file, name, warning, warning order and count stays the same. Governing decision: ADR-0008. Spec: `.scratch/image-write-purpose-evidence/spec.md` (user stories 6–8, 14–15, 18–19, 22–23).

**Blocked by:** 02 — Normal-image traversal takes one acquisition result per source (the cover call reuses the acquisition-result shape established there, and both tickets edit the same pipeline module)

**Status:** ready-for-agent

- [ ] The required-cover entry point takes the request, one source and one acquisition result, and returns the required-cover outcome directly, with no visitor or closure
- [ ] The required-cover visitor type, its disposition enum, its exactly-one-source guard and both protocol errors are deleted
- [ ] One request type remains, under the existing normal-images request name, with one purpose-neutral constructor taking output directory and base name; the required-cover request type is gone
- [ ] The EPUB cover attempt calls the entry point inside the EPUB resource archive's scoped acquisition and returns the outcome through the acquisition's generic result, so the payload borrow never escapes
- [ ] On an unavailable resource, the cover attempt calls the same entry point with the unavailability fact, so the pipeline (not the adapter) produces the retry disposition and its acquisition warning
- [ ] A failure carried by the acquisition itself still becomes an Image write failure, as it does today
- [ ] Retry versus completed outcomes, retained Image write facts, cover candidate ordering, Archive resource identity exclusion and fallback all behave as before
- [ ] Existing required-cover pipeline tests move to the direct call and merged request without assertion changes; tests that only exercised the deleted protocol errors are removed
- [ ] EPUB cover extraction tests (canned attempts, ADR-0005), Document extraction tests, EPUB adapter tests and the CLI integration tests pass without assertion edits
- [ ] The Emitted image role, prepared-image preparation and its optional result (ADR-0007), and EPUB resource archive acquisition mechanics (ADR-0001) are unchanged
- [ ] `cargo test` passes
