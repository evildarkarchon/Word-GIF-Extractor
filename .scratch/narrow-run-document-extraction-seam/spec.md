# Narrow the Extraction run's Document extraction seam to extract

Status: done

Governing decision: ADR-0018 (Narrow the Extraction run's Document extraction seam to extract), which supersedes parts of ADR-0016 and leaves ADR-0006 standing. Origin: candidate 1 of the 2026-10-05 architecture review, run with ADR-0001 to ADR-0016 open for re-evaluation and ADR-0017 binding, and settled in a grilling session before any code was written. Glossary terms used here are defined in `CONTEXT.md`: Extraction run, Extraction run request, Extraction run outcome, Extraction run observation, Applicable outcome facts, Document selection, Selected document, Document extraction, Document extraction outcome, EPUB cover policy, Image write policy.

## Problem Statement

The Extraction run reaches Document extraction through a run-private trait with three methods, but it uses only one of them per document. The other two report cover intent and the Applicable outcome facts. The run reads each exactly once, at the top of its inner function, and both are fixed before the first document is selected.

- **The production implementation** forwards all three methods to inherent methods of the same names. It has to name `DocumentExtraction` explicitly at every call, with a comment explaining why: otherwise the forwarding recurses into itself.
- **The scripted adapter** in the run's tests has to wrap a real `DocumentExtraction`, built from a real Image write policy and, where conversion matters, a real conversion policy, just to forward the two facts back. Eleven of its fourteen users build an Image write policy only to forward its defaults. One test exists only to check that forwarding.
- **The run repackages its own request.** `run` unpacks the request's four selection fields, rebuilds them into a second struct, and the inner function unpacks that again into Document selection's options.

A maintainer reading a run test therefore meets conversion targets, image format sets and policy constructors in tests whose subject is sequencing. The seam's interface is three times the size of what crosses it per document.

## Solution

The seam narrows to per-document extraction, a closure from a Selected document to its Document extraction outcome.

- **`run`** asks the request's own Document extraction for cover intent and the Applicable outcome facts once, and hands both to the inner function as plain values.
- **The trait** and its forwarding implementation are deleted.
- **The selection inputs** become one field of the Extraction run request, which `run` moves through whole.
- **The scripted adapter** keeps only its scripted outcomes. Run tests seed cover intent as a boolean and the Applicable outcome facts through their existing test-only constructor.
- **What Document extraction reports** from real policies gets a direct test in Document extraction's own tests. Until now it was checked only through the scripted adapter's wrapper.

Users of the command-line tool see no difference.

## User Stories

1. As a maintainer of the Extraction run, I want the run's Document extraction seam to carry only per-document extraction, so that its interface matches what crosses it per document.
2. As a maintainer of the Extraction run, I want `run` to read cover intent and the Applicable outcome facts once from the request's own Document extraction, so that the Image write policy stays their single owner and the request still retains nothing derived from its policies.
3. As a maintainer of the Extraction run, I want the inner function to take cover intent and the Applicable outcome facts as separate parameters, so that classification input and fact-group applicability stay distinct values, as ADR-0006 requires.
4. As a maintainer of the Extraction run, I want the seam to be a closure rather than a one-method trait, so that the forwarding implementation and its recursion workaround disappear.
5. As a maintainer of the Extraction run, I want the "every selected document is extracted exactly once" contract, and the reason the closure is `FnMut`, stated on the inner function's doc comment, so that the seam's contract survives the trait's deletion.
6. As a maintainer of the Extraction run, I want the request's selection inputs held as one value that `run` passes through, so that the four selection fields are spelled once in their struct and once where Document selection's options are built.
7. As a maintainer of Document extraction, I want a direct test of the cover intent and Applicable outcome facts it reports from real policies, including the GIF destination, so that the report has a test where it lives.
8. As a writer of run tests, I want to script a run with a cover-intent boolean and fabricated Applicable outcome facts, so that no run test builds an Image write policy or a conversion policy.
9. As a writer of run tests, I want the scripted adapter's path-keyed outcomes and its two panics (a path extracted twice, a path never scripted) kept, so that "consumed exactly once" stays enforced by a test.
10. As a maintainer reviewing the change, I want every moved run test's assertions unchanged, so that the change reads as behaviour-preserving.
11. As a maintainer reviewing the change, I want the integration suite untouched, so that command-line behaviour is checked by tests the change did not edit.
12. As a future architecture reviewer, I want ADR-0018 to state that ADR-0006's ownership rule stands and only the transport changed, so that a later review neither reverts this nor reads it as permission to cache facts in the request.

## Implementation Decisions

- **The inner function's parameters** are:
  - the selection inputs;
  - cover intent, as a boolean;
  - the Applicable outcome facts;
  - the Document search surface;
  - the EPUB declaration source;
  - the extract closure;
  - the observer.
  
  The selection seams remain two separate `&dyn` parameters, as ADR-0008 chose.
- **The closure parameter** is `&mut impl FnMut(SelectedDocument) -> DocumentExtractionOutcome`, statically dispatched like the observer. Production passes a closure over `DocumentExtraction::extract`.
- **`run`** remains the production composition, and its signature is unchanged. It takes the request apart into its selection inputs and its Document extraction, reads both facts from that Document extraction, and calls the inner function with the filesystem adapters and the closure.
- **The order of reads in `run` is a deliberate choice.** The two facts are read before the inner function is called, and the inner function reads its parameters before selecting anything. So the facts are fixed at the moment the run begins, exactly as today.
- **The selection-inputs struct** becomes a field of the Extraction run request. The request's constructor keeps its parameter list, and Extraction run intake does not change. The struct's doc comment keeps saying cover intent is absent on purpose, with the reason updated: `run` reads cover intent from Document extraction.
- **Deleted from production:** the run-private trait, its production implementation, and the comment about forwarding recursion. The doc comment on the trait is not lost: its contract moves to the inner function's doc comment.
- **Unchanged:** `DocumentExtraction`'s two report methods, its constructor, and their doc comments; the Extraction run request's public-to-crate shape apart from the nested field; the outcome accumulator.
- **Unchanged:** Extraction run intake, Extraction run presentation, Document selection, the Image write pipeline, and the command-line entry point.

## Testing Decisions

- **A good test exercises behaviour through the interface of the module that owns it.** What Document extraction reports is tested on Document extraction. What the run does with those values is tested through the run's inner function.
- **New test in Document extraction's tests.** It builds `DocumentExtraction` from real policies and asserts both report methods in two configurations:
  - a cover-only policy with conversion and a GIF destination, which reports cover intent, conversion applicable, and the destination;
  - no cover policy and default image policy, which reports no cover intent, no conversion, and no destination.
  
  This carries the deleted forwarding test's assertions down one level. It needs no files on disk.
- **Deleted:** `scripted_document_extraction_delegates_policy_facts_to_real_document_extraction`. Its subject, the forwarding, is gone. Its assertions survive in the test above.
- **Scripted adapter:**
  - It loses its wrapped real Document extraction and its three policy-binding constructors.
  - It keeps its path-keyed outcomes, its `with_outcome` builder, its inherent `extract`, and its two panic tests.
  - The run-scripting helper gains cover-intent and Applicable outcome facts parameters, and wraps the adapter in a closure for the inner function.
  - The run tests' Image write policy and conversion policy builders, and the conversion, image format and Image write policy imports that only they used, are deleted.
- **Run tests and their setup:**

  | Run test | Setup after this change |
  |---|---|
  | `scripted_document_extraction_panics_when_a_path_is_extracted_twice` | Setup rewritten |
  | `scripted_document_extraction_panics_on_an_unscripted_path` | Setup rewritten |
  | `no_selected_documents_returns_no_documents_outcome` | No cover intent, default facts |
  | `all_failed_requested_inputs_reach_one_no_documents_terminal_observation` | No cover intent, default facts |
  | `nested_discovery_failure_precedes_later_progress_and_extraction_in_run_stream` | No cover intent, default facts |
  | `recursive_discovery_failure_precedes_later_progress_and_extraction` | No cover intent, default facts |
  | `selection_diagnostic_and_completion_precede_extraction_in_one_observation_stream` | No cover intent, default facts |
  | `selected_epub_without_a_cover_returns_cover_no_output` | Cover intent set, default facts (ADR-0017 keeps it: cover intent reaches classification) |
  | `cover_only_run_skips_requested_docx_and_diagnoses_it` | Cover intent set, default facts |
  | `requested_conversion_retains_valid_zero_totals` | Fabricated facts with conversion applicable and no destination (ADR-0017 keeps it: the Applicable outcome facts seed the fold) |
  | `epub_identity_is_consistent_across_normal_and_cover_runs` | Unaffected: it runs the real composition through `run` |
  | `run_retains_partial_facts_and_continues_after_document_failure` | Fabricated facts with no conversion and the `blocked-gifs` destination it uses today (ADR-0017 keeps it) |
  | `run_carries_opaque_document_extraction_warnings_with_originating_paths` | No cover intent, default facts |
  | `every_failed_document_is_counted_in_the_outcome` | No cover intent, default facts (ADR-0017 keeps it) |

  "Default facts" means no conversion and no GIF destination, which is what the default Image write policy reports today. No assertion changes in any row.
- **The behaviour bar.** The integration suite is not edited. Any test whose asserted value changes is evidence of a behaviour change, not a test to fix.
- **Prior art:**
  - ADR-0016's move of sixteen run tests onto the seams: setup changed, assertions unchanged.
  - ADR-0017's move of classification tests to the accumulator, which seeds with `ApplicableOutcomeFacts::fabricated` exactly as the run tests now will.

## Out of Scope

- The Image write pipeline's forwarders for conversion and GIF destination, and whether the Image write policy's own accessors can become private to the pipeline.
- Having `DocumentExtraction::new` take an Image write policy, to remove the pipeline wrapping repeated at its call sites.
- The GIF destination has no end-to-end check through a real run. No intake, presentation, `run_cli` or integration test runs with a GIF output directory and asserts the routing facts or the summary that names them. This predates the change and is filed as `issues/03`.
- The accumulator taking cover intent at construction or folding a whole Document extraction outcome (candidate 2 of the same review).
- Any change to observations, outcomes, wording, exit status or the integration suite.

## Further Notes

- **Comment and docstring policy.** Three comments describe code that is deleted, so each is removed or rewritten, and each removal is called out in the change description:
  - the trait's doc comment, whose contract moves to the inner function;
  - the forwarding implementation's recursion comment;
  - the scripted adapter's statement that cover intent and Applicable outcome facts are delegated to a real Document extraction.
  
  The selection-inputs struct's "cover intent is absent on purpose" comment is rewritten only where its reason changes. The inner function's doc comment gains the contract and the `FnMut` reasoning.
- **ADR notes land with the code, not before.** When the code lands, ADR-0016's first, third and fifth paragraphs get status notes pointing to ADR-0018. ADR-0006 gets none: its decisions stand.
- **ADR-0003 is unchanged.** Nothing becomes public.

## Comments

- Ticket 01 landed in 8614988; ticket 02 landed in 2749885 (not yet merged). Ticket 03 remains open for triage.
