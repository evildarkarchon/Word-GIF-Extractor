# 01 — Pin what Document extraction reports from its policies

Status: done

Blocked by: None — can start immediately

Spec: `.scratch/narrow-run-document-extraction-seam/spec.md` (ADR-0018)

## What to build

Add a direct test, in Document extraction's own tests, of the two facts `DocumentExtraction` reports
from the policies it is built with: whether EPUB cover extraction is configured, and the Applicable
outcome facts (whether conversion applies, and the GIF destination).

Today these are checked only through the Extraction run tests' scripted adapter. That adapter wraps a
real Document extraction and forwards both facts, and one run test asserts the forwarding. Ticket 02
deletes that wrapper and that test. This ticket lands first, so the report keeps a test where it
lives once they are gone.

The new test builds `DocumentExtraction` from real policies in two configurations:

- **A cover-only cover policy, with conversion to JPG and a GIF destination.** It reports cover
  intent, conversion applicable, and that destination.
- **No cover policy, with the Image write policy a run with no image flags gets.** It reports no
  cover intent, no conversion, and no destination.

These are the assertions of
`scripted_document_extraction_delegates_policy_facts_to_real_document_extraction`, carried down one
level. Nothing is written to disk.

The test is additive. No production code changes, and no existing test is edited. Ticket 02 deletes
the forwarding test it replaces.

## Acceptance criteria

- [ ] One new test in `src/document_extraction/tests.rs` asserts both report methods in both
      configurations above, including the GIF destination value
- [ ] The test builds its policies with Document extraction's existing test helpers where they
      exist, and touches no files on disk
- [ ] The test has a doc comment saying what it pins and that it replaces the run-level forwarding
      check
- [ ] No production code and no existing test is changed
- [ ] `cargo fmt --check`, `cargo clippy` and `cargo test` all pass

## Comments

- Landed in 8614988 on branch `t3code/e4795e52` (not yet merged).
