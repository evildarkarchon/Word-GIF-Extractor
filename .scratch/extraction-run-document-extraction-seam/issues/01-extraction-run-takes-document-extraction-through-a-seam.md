# 01 — Extraction run takes its Document extraction through a seam

Status: ready-for-agent

Blocked by: None — can start immediately

Spec: `.scratch/extraction-run-document-extraction-seam/spec.md` (ADR-0016)

## What to build

The Extraction run stops hard-wiring its collaborators. A crate-private trait owned by the run's
module describes Document extraction as the run uses it: whether EPUB cover extraction is
configured, the Applicable outcome facts, and extracting one Selected document into a Document
extraction outcome. Extraction takes the extractor mutably; the run takes it through static
dispatch, the same way it already takes its observer. The existing Document extraction type
implements the trait beside it by forwarding to the methods it already has.

All sequencing logic moves into one run-private inner function that receives a Document search
surface, an EPUB declaration source (as two separate parameters, per ADR-0008) and the extractor,
and passes the first two to Document selection unchanged. The run's public entry keeps its
signature and becomes the production composition: it unpacks the Extraction run request and hands
the filesystem search surface, the file-backed EPUB declaration source and the request's real
Document extraction to the inner function.

This is a behaviour-preserving prefactor. Users of the command-line tool see no difference.

## Acceptance criteria

- [ ] The seam trait lives in the Extraction run's module; Document extraction's imports never name run vocabulary
- [ ] The trait covers exactly cover intent, Applicable outcome facts and per-document extraction — nothing more
- [ ] Cover intent and Applicable outcome facts still come from Document extraction (ADR-0006); nothing is copied into the request or the run
- [ ] Production Document extraction's own interface, receivers and behaviour are unchanged
- [ ] The run's public entry keeps its signature; the command-line entry point, intake and presentation are not edited
- [ ] Nothing outside the run's module calls the inner function
- [ ] Every new item is crate-private or narrower; the library still exports exactly four items (ADR-0003)
- [ ] No test file is edited, and `cargo fmt --check`, `cargo clippy` and `cargo test` all pass
