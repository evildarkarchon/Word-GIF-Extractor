# 01 — The Image write purpose decides source eligibility and path evidence

**What to build:** A document adapter describes an archive resource with one purpose-neutral Archive image source constructor — its name, plus an optional builder step for the declared MIME — and the Image write purpose alone decides what those facts mean. The purpose's eligibility decision becomes a single two-way answer: reject, or inspect carrying the optional name it accepts as path evidence. Normal images reject an unsafe name under the existing archive-path safety rule and otherwise inspect with the name as evidence; a required cover always inspects with no path evidence. Archive image discovery passes the accepted evidence into Image format identification (magic bytes, then accepted path extension, then declared MIME) and into the extension-fallback warning instead of reading the source again. The single-variant filtered-format action goes away: the filtered-format decision returns only its optional warning, and discovery still completes without emission for a filtered format.

This is a behaviour-preserving refactor: every file, name, warning, warning order and count stays the same. Governing decision: ADR-0008. Spec: `.scratch/image-write-purpose-evidence/spec.md` (user stories 1–3, 9–13, 19–20, 25).

**Blocked by:** None — can start immediately

**Status:** ready-for-agent

- [ ] Archive image source has exactly one constructor, taking the source name, and an optional step that adds the declared MIME; it no longer records path evidence, and the required-cover constructor is gone
- [ ] The source name is still the diagnostic name used in acquisition-failure warnings
- [ ] The purpose's eligibility decision is either reject or inspect-with-optional-path-evidence, so eligibility and evidence come from one decision about one name
- [ ] Normal images: an unsafe name is rejected; a safe name is inspected with that name offered as path evidence
- [ ] Required cover: always inspected, never offered path evidence, so a cover still uses byte evidence, then declared MIME, then the JPEG default
- [ ] Discovery uses the accepted evidence for both identification and the extension-fallback warning
- [ ] The filtered-format decision carries only an optional warning; the single-variant action type is deleted
- [ ] The EPUB adapter builds normal sources and cover-candidate sources with the one constructor plus the manifest/declared MIME; the DOCX adapter builds sources with no MIME
- [ ] New test at the pipeline's entry points: one raw source (same name, same bytes, no magic signature, recognisable extension) is emitted through extension fallback, with its warning, under normal-image traversal, and as a required cover with no declared MIME it takes the JPEG default, with its warning, never the extension
- [ ] The purpose-level test asserting that a cover-built source is unsafe for normal traversal is removed, because the new test replaces it; the archive-path safety case table stays
- [ ] Existing discovery and pipeline tests move to the new constructor without assertion changes
- [ ] Every item stays crate-private, or narrower where it already is (ADR-0003)
- [ ] `cargo test` passes, and the CLI integration suite is unchanged
