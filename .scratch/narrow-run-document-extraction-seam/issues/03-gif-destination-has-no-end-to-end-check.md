# 03 — The GIF destination has no end-to-end check

Status: needs-triage

Blocked by: None

Spec: `.scratch/narrow-run-document-extraction-seam/spec.md` (ADR-0018, out of scope there)

## What was found

While grilling ADR-0018, a search for tests outside the Extraction run's own tests found what would
catch `run` handing the fold a wrong fact. Each fact is covered differently:

- **Cover intent.** Caught by `builds_validated_epub_cover_extraction_policy` in Extraction run
  intake's tests, and by the on-disk `epub_identity_is_consistent_across_normal_and_cover_runs`.
- **Conversion applicable.** Caught by `builds_default_conversion_policy` in intake's tests, but only
  in one direction. A run that wrongly reported conversion as applicable when none was requested
  would go unnoticed outside the run tests.
- **GIF destination.** Caught by nothing. No intake, presentation, `run_cli` or integration test runs
  with a GIF output directory and asserts the outcome's routing facts or the summary line naming
  routed GIFs and their destination. Files would still land in the GIF directory, because the Image
  write pipeline routes them independently of the reported fact, so even a file-existence check would
  pass.

This predates ADR-0018. After ticket 01, Document extraction's report of the destination is pinned
directly, but nothing pins that a real run with `--gif-output` reports routing in its outcome or
terminal summary.

## Questions for triage

- Should the check live in intake's tests (assert the outcome's GIF routing facts after a real run
  of a DOCX containing a GIF), in the integration suite (assert the summary line), or both?
- Should the conversion direction gap (reported as applicable when not requested) be closed in the
  same change?
