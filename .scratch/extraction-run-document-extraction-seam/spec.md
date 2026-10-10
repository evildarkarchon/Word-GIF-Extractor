# Give the Extraction run a Document extraction seam

Status: done

Governing decision: ADR-0016 (Give the Extraction run a Document extraction seam). Origin: candidate 01 of the 2026-10-05 architecture review, settled in a grilling session before any code was written. Glossary terms used here — Extraction run, Extraction run request, Extraction run outcome, Extraction run observation, Extraction run presentation, Document selection, Document search surface, EPUB declarations, Selected document, Document extraction, Document extraction outcome, Document extraction facts, Document extraction warning, Document extraction error, Applicable outcome facts, EPUB cover policy, Image write policy — are defined in `CONTEXT.md`.

## Problem Statement

A maintainer who wants to test how an Extraction run sequences its work cannot reach the run without the whole stack underneath it. The run hard-wires its collaborators: Document selection always reads the real filesystem and always parses real EPUB files for their declarations, and Document extraction always opens real archives and writes real images. So every one of the seventeen run tests writes DOCX or EPUB files into a temporary directory — including the ones whose subject has nothing to do with archives, such as "a run with no selected documents ends with exactly one terminal observation" or "a document's warnings are observed before its error". One test stages a broken directory link on disk, which needs symlink privileges on Windows; another provokes a document failure by putting a plain file where a GIF destination directory is expected.

The facts a run consumes cannot be built any other way. Document extraction facts, warnings and errors have only private constructors, so the outcome accumulator that classifies the Extraction run outcome — cover-only versus images, applicable conversion and GIF-routing facts, failed-document counts — is exercised only through archives on disk and has no tests of its own. The same pressure leaks into Extraction run presentation: one presentation test deletes a directory from inside its observer callback to induce a traversal failure, and another writes a DOCX only to obtain one Document extraction warning to render. A maintainer changing the run's ordering contract has to reason about ZIP fixtures, conversion, and file emission to read a test whose assertion is about the order of six observations.

## Solution

The Extraction run gets one new seam for Document extraction, owned by the run, with two adapters: the real Document extraction in production and a scripted one in tests. Document selection stays real, but the run now passes it whichever Document search surface and EPUB declaration source it is given, so run tests use the in-memory adapters Document selection's own tests already use. Tests fabricate Document extraction outcomes through test-only entry points that go through the same conversions production uses, so the invariant guard on Document extraction facts still checks them.

The run's public entry is unchanged and still composes the production adapters, so nothing calling it — the command-line entry point, intake tests, presentation tests — changes. Sixteen run tests leave the filesystem with their assertions untouched; one stays on disk as the in-place check of the production composition. Two presentation tests stop running the whole stack and feed presentation its observations directly.

Users of the command-line tool see no difference: every observation, outcome, file, name, warning, exit status and line of output stays exactly as it is.

## User Stories

1. As a maintainer of the Extraction run, I want to test the run's ordering contract without writing archives to disk, so that a test about observation order reads as a test about observation order.
2. As a maintainer of the Extraction run, I want the run to accept the Document extraction it uses, so that a test can substitute scripted outcomes for real extraction.
3. As a maintainer of the Extraction run, I want the run to accept the Document search surface and EPUB declaration source it hands to Document selection, so that run tests can describe inputs in memory.
4. As a maintainer of the Extraction run, I want Document selection to stay real in run tests, so that the interleaving of selection diagnostics with extraction observations is exercised by both modules working together rather than re-asserted from a script.
5. As a maintainer of the Extraction run, I want the seam to cover exactly what the run reads from Document extraction — whether EPUB cover extraction is configured, the Applicable outcome facts, and per-document extraction — so that the seam's interface is the run's actual dependency and nothing more.
6. As a maintainer of the Extraction run, I want cover intent and Applicable outcome facts still to come from Document extraction, so that the Image write policy remains their single owner as ADR-0006 requires.
7. As a maintainer of the Extraction run, I want the run's existing entry to keep its shape and compose the production adapters itself, so that the command-line entry point, intake tests and presentation tests do not change.
8. As a maintainer of the Extraction run, I want the run's sequencing logic to live in one inner function that both the production entry and the tests call, so that tests exercise the same code production runs.
9. As a maintainer of the Extraction run, I want the seam's trait owned by the run's module, so that Document extraction's imports never name run vocabulary and dependency edges keep running one way.
10. As a maintainer of the Extraction run, I want the seam to take the extractor mutably with static dispatch, matching how the run already takes its observer, so that a test adapter can hand out non-cloneable outcomes without interior mutability.
11. As a maintainer of Document extraction, I want production extraction to satisfy the seam without any change to its own interface or behaviour, so that the seam is an addition rather than a rewrite.
12. As a maintainer of Document extraction, I want test-only entry points that build Document extraction facts, warnings and errors through the same private conversions production uses, so that fabricated values cannot bypass the facts' partition guard.
13. As a maintainer of Document extraction, I want the partition guard on Document extraction facts to check fabricated test facts too, so that a test cannot construct an outcome production could never produce.
14. As a maintainer of Document extraction, I want production visibility of every conversion and constructor to stay exactly as it is, so that the seam widens nothing outside test builds.
15. As a test author, I want a scripted Document extraction that delegates cover intent and Applicable outcome facts to a real Document extraction built from real policies, so that a test needing conversion to apply builds a policy with conversion, just as production does.
16. As a test author, I want to script one Document extraction outcome per document path, so that a multi-document test says plainly which document completes, fails, or warns.
17. As a test author, I want the scripted extraction to panic when a path is extracted twice or was never scripted, so that the request's "consumed exactly once" is enforced rather than merely documented.
18. As a test author, I want to fabricate Document extraction facts from Image write results built with the existing result constructor and count literals, so that I use the same fabrication style EPUB cover extraction's canned attempts already use.
19. As a test author, I want to fabricate a Document extraction warning from an Image write warning, so that the warning's wording stays owned by Document extraction and is never restated in a test.
20. As a test author, I want to fabricate a Document extraction error from an underlying cause, so that failure paths become one scripted line instead of a staged filesystem conflict.
21. As a test author, I want run tests to describe missing inputs, discovery failures, broken links and EPUB declarations with the in-memory Document search surface and declaration source, so that no run test needs symlink privileges or deletes files mid-run.
22. As a test author, I want the sixteen moved run tests to keep their assertions exactly, so that their staying green is the evidence the refactor preserved behaviour.
23. As a test author, I want one run test to stay on disk — the one asserting that an EPUB's declared identity is consistent across normal and cover runs — so that the production composition of real selection and real extraction is still checked in place.
24. As a test author, I want presentation's discovery-failure suspension test to feed presentation its observations directly, so that the last observer doubling as a filesystem hook is retired, as ADR-0008 retired it from Document selection.
25. As a test author, I want presentation's warning test to feed a fabricated Document extraction warning directly, so that rendering a warning no longer requires extracting a real DOCX.
26. As a test author, I want the command-line entry point's three run tests to stay end-to-end, so that the full composition from arguments to exit status remains covered.
27. As a test author, I want new tests for outcome-classification branches that nothing reaches today to arrive in a separate follow-up, through the run's inner function, so that new coverage never blurs the evidence that moved tests kept their assertions.
28. As a maintainer adding the seam, I want the commit that introduces it to leave every test file unedited and every test green, so that a failure there is the seam and not a moved test.
29. As a user of the command-line tool, I want every observation, outcome, extracted file, output name, warning, exit status and summary line to stay the same, so that this change is invisible to me.
30. As a future architecture reviewer, I want the decision to keep Document selection real, and the decision to route fabricated facts through the private conversions, recorded with their reasons, so that I do not propose scripting selection or adding direct test constructors without knowing why they were declined.

## Implementation Decisions

- **Behaviour-preserving refactor.** The observation stream, the Extraction run outcome, files written, output names, warnings, exit status and command-line output are byte-identical. The command-line integration suite does not change. A moved test whose assertion has to change is evidence of a behaviour change, not a test to fix.
- **One new seam, owned by the run.** The Extraction run gains a crate-private trait describing Document extraction as the run uses it: whether EPUB cover extraction is configured, the Applicable outcome facts, and extracting one Selected document into a Document extraction outcome. The trait lives in the Extraction run's module. Its production implementation, for the existing Document extraction type, sits beside it and forwards to the methods that type already has.
- **Receiver and dispatch.** Extraction takes the extractor mutably and the run takes it through static dispatch, matching how the run already takes its observer. Production Document extraction keeps its existing shared-reference method; the trait implementation forwards to it.
- **Document selection stays real.** No seam is added in front of Document selection. The run's inner function receives a Document search surface and an EPUB declaration source and passes them through to Document selection unchanged, as the two separate parameters ADR-0008 chose over a bundle. Selected document gains no test constructor: the run reads only a selected document's path and display name.
- **Entry shape.** The run's existing entry keeps its signature and is the production composition: it unpacks the Extraction run request and hands the filesystem search surface, the file-backed EPUB declaration source and the request's real Document extraction to an inner run-private function holding all sequencing logic. Nothing outside the run's module calls the inner function except the run's own tests. The command-line entry point, intake and presentation are not edited.
- **Cover intent and Applicable outcome facts stay with Document extraction.** They reach the run through the seam, not as values copied into the request or the run, so the Image write policy remains their single owner (ADR-0006).
- **Test fabrication goes through the private conversions.** Document extraction gains test-only, crate-visible entry points that delegate to its existing private conversions — Image write result to Document extraction facts, Image write warning to Document extraction warning, and an underlying cause to Document extraction error. Direct test constructors are not added, because they would let fabricated facts bypass the partition guard ADR-0007 kept for exactly that purpose. Production visibility is unchanged.
- **Scripted test adapter.**
  - It lives beside the run's tests, its only users, rather than in shared test support.
  - It wraps a real Document extraction built from real policies and delegates cover intent and Applicable outcome facts to it.
  - It scripts extraction by document path: each path maps to one Document extraction outcome, taken out on use.
  - Extracting a path twice panics; extracting an unscripted path panics.
- **Untouched.** Document selection's interface and behaviour, Document extraction's production interface and behaviour, the Extraction run request, the observation vocabulary and outcome accumulator (ADR-0004, ADR-0006), the library's four exported items (ADR-0003), and the outcome-to-exit-status mapping (ADR-0014).
- **Visibility.** Every new item is crate-private or narrower; the test entry points exist only in test builds (ADR-0003).
- **No glossary change.** The seam carries the existing Document extraction role; the scripted adapter is test vocabulary.

## Testing Decisions

- **What a good test is here.** It builds a run from in-memory Document selection inputs and scripted Document extraction outcomes, calls the run's inner function with a recording observer, and asserts on the returned Extraction run outcome and the ordered observations. It does not reach past the run into the accumulator or into Document selection's internals, and it never restates wording owned by Document extraction or presentation.
- **Primary test seam.** The run's inner function, with three adapters plugged in: the in-memory Document search surface, the in-memory EPUB declaration source, and the scripted Document extraction. Two of the three already exist. No other new seam is introduced.
- **Run tests that move (sixteen).** Every ordering, classification, failure and warning-transport run test: the no-documents short-circuit, all-failed requested inputs, nested and recursive discovery failures and selection diagnostics preceding extraction, documents without images, an EPUB without a cover, the cover-only run skipping a requested DOCX, produced images, cover fallback classified as images, zero conversion totals, routed GIFs, combined conversion and routing facts, a failed document counted in no-output, partial facts and continuation after a failure, and opaque warning transport. Setup changes; assertions do not. Where an assertion names a real path, the in-memory path takes its place without changing what is asserted.
- **Run test that stays on disk (one).** The test asserting that an EPUB's declared identity is consistent across normal and cover runs. Its subject is the production composition itself.
- **Presentation tests rewritten (two).**
  - The discovery-failure suspension test feeds a recursive discovery start and a discovery failure directly. Its suspension assertion is unchanged.
  - The warning-presentation test feeds a warning fabricated through the test entry point. Its prefix and suspension assertions are unchanged.
- **Kept end-to-end.** The command-line entry point's three run tests and the whole `tests/` integration suite, unedited.
- **Follow-up coverage.** After the move, add run tests through the inner function for outcome-classification branches no test reaches today, such as a produced Covers outcome and the mixed-purpose merge. They go in their own commit.
- **Prior art.**
  - Document selection's tests use the in-memory Document search surface and EPUB declaration source.
  - EPUB cover extraction's canned attempts fabricate Image write results from count literals.
  - Presentation's sibling tests feed observations directly.
  - The recording run observer and the single-terminal-observation assertion already exist in the run's tests.
- **Suggested commit order.** Each commit builds and passes formatting, lint and the full test suite on its own:
  1. Add the seam, its production implementation and the inner function, with no test file edited.
  2. Add the test entry points and the scripted adapter, and move the sixteen run tests.
  3. Rewrite the two presentation tests.
  4. Add the follow-up classification tests.

## Out of Scope

- The Document extraction tests' helpers that run Document selection to obtain a Selected document, and the EPUB adapter's dependence on Document selection in its tests (architecture review candidate 04).
- The intake tests that run a DOCX to observe an intake decision, and making the Extraction run request a readable value (architecture review candidate 06).
- The EPUB fixture builder chain in test support.
- The manual temporary-directory cleanups that predate the temporary-path guard, except where a moved test sheds its temporary directory anyway.
- Direct unit tests of the outcome accumulator's own interface.
- Any seam in front of Document selection itself.
- Any change to observable command-line output.

## Further Notes

- ADR-0016 records this decision and is written ahead of the code, in the manner of ADR-0006, ADR-0007 and ADR-0008. It is not yet committed at the time of writing.
- ADR number 0015 is deliberately vacant. An earlier ADR-0015, written against an older `main`, was withdrawn pending a fresh review of whether its change is still needed. Its glossary edits were reverted with it. Nothing in this spec depends on it.
- Candidate 02 of the same review, which has the Image write pipeline report what it emitted for, is independent of this change. Its new tests would benefit from the scripted adapter introduced here.

## Comments

- Every ticket landed: 01 in PR #64, 02 in PR #65, 03 in PR #66, 04 in PR #67, 05 in PR #68.
