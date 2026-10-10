# 02 — Narrow the run's Document extraction seam to an extract closure

Status: done

Blocked by: 01

Spec: `.scratch/narrow-run-document-extraction-seam/spec.md` (ADR-0018)

## What to build

The Extraction run reaches Document extraction through a closure from a Selected document to its
Document extraction outcome. The run-private three-method trait and its forwarding implementation
are deleted.

**`run`** remains the production composition, and its signature is unchanged. It:

1. takes the request apart into its selection inputs and its Document extraction;
2. reads cover intent and the Applicable outcome facts from that Document extraction once;
3. calls the inner function with those two values, the filesystem search surface, the EPUB file
   declarations, a closure over `DocumentExtraction::extract`, and the observer.

**The inner function** takes, in order:

- the selection inputs;
- cover intent;
- the Applicable outcome facts;
- the Document search surface;
- the EPUB declaration source;
- `&mut impl FnMut(SelectedDocument) -> DocumentExtractionOutcome`;
- the observer.

It uses cover intent exactly where it used the trait's answer before: as Document selection's EPUB-only flag, in the extraction-started observation, and in the accumulator's finish. It seeds the accumulator with the Applicable outcome facts.

**The selection-inputs struct** becomes a field of the Extraction run request, and `run` moves it
through whole. The request's constructor keeps its parameter list, so Extraction run intake does not
change.

**The run's tests:**

- The scripted adapter drops its wrapped real Document extraction and its policy-binding
  constructors.
- It keeps its path-keyed outcomes, `with_outcome`, an inherent `extract`, and its two panic tests.
- The run-scripting helper gains cover-intent and Applicable outcome facts parameters, and wraps the
  adapter in a closure.
- The tests seed facts through `ApplicableOutcomeFacts::fabricated`. The spec's table gives each
  test's values.
- `scripted_document_extraction_delegates_policy_facts_to_real_document_extraction` is deleted.
  Ticket 01 carried its assertions down to Document extraction.
- The run tests' Image write policy and conversion policy builders, and the imports only they used,
  are deleted.

The change is strictly behaviour-preserving.

## Acceptance criteria

- [ ] The run-private Document extraction trait and its production implementation no longer exist
- [ ] The inner function takes the seven parameters above. The extract parameter is
      `&mut impl FnMut(SelectedDocument) -> DocumentExtractionOutcome`
- [ ] The inner function's doc comment states:
      - that every selected document is passed to the closure exactly once;
      - why the closure is `FnMut` (outcomes are not `Clone`, so a scripted adapter hands each out
        by value);
      - that cover intent and the Applicable outcome facts arrive as values read from Document
        extraction by `run`
- [ ] `run` reads both facts from the request's Document extraction once, before calling the inner
      function. `run`'s signature and doc contract are unchanged
- [ ] The selection-inputs struct is a field of the Extraction run request and is not rebuilt in
      `run`. Its "cover intent is absent on purpose" comment states the new reason. The request's
      constructor signature is unchanged
- [ ] `DocumentExtraction`, Extraction run intake, presentation, Document selection, the accumulator
      and the Image write pipeline have no production changes
- [ ] The scripted adapter holds no Document extraction and no policies. Its two panic tests pass
      unchanged in assertion
- [ ] The forwarding test is deleted. No other run test's assertions change; only setup changes, as
      the spec's table lists
- [ ] The run tests no longer import the conversion, image format or Image write policy types
- [ ] Every comment removed or rewritten because its code was deleted is called out in the commit
      message: the trait's doc comment, the recursion comment, and the scripted adapter's delegation
      note
- [ ] ADR-0016's first, third and fifth paragraphs each get a "Superseded in part by ADR-0018" note.
      ADR-0006 gets none
- [ ] The spec and this ticket are set to `done`, with the landing commit recorded under
      `## Comments`
- [ ] Nothing under `tests/` is edited. Every new or changed item stays crate-private or narrower
      (ADR-0003)
- [ ] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass

## Comments

- Landed in 2749885 on branch `t3code/narrow-extract-closure-seam` (not yet merged).
- `ImageFormat` stays imported by the run tests: the `extension_fallback` warning helper needs `ImageFormat::Png`. The spec scopes the deletion to imports "that only they used", so the acceptance line above reads stricter than intended.
