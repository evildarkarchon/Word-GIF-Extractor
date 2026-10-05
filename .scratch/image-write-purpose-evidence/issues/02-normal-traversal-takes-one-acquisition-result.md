# 02 — Normal-image traversal takes one acquisition result per source

**What to build:** A document adapter hands each resource to normal-image traversal through a single visitor method, `visit`, which takes the Archive image source and a standard `Result` holding either a mutable reader or any displayable error. The adapter no longer writes a two-armed visit-or-unreadable dispatch per resource, and the separate `unreadable` method is removed. Inside `visit`, the Image write purpose's eligibility decision is applied first, for both arms. A rejected source is skipped silently whatever the acquisition result. An eligible unavailable source records the existing acquisition-failure warning. An eligible readable source goes through Archive image discovery exactly as before. The visitor stays scoped to normal-image traversal's closure and keeps its name.

This is a behaviour-preserving refactor: every file, name, warning, warning order and count stays the same, and all discovery warnings still come before all conversion warnings. Governing decision: ADR-0015. Spec: `.scratch/image-write-purpose-evidence/spec.md` (user stories 4–5, 16–17, 19, 21).

**Blocked by:** 01 — The Image write purpose decides source eligibility and path evidence (the visitor consumes the reshaped eligibility decision, and both tickets edit the same visitor code)

**Status:** ready-for-agent

- [ ] The normal-image visitor has one method, `visit`, taking a source and a `Result` of a mutable reader or a displayable error; `unreadable` is deleted
- [ ] ZIP entry errors and EPUB resource-unavailability facts are both accepted as the error without conversion
- [ ] Eligibility is decided once, before either arm, which removes the visitor's separate re-check for unreadable sources
- [ ] An unsafe-named source that is also unavailable stays silent: no warning, no count
- [ ] A safe-named unavailable source records the existing acquisition-failure warning
- [ ] The DOCX adapter makes one `visit` call per ZIP entry, passing the entry-open result mapped to a reader; its fallback name for unnamed entries is unchanged
- [ ] The EPUB adapter's normal traversal calls `visit` with the readable payload inside the EPUB resource archive's scoped acquisition, and with the unavailability fact when acquisition reports the resource unavailable; the Archive resource identity exclusion used by cover fallback is unchanged
- [ ] The unavailable-source eligibility test is rewritten against the single method with error results: an unsafe name stays silent, a safe name warns
- [ ] Existing pipeline tests move from `unreadable` to `visit` with error results, without assertion changes
- [ ] DOCX adapter, EPUB adapter and Document extraction tests pass without assertion edits
- [ ] `cargo test` passes, and the CLI integration suite is unchanged
