# Let the Image write purpose own source evidence

Status: ready-for-agent

Governing decision: ADR-0015 (Let the Image write purpose interpret Archive image sources). Glossary terms used here — Archive image source, Image write purpose, Archive image discovery, Image write pipeline, Image write policy, Image file emission, EPUB resource archive, EPUB cover extraction, Emitted image role — are defined in `CONTEXT.md`.

## Problem Statement

A maintainer adding to or changing a document adapter has to know a rule the Image write pipeline never states: the Archive image source must be built with the constructor that matches the entry point it will be handed to. A normal source and a required-cover source differ only in whether the source name is recorded as path evidence, and that single optional field silently decides two Image write purpose questions — whether a normal source is eligible, and whether its name may identify its Image format. If an adapter builds a cover-style source and hands it to normal-image traversal, the image is rejected as unsafe and disappears with no warning, no count and no failure. Nothing in the types prevents it, and no test would notice.

The same leak shows up as repetition. Every adapter — DOCX entries, EPUB normal images and the EPUB required cover — writes its own "acquire the payload, then either visit it or report it unreadable" step against two different visitors whose `unreadable` methods behave differently. The normal visitor re-checks eligibility to stay silent for unsafe names, duplicating Archive image discovery's own guard, and the cover visitor checks at runtime that its traversal supplied exactly one source, a rule the caller can only learn by tripping it. The Image write purpose is spelled out five times across paired request types, entry methods, visitors, source constructors and purpose types, and the caller has to keep them all lined up.

## Solution

The document adapter hands the Image write pipeline only what it knows about a resource — its name, any declared MIME, and either a reader or the reason the resource was unavailable — and the entry point it calls decides the Image write purpose. The purpose, not the adapter, decides whether the source is eligible and whether its name is format evidence. A mismatched pairing becomes impossible to express, because there is nothing left to pair.

Normal-image traversal keeps its scoped visitor but takes one acquisition result per source through one method. The required cover becomes a single direct call that takes one source and returns its retry-or-completed disposition, so "exactly one source" is a property of the call rather than a runtime check. The two identical request types become one.

Users of the command-line tool see no difference: every file, name, warning, warning order and count stays exactly as it is today.

## User Stories

1. As a maintainer writing a document adapter, I want to describe an archive resource with one purpose-neutral constructor, so that I cannot pick the wrong constructor for the entry point I call.
2. As a maintainer writing a document adapter, I want the entry point I call to choose the Image write purpose, so that purpose is decided in exactly one place.
3. As a maintainer writing a document adapter, I want to pass a resource's declared MIME as an optional fact, so that adapters without MIME declarations, such as DOCX, simply omit it.
4. As a maintainer writing a document adapter, I want to hand normal-image traversal a reader or an unavailability reason through one method, so that I do not write a two-armed visit-or-unreadable dispatch for every resource.
5. As a maintainer writing a document adapter, I want any displayable error to be acceptable as the unavailability reason, so that ZIP entry errors and EPUB resource-unavailability facts both fit without conversion.
6. As a maintainer of the EPUB adapter, I want the required-cover write to run inside the EPUB resource archive's scoped payload acquisition and return its outcome through it, so that the payload borrow never escapes and no visitor is needed.
7. As a maintainer of the EPUB adapter, I want an unavailable cover candidate to go through the same required-cover entry point with its unavailability reason, so that the retry disposition and its acquisition warning are produced by the pipeline, not reconstructed by the adapter.
8. As a maintainer of EPUB cover extraction, I want the required-cover outcome to keep its retry-versus-completed disposition and its retained Image write facts, so that cover candidate ordering, Archive resource identity exclusion and fallback behave exactly as before.
9. As a maintainer of the Image write pipeline, I want eligibility and path evidence decided as one purpose decision, so that the two answers about one name can never drift apart.
10. As a maintainer of the Image write pipeline, I want a normal-images purpose that rejects unsafe names and otherwise offers the name as path evidence, so that the existing safety rule and extension fallback are preserved.
11. As a maintainer of the Image write pipeline, I want a required-cover purpose that always inspects and never offers path evidence, so that covers keep using byte evidence, then declared MIME, then the JPEG default.
12. As a maintainer of the Image write pipeline, I want Archive image discovery to read the accepted path evidence from the purpose's decision, so that identification and the extension-fallback warning use the same value without re-reading the source.
13. As a maintainer of the Image write pipeline, I want the single-variant filtered-format action removed, so that discovery no longer destructures a value that offers no choice and the purpose returns only its optional warning.
14. As a maintainer of the Image write pipeline, I want one request type with one purpose-neutral constructor for output directory and base name, so that the request no longer pretends to carry a purpose.
15. As a maintainer of the Image write pipeline, I want the required-cover visitor, its disposition state and its "no source" and "more than one source" errors deleted, so that the cover path has no runtime protocol to violate.
16. As a maintainer of the Image write pipeline, I want an unsafe-named normal source that is also unavailable to stay silent, so that eligibility is applied identically whether or not acquisition succeeded.
17. As a maintainer of the Image write pipeline, I want normal-image warnings to stay ordered with all discovery warnings before all conversion warnings, so that warning order is unchanged for documents already in use.
18. As a maintainer reading Document extraction, I want the Image write pipeline's results, failures and counts to keep their current shape, so that Document extraction facts, warnings and output-purpose classification need no change.
19. As a test author, I want to exercise normal and required-cover behaviour by building purpose-neutral sources and calling the pipeline's entry points with in-memory readers, so that tests cross the same interface production adapters do.
20. As a test author, I want a test showing that one raw source means different things to the two purposes, so that the property which used to be broken by constructor mismatch is pinned at the interface where purpose is now chosen.
21. As a test author, I want the unavailable-source eligibility test expressed through the single acquisition-result method, so that the silent skip of unsafe names stays pinned.
22. As a test author, I want EPUB cover extraction's canned attempts to keep working unchanged, so that cover policy remains testable without opening a real EPUB.
23. As a user of the command-line tool, I want every extracted file, output name, warning, warning order and summary count to stay the same, so that this change is invisible to me.
24. As a future architecture reviewer, I want the decision not to invert iteration into a pipeline-driven source trait recorded with its reason, so that I do not propose moving the EPUB fallback's exclusion rule across a new seam without knowing why it was declined.
25. As a future reader of the glossary, I want Archive image source defined as purpose-neutral facts and Image write purpose defined as their interpreter, so that the vocabulary states where the evidence decision lives.

## Implementation Decisions

- **Behaviour-preserving refactor.** Observable output is byte-identical: files, names, warnings, warning order and counts. The command-line integration suite does not change. Behaviour fixes found along the way, such as the output-naming overlap noted in the architecture review, go in separate later changes.
- **Archive image source becomes purpose-neutral.** It has one constructor taking the source name and an optional builder step adding the declared MIME. It no longer stores path evidence. Its name doubles as the diagnostic name used in acquisition-failure warnings, as it does today for both kinds.
- **The purpose decides eligibility and path evidence together.** The Image write purpose's eligibility decision becomes a two-way value: reject, or inspect carrying the optional name accepted as path evidence. Normal images reject an unsafe name and otherwise inspect with the name as evidence, under the existing archive-path safety rule. A required cover always inspects with no path evidence. Archive image discovery threads the accepted evidence into Image format identification (magic bytes, then accepted path extension, then declared MIME) and into the extension-fallback warning.
- **The filtered-format decision loses its action.** It returns only the optional warning. Discovery always completes without emission for a filtered format, as it does now. The rest of the purpose trait stays as it is: static dispatch, two implementations, and the unidentified-format and conversion decisions with their action enums.
- **Normal-image visitor: one method.** The scoped visitor offered to normal-image traversal keeps its name and the method name `visit`, which now takes the source and a standard `Result` of either a mutable reader or a displayable error. Eligibility is applied first, for both arms. A rejected source is skipped silently whatever the acquisition result. An eligible unavailable source records the existing acquisition-failure warning. An eligible readable source goes through discovery exactly as before. The separate `unreadable` method is removed.
- **Required cover: a direct call.** The required-cover entry point takes the request, one source and one acquisition result of the same `Result` shape. It returns the existing required-cover outcome: retry or completed, each carrying its Image write result, or an Image write failure retaining partial facts when emission fails. Disposition rules are unchanged:
  - an unavailable source or a read failure gives retry with the acquisition warning;
  - unidentified, filtered and conversion-declined covers complete;
  - an emitted cover completes, and uses singular output naming.
- **Deleted from the cover path.** The required-cover visitor type, its disposition enum, its "exactly one source" guard and both runtime protocol errors are deleted.
- **One request type.** The two field-identical request types merge under the existing normal-images request name, with one purpose-neutral constructor taking output directory and base name. The name deliberately avoids "placement" (see ADR-0015).
- **Adapters.**
  - The DOCX adapter makes one visitor call per ZIP entry, passing the entry-open result mapped to a reader. Its source-name fallback for unnamed entries stays as it is.
  - The EPUB adapter's normal-image traversal calls the visitor with the readable payload inside the resource archive's scoped acquisition, and with the unavailability fact when acquisition reports the resource unavailable.
  - The EPUB adapter's cover attempt calls the required-cover entry point inside the scoped acquisition, returning the outcome through the acquisition's generic result. On an unavailable resource it calls the same entry point with the unavailability fact. A failure carried by the acquisition itself still converts into an Image write failure, as it does now.
  - The EPUB adapter's cover candidates still build sources with the declared MIME. Normal EPUB images still add the manifest MIME.
- **Untouched.**
  - the Emitted image role and prepared-image preparation, including the conversion decision's optional result and its `unreachable!` on the normal path, which ADR-0007 assigned to the purpose trait;
  - image write counts, results and failures, and the normal-image output flag;
  - the cover-attempts seam and its canned test adapter (ADR-0005);
  - EPUB resource archive acquisition mechanics (ADR-0001);
  - Document extraction and everything above it.
- **Visibility.** Every item stays crate-private, or narrower where it already is (ADR-0003).

## Testing Decisions

- **What a good test is here.** It builds Archive image sources with the one public constructor, feeds in-memory readers or unavailability reasons through the Image write pipeline's two entry points, and asserts on the returned Image write facts (counts, warnings, retry or completed disposition) and on files written to a temporary output directory. It does not assert on how the purpose reached its decision, and it does not reach past the entry points.
- **The primary test seam is the Image write pipeline's entry points**: normal-image traversal via its visitor, and the direct required-cover call. No new seam is introduced. Purpose-level tests stay only for rules that are inherently below the entry point, the archive-path safety cases, and keep using their existing table of unsafe paths.
- **New test:** one raw source, same name and same bytes with no magic signature and a recognisable extension, is used with each purpose. Under normal-image traversal it is emitted through extension fallback with the extension-fallback warning. As a required cover with no declared MIME it takes the JPEG default with its warning and never the extension. This replaces the purpose-level test that pinned the safety-follows-evidence pairing, which the new interface makes unconstructable.
- **Rewritten test:** the unavailable-source eligibility test is expressed through the single visitor method with error results: an unsafe name stays silent and a safe name warns.
- **Mechanical updates:** existing pipeline, discovery and required-cover tests move from the old constructors, request constructors and `unreadable` method to their replacements. Their assertions do not change.
- **Unchanged suites, used as evidence:** EPUB cover extraction tests (canned attempts), Document extraction tests, EPUB and DOCX adapter tests, and the command-line integration tests should pass without assertion edits. Under the behaviour-preserving constraint, their staying green is the evidence that behaviour is preserved.
- **Prior art:**
  - the pipeline's existing in-memory reader tests that drive normal traversal through a closure over the visitor;
  - the required-cover pipeline tests that assert retry versus completed outcomes;
  - the purpose module's unsafe-path case table.
- **Completion bar:** the full test suite passes.

## Out of Scope

- Binding the pipeline and output placement into a per-document write target, and removing the EPUB adapter's plan copy of the resource catalog (architecture review candidate 04).
- Replacing the normal-image output flag with a closed "emitted for" value that Document extraction maps to its output purpose (architecture review candidate 03).
- Inverting control so that the pipeline drives iteration over an adapter-implemented source trait (rejected in ADR-0015).
- Collapsing the Image write purpose trait into a closed enum, or changing the conversion decision's optional result (ADR-0007's deferred item).
- Moving output naming into Image file emission, and the naming-namespace overlap between collision suffixes and multi-image numbering (architecture review candidate 05).
- Removing the base name from pipeline conversion warnings.
- Any change to observable command-line output.

## Further Notes

- `CONTEXT.md` has already been updated: Image write purpose now states that it interprets Archive image source facts, and Archive image source is newly defined. ADR-0015 has already been written. Both were written ahead of the code change they govern, in the manner of ADR-0006 and ADR-0007, and are not yet committed.
- The architecture review found one piece of independent evidence of the current hazard: the purpose module's own test asserts that a cover-built source is unsafe for normal traversal. That test exists because the pairing has to be remembered by callers, and its replacement in this spec is the clearest sign the leak is closed.
- Suggested commit order, following the repository's recent precedent: the documentation changes (ADR-0015 and the glossary edits) first, then the pipeline refactor and the adapter call-site updates together, so that every commit builds and passes tests.
