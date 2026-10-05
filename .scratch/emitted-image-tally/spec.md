# Record each emitted image once into an Emitted image tally

Status: done

Governing decision: ADR-0017 (Record each emitted image once into an Emitted image tally), which supersedes parts of ADR-0004, ADR-0006, ADR-0007 and ADR-0016. Origin: candidate 1 of the 2026-10-05 architecture review, run with every ADR open for re-evaluation and settled in a grilling session before any code was written. Glossary terms used here are defined in `CONTEXT.md`: Extraction run, Extraction run outcome, Extraction run presentation, Document extraction, Document extraction facts, Document extraction warning, Applicable outcome facts, EPUB cover extraction, Image write pipeline, Image write purpose, Emitted image role, Emitted image tally. The CONTEXT.md entries for the Emitted image tally and the revised Document extraction facts already landed with ADR-0017, ahead of the code, as a deliberate departure from ADR-0008's convention. Until this work lands, ADR-0017 is the reference for which of the two is current.

## Problem Statement

A maintainer who wants to know whether an Extraction run produced covers or images, or how many images it converted, skipped or routed, has to read three modules that each spell the same facts differently.

- **Image write pipeline:** keeps four counts and a private flag recording whether any normal image was emitted.
- **Document extraction:** copies the four counts into a second type under new names, and derives a three-way output purpose (covers only, included normal images, nothing emitted) from the emitted count and the flag.
- **Outcome accumulator:** adds the counts up again, merges the purposes, and then reads the merged purpose only as "did any document include normal images?". Two of the three states are never told apart.

The rule that keeps these totals consistent is that converted, conversion-skipped and GIF-routed images never together exceed the emitted total. It is guaranteed only per image. Above that, it is a debug assertion at the Document extraction seam that can now fire only on results built by hand, plus a second, validating outcome constructor that only tests call.

The tests carry the cost. To say "this document wrote one cover", a test hand-builds pipeline counts, sets a "normal image output absent" marker, and adds a comment translating that into "a cover". The test then routes it through a test-only entry point whose only purpose is to feed the debug assertion. The outcome accumulator is pure logic, but it has no tests of its own. Every classification test drives the whole Extraction run: an in-memory search surface, in-memory EPUB declarations, a scripted Document extraction bound to real policies, and those hand-built results. That is roughly 340 lines of scaffolding around an 80-line fold. One validation test keeps alive a constructor production never calls. Extraction run presentation's tests build every produced outcome through that same constructor.

## Solution

Each emitted image is recorded exactly once into one Emitted image tally, under its Image write purpose (normal image or cover) and its Emitted image role.

- **Pipeline:** records into the tally at the one place it counts today.
- **Document extraction facts:** a document's tally plus its warnings.
- **Accumulator:** adds the documents' tallies together.

Whether output was covers only, included normal images, or nothing is read off the tally's normal-image and cover totals instead of being carried beside them. The partition rule becomes a property of the value, because the only way to grow a tally is to record images. The debug assertion, the test-only facts entry point and the validating outcome constructor are all deleted rather than moved.

Outcome classification is tested where it lives: tallies and failures go into the accumulator, and an Extraction run outcome comes out. Run tests keep only what the run itself decides. Users of the command-line tool see no difference: every observation, outcome, file, name, warning, exit status and line of output stays exactly as it is.

## User Stories

1. As a maintainer of the Image write pipeline, I want each emitted image recorded exactly once, so that no count can disagree with the images actually written.
2. As a maintainer of the Image write pipeline, I want recording to replace the existing per-role count increments at the single place images are counted, so that no second counting site appears.
3. As a maintainer of the Image write pipeline, I want normal-image output recorded through one operation and required-cover output through another, so that each call site states the Image write purpose it already knows statically.
4. As a maintainer of the Image write pipeline, I want the routed GIF destination to stay bound to the routing decision inside the pipeline, so that ADR-0007's guarantee is untouched while the tally records only the role.
5. As a maintainer of the Image write pipeline, I want a required cover that is a routed GIF to record as a routed cover, so that routed covers count toward the run's routed GIF total exactly as they do today.
6. As a maintainer of the Image write pipeline, I want an image write result to be a tally plus warnings, so that the separate normal-output flag and its marker type disappear.
7. As a maintainer of the Image write pipeline, I want combining results to combine their tallies and concatenate warnings in order, so that EPUB cover extraction's retry and fallback folding keep their behaviour.
8. As a maintainer of the Image write pipeline, I want building a complete result from a tally and warnings to stay crate-visible, so that EPUB cover extraction's scripted attempts can keep working without touching disk.
9. As a maintainer of EPUB cover extraction, I want scripted attempts to record a cover or normal images into a tally, so that the ADR-0005 seam keeps testing ordering, exclusion and retry without hand-building counts.
10. As a maintainer of Document extraction, I want Document extraction facts to carry the document's tally and warnings and nothing derived from them, so that there is no second spelling of the counts and no separate output-purpose classification.
11. As a maintainer of Document extraction, I want the debug assertion on the partition deleted, so that no check remains for a condition the type rules out.
12. As a maintainer of Document extraction, I want the test-only facts entry point that routed hand-built pipeline results through the assertion replaced by a test-only constructor taking a tally and Document extraction warnings, so that tests build facts in domain vocabulary.
13. As a maintainer of Document extraction, I want the Document extraction warning and error test entry points left exactly as they are, so that warning wording and error sealing stay owned in one place.
14. As a maintainer of the Extraction run outcome, I want the accumulator to add documents' tallies together, so that cross-document totals are one addition rather than four re-spelled sums.
15. As a maintainer of the Extraction run outcome, I want a run's output classified as covers exactly when the run sought covers and no normal image was recorded, so that classification reads one total instead of a merged three-way value.
16. As a maintainer of the Extraction run outcome, I want "documents with output" to keep counting documents whose tally emitted at least one image, so that an empty document still does not count.
17. As a maintainer of the Extraction run outcome, I want failure recording to stay separate from folding, so that a failed document's partial tally still folds and its failure is still one extra fact.
18. As a maintainer of the Extraction run outcome, I want the validating outcome constructor deleted, so that there is one definition of a valid produced outcome and it is the one production uses.
19. As a maintainer of the Extraction run outcome, I want the outcome and produced-output types to keep their shape and accessors, so that Extraction run presentation needs no production change.
20. As a maintainer of the Extraction run outcome, I want to seed the accumulator in tests with Applicable outcome facts I choose, so that I can test conversion and GIF-routing applicability without building a Document extraction.
21. As a maintainer of the Extraction run outcome, I want classification tests that feed tallies and failures into the accumulator and assert on the outcome, so that a classification test reads as one.
22. As a maintainer of the Extraction run outcome, I want "intent × failure over zero facts" pinned as one compact case table, so that the four no-output combinations are visibly complete.
23. As a maintainer of the Extraction run outcome, I want "GIF routing applies but nothing was routed" to yield no routing facts, pinned at the accumulator, so that a branch currently pinned only incidentally by a run test has its own test.
24. As a maintainer of the Extraction run, I want run tests to keep only what the run hands the fold, so that run tests stay about sequencing and the run's own decisions.
25. As a maintainer of the Extraction run, I want one run test pinning that a failed document's partial facts are folded, so that the run cannot drop them.
26. As a maintainer of the Extraction run, I want one run test pinning that every failure is recorded and matches the error observations, so that the outcome counts a failure exactly when the terminal shows one.
27. As a maintainer of the Extraction run, I want one run test pinning that the run's cover intent reaches classification, so that a run seeking covers that produced nothing still says it sought covers.
28. As a maintainer of the Extraction run, I want one run test pinning that the Applicable outcome facts seed the fold, so that a conversion run with zero totals still reports conversion facts.
29. As a maintainer of Extraction run presentation, I want its test helper to build produced outcomes through the accumulator, so that presentation tests never hold an outcome production could not produce.
30. As a reader of any test, I want a scripted document described as "one cover written" or "two normal images, one converted", so that no comment has to translate pipeline counts into what they mean.
31. As a maintainer reviewing the change, I want every test either unchanged, mechanically rewritten, moved down with the same assertion, converted to assert on a real document's tally, or deleted because its subject is gone, so that the change can be reviewed as behaviour-preserving.
32. As a maintainer reviewing the change, I want the integration suite untouched, so that command-line behaviour is checked by tests the change did not edit.
33. As a future architecture reviewer, I want ADR-0017 and the glossary to state the tally as the one home of emitted-image accounting, so that a later review does not re-propose a second counter type.
34. As a reader of the superseded ADRs, I want a note on each superseded paragraph pointing to ADR-0017 once the code lands, so that no ADR describes a design the code no longer has.

## Implementation Decisions

- **A new leaf module owns the Emitted image tally and imports nothing.** The Image write pipeline, Document extraction and the Extraction run observation module import it. No edge points out of it, so ADR-0004's one-way dependency rule holds without a duplicate classification type.
- **The tally's interface:**
  - record one normal image with a role;
  - record one cover with a role;
  - combine with another tally by addition;
  - read the normal-image total, the cover total, the emitted total (derived as normal images plus covers, never stored), and the converted, conversion-skipped and GIF-routed totals.
  
  Nothing else can change a tally. An empty tally is the starting value.
- **The tally owns a destination-free four-way role:** routed GIF, converted, conversion-skipped, preserved. The pipeline keeps its own Emitted image role with the borrowed routed destination exactly as ADR-0007 decided, and maps it onto the tally's role in the match that increments counts today. That match is the only place production counts images, so the mapping replaces it.
- **Recording has no runtime purpose value.** The normal-image visitor records normal images and the required-cover path records covers, because each is statically bound to its Image write purpose. Whether the Image write purpose trait should become data is left open.
- **A required cover may record any role but conversion-skipped.** A cover conversion fallback completes without emitting. The tally does not enforce this; it records what the pipeline emits.
- **An image write result is a tally plus warnings.**
  - The four-count type and the normal-image-output marker are deleted.
  - Building a result from a tally and warnings stays crate-visible, and production still calls it once inside the pipeline.
  - Appending one result to another combines the tallies and concatenates warnings in order. Prepending earlier facts onto a failure keeps its meaning.
- **Document extraction facts are a tally plus ordered Document extraction warnings.**
  - Deleted: the emitted-image totals type, the three-way output purpose and its merge rule, the partition debug assertion, and the test-only facts entry point that took a hand-built pipeline result.
  - Added: a test-only constructor taking a tally and Document extraction warnings.
  - The translation from a pipeline result keeps only the warning translation and the hand-over of the tally.
- **The outcome accumulator's seed and fold:**
  - It is seeded from Applicable outcome facts as today.
  - It folds each document's facts by adding the tally and counting the document as having output when its tally emitted at least one image.
  - It records failures separately as today.
  - On finish, it classifies output as covers when the run's cover intent is set and the combined normal-image total is zero, and as images otherwise. This is exactly equivalent to today's rule. The pipeline's unwritten rule that one document never emits both a normal image and a cover stops mattering: such a document would classify as images, which agrees with the merge.
- **Applicable outcome facts gain a test-only constructor** so accumulator tests can choose conversion applicability and a GIF destination directly. The value carries no invariant to bypass.
- **The validating produced-outcome constructor is deleted,** together with its own validation test. The Extraction run outcome and produced-output types keep their variants, fields and accessors.
- **The Extraction run, Document selection, Extraction run presentation's production code and the command-line entry point do not change.**

## Testing Decisions

- **A good test exercises behaviour through the interface of the module that owns it and asserts only what a caller can observe.** For classification, that means tallies and failures go into the accumulator and the outcome comes out. A test must not reach past an interface to check something the interface already decides. Tests compare outcomes through accessors, or against an outcome built the same way, never against a hand-assembled produced outcome.
- **The accumulator is the primary test seam.** It is an existing interface tested directly for the first time. The moved-down tests live beside the outcome types in the observation module's test file. That file is otherwise emptied when the validating constructor's test is deleted.
- **No new seams.** The Extraction run's inner function (ADR-0016), the Image write pipeline's existing entry points, Document extraction's per-document extraction, and EPUB cover extraction's scripted-attempts seam (ADR-0005) stay as they are. The tally gets no test module of its own: recording is exercised through the pipeline and combining through the accumulator.
- **Run-test verdicts.** One rule decides each run test: a test moves down when its assertion is about what the outcome says given some facts, and stays when its assertion is about what the run handed the fold, or about sequencing.

  | Run test | Verdict |
  |---|---|
  | `scripted_document_extraction_delegates_policy_facts_to_real_document_extraction` | Unaffected |
  | `scripted_document_extraction_panics_when_a_path_is_extracted_twice` | Unaffected; setup rewritten |
  | `scripted_document_extraction_panics_on_an_unscripted_path` | Unaffected |
  | `no_selected_documents_returns_no_documents_outcome` | Unaffected |
  | `all_failed_requested_inputs_reach_one_no_documents_terminal_observation` | Unaffected |
  | `nested_discovery_failure_precedes_later_progress_and_extraction_in_run_stream` | Unaffected; setup rewritten |
  | `recursive_discovery_failure_precedes_later_progress_and_extraction` | Unaffected; setup rewritten |
  | `selection_diagnostic_and_completion_precede_extraction_in_one_observation_stream` | Unaffected; setup rewritten |
  | `selected_document_without_images_returns_image_no_output` | Moves down |
  | `selected_epub_without_a_cover_returns_cover_no_output` | **Stays** — the run's cover intent reaches classification. With nothing folded, intent is the only possible source of Covers, so this is the one test that fails if finish ignores intent. |
  | `cover_only_run_skips_requested_docx_and_diagnoses_it` | Unaffected (its subject is selection eligibility); its scripted cover becomes a recorded cover |
  | `normal_document_output_returns_produced_images` | Moves down |
  | `epub_normal_fallback_is_classified_as_images` | Moves down |
  | `requested_conversion_retains_valid_zero_totals` | **Stays** — the Applicable outcome facts seed the fold |
  | `routed_gif_retains_its_count_and_destination` | Moves down |
  | `produced_outcome_retains_combined_conversion_and_gif_routing_facts` | Moves down; its "partition guard at equality" rationale is gone with the guard |
  | `epub_identity_is_consistent_across_normal_and_cover_runs` | Unaffected (real composition) |
  | `failed_document_without_output_is_counted_in_no_output` | Moves down |
  | `run_retains_partial_facts_and_continues_after_document_failure` | **Stays** — a failed document's partial facts are folded; its outcome equality is rewritten through accessors |
  | `run_carries_opaque_document_extraction_warnings_with_originating_paths` | Unaffected; setup rewritten |
  | `cover_only_run_with_a_written_cover_and_an_empty_epub_produces_covers` | Moves down |
  | `cover_run_merging_covers_with_fallback_images_classifies_as_images_in_either_order` | Moves down |
  | `produced_outcome_sums_conversion_and_gif_routing_totals_across_documents` | Moves down |
  | `every_failed_document_is_counted_in_the_outcome` | **Stays** — every failure is recorded and matches the error observations; its outcome equality is rewritten through accessors |
  | `failed_epub_in_cover_only_run_is_counted_in_cover_no_output` | Moves down |

  "Setup rewritten" means the scripted outcome is built from a tally instead of hand-built counts. The assertion does not change.
- **Folding duplicates at the accumulator.**
  - The three moved no-output tests (no intent and no failure, no intent with a failure, intent with a failure) become one case table with the staying run test's case (intent, no failure) included, covering all four intent × failure combinations over zero facts.
  - The single-document combined-facts case and the single-document fallback case may fold into the multi-document tests they are special cases of, if the case each pins is still named.
  - Because tallies combine by addition, the "either order" loop of the merge test needs to cover only one order. A second order may stay as documentation but proves nothing new.
- **One new accumulator test is needed.** GIF routing applies, nothing was routed, and the outcome carries no routing facts. Today only the staying partial-facts run test pins this, incidentally.
- **Document extraction tests.**
  - Deleted, because their subject is gone: the test that fabricated facts pass through the production translation, and the test that fabricated facts trip the partition guard.
  - Converted to assert on the document's tally, because each extracts a real document:
    - failed extraction retains facts;
    - DOCX warning bodies;
    - EPUB cover warning bodies;
    - EPUB cover conversion warning bodies;
    - EPUB cover retry warning bodies;
    - EPUB cover output classified as covers only (now: one cover, no normal images);
    - EPUB cover fallback classified as normal images;
    - normal policy extracts EPUB images;
    - retained EPUB declarations;
    - selection declaration failure is retried.
  - Unaffected: the fabricated error preserves its source chain.
- **Mechanical rewrites.**
  - Image write pipeline tests (about 56 count reads and 10 normal-output reads), EPUB adapter tests, DOCX adapter tests and EPUB cover extraction tests read tally totals instead of count fields.
  - EPUB cover extraction's scripted-attempt helpers record covers or normal images instead of hand-building counts.
  - The pipeline test that builds results by hand to check they carry their facts through the fold builds them from tallies.
  - A rewrite counts as mechanical only when the asserted value stays the same.
- **Extraction run presentation's produced-outcome test helper** builds outcomes through the accumulator. Shapes with more emitted images than documents are split across that many documents' tallies, and covers outcomes record covers and finish with cover intent. Its fifteen callers keep their arguments. Presentation's opaque-warning test entry point is unaffected.
- **The behaviour bar.** The integration suite in the repository's top-level test directory is not edited. Any test whose asserted value changes is evidence of a behaviour change, not a test to fix.
- **Prior art:**
  - EPUB cover extraction's scripted-attempts tests: policy decisions asserted over scripted facts, without the filesystem.
  - The run tests that moved onto ADR-0016's seams: assertions unchanged, setup changed.
  - Document selection's identity tests: a deep value tested through its own small interface.

## Out of Scope

- Narrowing the Extraction run's Document extraction seam to per-document extraction alone, which removes the Applicable outcome facts forwarding (candidate 2 of the same review). It touches the accumulator's seed, not its fold, and lands separately afterwards.
- The Archive resource identity session brand and EPUB cover extraction's generic shape (ADR-0005, candidate 4).
- Turning the Image write purpose trait into data (candidate 5).
- The Document extraction warning and error test entry points.
- Any change to the Extraction run outcome's variants, presentation wording, observation order, exit status, or the integration suite.
- The two defects the review surfaced: a DOCX that is both requested and found by directory search is selected twice, and an EPUB XHTML resource with inline SVG near its start is probably emitted as SVG. Neither is filed yet; each belongs in its own `.scratch/` feature.

## Further Notes

- **Comment and docstring policy.** Comments justifying the deleted types (the partition assertion's explanation, the output purpose's cycle rationale, the validating constructor's "not leftover" note, the accumulator's "why finishing needs no check" section) are rewritten or removed only because the code they describe is deleted or now false. Each such removal is called out in the change description. New items (the tally, its role, its operations, the test constructors) get doc comments. Comments elsewhere that go stale are updated rather than dropped, for example a pipeline test comment naming the output purpose type, and run-test comments that translate counts into "a written cover".
- **Superseded ADR paragraphs get a status note pointing to ADR-0017 when the code lands, not before:**
  - ADR-0004's rationale for the output purpose type;
  - ADR-0006's tripwire placement and validating-constructor paragraphs;
  - ADR-0007's counts paragraph and assertion paragraph;
  - ADR-0016's fabrication-through-the-guard paragraph and classification-test-level decision.
- **ADR-0003 is unchanged.** Every added item stays crate-private, and nothing becomes public.
