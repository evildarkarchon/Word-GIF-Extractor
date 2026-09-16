# 01: Bind Archive image evidence to purpose-specific visitors

**What to build:** Deepen the Image write pipeline so normal-image traversal and required-cover traversal each accept only their matching borrowed source facts. Crossed purpose/source pairings must become unconstructible while DOCX extraction, normal EPUB extraction, required-cover retry and completion, conversion, Image file emission, warnings, naming, counts, and the public library interface retain their existing behavior.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [x] Normal traversal accepts a borrowed, cheap-to-copy normal-image source whose atomic constructors distinguish a named source from a declared source with MIME evidence.
- [x] Required-cover traversal accepts a borrowed, cheap-to-copy required-cover source that requires both its diagnostic name and declared MIME; neither visitor accepts the other purpose's source type.
- [x] Normal-image pipeline names state their purpose, all crate-private DOCX and EPUB callers use the purpose-specific interfaces directly, and the generic Archive image source interface is removed without a compatibility or conversion layer.
- [x] Archive image discovery uses one private, exhaustive dispatch over the two real purposes while purpose-specific conversion preparation remains separate.
- [x] Normal discovery preserves unsafe-name rejection before reader access and preserves evidence precedence of magic bytes, safe path extension, then declared MIME, including the existing extension-fallback warning.
- [x] Required-cover discovery ignores diagnostic-path extensions, uses magic bytes before declared MIME, and preserves JPEG defaulting, format-filter completion, acquisition retry, conversion completion, and warning behavior.
- [x] Discovery preserves the 1,027-byte evidence limit, retains the already-read prefix for accepted payloads, keeps normal acquisition failures non-fatal, and preserves warning ordering and partial facts across later sources or cover attempts.
- [x] Purpose behavior is covered through the purpose-specific pipeline visitor seam; focused recognition and bounded-reader tests remain close to discovery, and the obsolete runtime test for a crossed purpose/source pairing is removed or replaced.
- [x] Output bytes, filenames, collisions, GIF routing, conversion outcomes, emitted-image roles and counts, EPUB cover ordering and fallback, and all existing CLI-visible behavior remain unchanged.
- [x] All new or renamed items remain crate-private, the library continues to expose exactly its existing four items, and repository formatting, focused tests, the full test suite, `cargo check`, and the release build pass.


## Implementation

Completed on 2026-09-16. Normal and required-cover visitors now accept only their matching borrowed, `Copy` source facts. Discovery selects evidence and completion rules in one private exhaustive dispatch; conversion preparation and the four-item public interface remain unchanged.

Validation: `cargo fmt --check`, `git diff --check`, `cargo check`, focused pipeline tests (45), focused document-extraction tests (44), `cargo test` (244 unit and 12 CLI integration tests), and `cargo build --release` passed. Independent standards and spec reviews found no issues.

Comments describing removed generic interfaces or relocated tests were updated; comments attached to deleted discovery-policy code and obsolete/duplicate tests were removed with that code. Accurate comments were retained, including relocated safety-fixture explanations.
