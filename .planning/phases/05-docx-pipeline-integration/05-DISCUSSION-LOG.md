# Phase 5: DOCX Pipeline Integration - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md -- this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 05-docx-pipeline-integration
**Areas discussed:** Conversion summary stats, Lossless WebP gap, Parameter threading

---

## Conversion Summary Stats

| Option | Description | Selected |
|--------|-------------|----------|
| Detailed breakdown | Show converted/skipped counts: "Extracted 8 image(s), converted 6, skipped 2 from 3 document(s)". Adds converted/skipped fields to ExtractionCounts. | ✓ |
| Simple total only | Keep existing message format. No distinction between converted and skipped. | |
| You decide | Claude picks based on existing reporting patterns. | |

**User's choice:** Detailed breakdown
**Notes:** None

| Option | Description | Selected |
|--------|-------------|----------|
| Only with --convert | Conversion stats only appear when --convert flag is active. Without --convert, existing format unchanged. | ✓ |
| Always show | Always include conversion stats, showing "converted 0, skipped 0" even without conversion. | |

**User's choice:** Only with --convert
**Notes:** None

---

## Lossless WebP Gap

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, add it here | Add encode_webp_lossless() using image crate's built-in WebP encoder. Thread lossless flag through convert_image() and try_convert(). Prevents silent bug. | ✓ |
| Defer to Phase 6 | Fix it when EPUB integration happens. Risk: --lossless silently produces lossy output. | |
| You decide | Claude determines timing based on scope and risk. | |

**User's choice:** Yes, add it here
**Notes:** None

---

## Parameter Threading

| Option | Description | Selected |
|--------|-------------|----------|
| Individual params | Add convert, quality, lossless as separate parameters. Consistent with existing pattern. 7 params borderline but acceptable. | ✓ |
| Config struct | Group extraction options into a struct. Cleaner signatures but adds a new abstraction. Would need to refactor existing callers. | |
| You decide | Claude picks based on codebase patterns and complexity tradeoff. | |

**User's choice:** Individual params
**Notes:** None

---

## Claude's Discretion

- Internal logic flow in docx::process_file() for conversion vs. route vs. skip ordering
- Test strategy for integration
- Refactoring approach for per-image extraction loop

## Deferred Ideas

None -- discussion stayed within phase scope
