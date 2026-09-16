# Bind Archive image source evidence to Image write purpose

Status: ready-for-agent

Decision: `docs/adr/0013-bind-archive-image-evidence-to-image-write-purpose.md`

## Problem Statement

Maintainers can currently select an Archive image source shape independently from its Image write purpose. The type system therefore permits normal-image traversal to receive required-cover source facts and required-cover traversal to receive normal source facts, even though the two purposes have different eligibility, evidence, warning, and completion rules. One crossed pairing silently rejects the source; the other can incorrectly make a required cover's manifest-path extension available as Image format evidence.

This parallel choice spreads an invariant across document adapters, Archive image discovery, purpose policy, and visitor construction. A maintainer changing evidence precedence or adding a call site must remember a relationship the interface does not express, and tests can exercise invalid runtime combinations that production should be unable to construct.

## Solution

Deepen the Image write pipeline by binding source facts to Image write purpose at its existing visitor seam. Normal traversal accepts a borrowed `NormalImageSource`; required-cover traversal accepts a borrowed `RequiredCoverSource`. Each value exposes only the facts its purpose permits, each is cheap to copy, and crossed visitor/source pairings do not compile.

Archive image discovery owns one closed private dispatch over the two real source shapes. It preserves all existing source eligibility, evidence precedence, bounded reads, warnings, and retry or completion behavior. Purpose-specific conversion preparation, Image file emission, result accounting, EPUB cover extraction, and the library interface remain unchanged.

## User Stories

1. As a maintainer, I want normal and required-cover source facts to have different types, so that their evidence rules cannot be crossed accidentally.
2. As a maintainer, I want a normal-image visitor to accept only a normal-image source, so that required-cover facts cannot silently disappear during normal traversal.
3. As a maintainer, I want a required-cover visitor to accept only a required-cover source, so that a manifest-path extension cannot become cover evidence accidentally.
4. As a maintainer, I want invalid purpose/source pairings rejected by the compiler, so that correctness does not depend on runtime guards.
5. As a DOCX user, I want extracted images to use the same byte, extension, and format-filtering behavior as before, so that the refactor does not change my output.
6. As an EPUB user extracting normal images, I want magic bytes, safe manifest-path extension, and declared MIME considered in the same order as before, so that accepted resources do not change.
7. As an EPUB user extracting a required cover, I want magic bytes considered before declared MIME and the manifest-path extension ignored, so that cover evidence follows the intended policy.
8. As an EPUB user, I want an unidentified required cover to retain the existing JPEG default and warning, so that fallback behavior remains stable.
9. As an EPUB user, I want a required-cover acquisition failure to remain retryable, so that later Cover candidates are still attempted.
10. As an EPUB user, I want filtering, JPEG defaulting, conversion skip, conversion failure, and successful emission to remain terminal cover outcomes, so that only acquisition failure triggers a retry.
11. As a user, I want warning facts to retain their existing order, so that terminal output remains stable and understandable.
12. As a user, I want output filenames, collision handling, conversion, GIF routing, and emitted-image counts to remain unchanged, so that this architecture change has no workflow-visible side effects.
13. As a maintainer, I want unsafe normal source paths rejected before their readers are touched, so that Archive image discovery preserves its current safety rule.
14. As a maintainer, I want evidence reads to retain the 1,027-byte limit, so that rejected resources are not buffered unnecessarily.
15. As a maintainer, I want accepted resources to retain the already-read evidence prefix when the remaining payload is acquired, so that bytes are neither lost nor reread.
16. As a maintainer, I want source values to borrow their strings and be cheap to copy, so that EPUB acquisition can reuse the same facts for either `visit` or `unreadable` without allocation or cloning.
17. As a maintainer, I want normal source construction to distinguish a named source from a declared source, so that optional MIME evidence is explicit.
18. As a maintainer, I want required-cover construction to require declared MIME, so that every constructed cover source is complete for the real caller contract.
19. As a maintainer, I want normal-image pipeline names to state their purpose, so that the interface does not look more generic than its behavior.
20. As a maintainer, I want the shared discovery implementation to remain private and closed over the two real purposes, so that extension machinery is added only when another purpose exists.
21. As a contributor, I want conversion preparation to stay separate from Archive image discovery, so that the refactor remains narrow and reviewable.
22. As a contributor, I want existing DOCX and EPUB adapters to keep ownership of archive acquisition, so that ZIP mechanics do not leak into the Image write pipeline interface.
23. As a test author, I want purpose behavior tested through purpose-specific pipeline visitors, so that tests exercise the same seam as production callers.
24. As a test author, I want focused recognition and bounded-reader tests to remain close to Archive image discovery, so that low-level byte mechanics retain precise coverage.
25. As a test author, I want the obsolete mismatched-pairing runtime test removed, so that the suite does not preserve an impossible state as supported behavior.
26. As a maintainer, I want all current integration behavior retained, so that unchanged DOCX, EPUB, cover fallback, conversion, emission, and Extraction run tests guard the refactor.
27. As a library consumer, I want the existing four-item library interface unchanged, so that this internal deepening creates no compatibility burden.
28. As a future maintainer, I want the rejected raw-parameter and generic-associated-type designs recorded, so that later reviews do not repeat the same trade-off analysis.

## Implementation Decisions

- Introduce a borrowed, crate-private `NormalImageSource` value. It is cheap to copy and carries a source name plus optional declared MIME.
- Normal source construction is atomic: one constructor represents a named source without MIME, and one represents a declared source with MIME. The source name remains both diagnostic identity and potential path-extension evidence, subject to the existing safety check.
- Introduce a borrowed, crate-private `RequiredCoverSource` value. It is cheap to copy and requires both diagnostic name and declared MIME.
- A required cover's diagnostic name is never exposed as path-extension evidence. Required-cover identification remains magic bytes, then declared MIME, then the existing JPEG default when neither identifies an Image format.
- Replace the generic normal-image interface names with purpose-specific names: `NormalImageWriteRequest`, `NormalImageWriteVisitor`, and `write_normal_images`.
- The normal-image visitor accepts only `NormalImageSource`. The required-cover visitor accepts only `RequiredCoverSource`. Both source values are passed cheaply by value and may be reused after a scoped acquisition attempt.
- Remove the generic `ArchiveImageSource` from the Image write pipeline interface. Perform a direct migration of every crate-private caller; do not add a compatibility module or transitional conversion path.
- Archive image discovery owns a closed private dispatch over normal and required-cover source facts. One exhaustive decision point selects source eligibility, available evidence, unidentified-format behavior, and filtered-format behavior.
- Keep shared magic-byte, extension, MIME, and bounded-read mechanics private to Archive image discovery.
- Remove discovery decisions from the generic Image write purpose interface. Purpose-specific conversion preparation remains separate and retains the current normal-image versus required-cover outcomes.
- Preserve exact normal evidence precedence: magic bytes, safe path extension, then declared MIME.
- Preserve exact required-cover evidence precedence: magic bytes, then declared MIME; the diagnostic path never participates.
- Preserve the current 1,027-byte evidence window, unreadable-source facts, extension-fallback facts, cover-default facts, format filtering, accepted-payload tail read, and reader-consumption behavior.
- Preserve warning ordering. Normal traversal retains all discovery warnings in source order followed by conversion warnings; required-cover attempts retain discovery warnings before conversion warnings.
- Preserve error semantics. Normal acquisition failures remain non-fatal; required-cover acquisition failures remain retryable; traversal and Image file emission failures remain fatal while retaining partial facts.
- Preserve the required-cover traversal cardinality checks for zero or multiple attempts. This change removes purpose/source mismatch, not the scoped traversal protocol.
- Preserve Image file emission, output naming, collisions, Conversion policy, GIF routing, Emitted image role, counts, Document extraction facts, Extraction run outcomes, and terminal wording.
- Keep the EPUB Cover attempts seam, its generic Archive resource identity, and the production adapter placement unchanged under ADR-0005.
- Keep every new or renamed item crate-private under ADR-0003. The library continues to expose exactly `Args`, `run_cli`, `TerminalOutput`, and `Capture`.
- Treat the new source values as implementation values expressing existing Archive image discovery and Image write purpose concepts. Do not add glossary entries to `CONTEXT.md`.
- Use the closed two-purpose design accepted in ADR-0013. Raw visitor parameters were rejected because they hide fact meaning; associated source/evidence types were rejected because two real purposes do not justify a larger generic interface.
- Do not add a compile-fail harness. Nominal visitor parameter types carry the invariant directly.
- Preserve accurate existing comments. Rewrite a comment only when the changed interface makes it inaccurate, report any such rewrite in the implementation handoff, and add concise doc comments to every added or substantially rewritten method.

## Testing Decisions

- The confirmed highest test seam is the purpose-specific Image write pipeline visitor interface. Tests of source eligibility, evidence precedence, warning facts, reader consumption, and required-cover retry or completion cross this seam.
- Keep focused internal tests for Image format recognition and bounded-reader mechanics where precise byte-level assertions provide useful locality. These tests cover implementation mechanics, not the removed purpose/source pairing.
- Remove or replace the test that constructs a required-cover source and asks the normal Image write purpose to reject it. The new type relationship makes that state unconstructible.
- Verify an unsafe normal source remains silent and leaves its reader untouched.
- Verify normal magic evidence outranks a conflicting safe extension and declared MIME.
- Verify a safe normal extension remains eligible evidence, outranks declared MIME, and emits the existing extension-fallback warning.
- Verify normal declared MIME is consulted only after magic and extension evidence fail.
- Verify a required cover never uses its diagnostic path extension as evidence, even when that extension names a supported Image format.
- Verify required-cover magic evidence outranks declared MIME.
- Verify required-cover MIME is used after magic evidence fails.
- Verify unidentified required-cover evidence retains the existing JPEG default warning and completion behavior.
- Verify filtered required-cover evidence retains the existing warning and terminal completion behavior.
- Verify normal acquisition and tail-read failures remain non-fatal warning facts, while required-cover acquisition failures retain retry disposition.
- Verify warning order remains unchanged across discovery, conversion, later sources, cover retry, and normal fallback.
- Reuse existing Image write pipeline tests as prior art for reader-position assertions, warning phase order, partial facts, singular and multiple output, collision handling, and incremental emission.
- Reuse existing DOCX tests as prior art for archive encounter order and extension-fallback behavior.
- Reuse existing EPUB tests as prior art for magic-first identification, declared MIME, resource acquisition warnings, deterministic traversal, and exact emitted bytes.
- Reuse existing EPUB cover extraction tests as prior art for Cover candidate ordering, duplicate Archive resource identity exclusion, retry, fallback, partial facts, and warning order. Their integer test adapter remains unchanged.
- Existing integration assertions should remain behaviorally unchanged. Mechanical updates for renamed crate-private interfaces are acceptable; changing expected selected documents, emitted files, warnings, counts, or outcomes is evidence of a regression.
- Complete validation with repository formatting, focused Image write pipeline and document-adapter tests, the full test suite, `cargo check`, and the release build.

## Out of Scope

- Changing user-facing CLI flags, defaults, messages, warnings, progress, or terminal summaries.
- Changing which documents are selected or how Selected documents retain EPUB declarations and Output placement.
- Changing Archive image discovery's supported Image formats, magic signatures, MIME mappings, safety rules, evidence window, or evidence precedence.
- Reorganizing Conversion policy, purpose-specific preparation, Image file emission, output naming, collision handling, result accounting, or Emitted image role.
- Changing EPUB cover policy, Cover candidate ordering, Archive resource identity, retry, exclusion, fallback, or the Cover attempts seam.
- Deepening the EPUB resource archive or changing its parallel extraction plan as part of this work.
- Adding another Image write purpose, a generic extension mechanism, a port, an adapter, or a mock at the source/purpose seam.
- Adding a compile-fail test framework or a temporary compatibility interface.
- Widening the public library interface or moving in-crate unit tests across it.
- Adding implementation-specific source terminology to `CONTEXT.md`.
- Implementing the refactor as part of publishing this spec.

## Further Notes

ADR-0013 records the accepted interface shape and the rejected alternatives. ADR-0003 continues to govern the narrow library interface, ADR-0005 continues to govern generic EPUB cover extraction and its production/test adapters, and ADR-0007 continues to govern the closed Emitted image role.

The test seams were explicitly accepted during the preceding design tree: purpose behavior crosses the existing Image write pipeline visitor seam, while focused recognition and bounded-reader mechanics retain an internal seam. No new port or adapter is justified because all dependencies in this deepening are in-process.

This specification is synthesis and local issue publication only. ADR-0013 and this spec are the only repository changes made for the design; the implementation remains future work.
