# Phase 6: EPUB Pipeline Integration - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 06-epub-pipeline-integration
**Areas discussed:** Config struct refactor, Cover-only + conversion, Parameter threading depth, EPUB-specific edge cases

---

## Config Struct Refactor

### Question 1: How should we handle the parameter growth?

| Option | Description | Selected |
|--------|-------------|----------|
| Extraction config struct | Create ExtractionConfig in common.rs bundling: convert, quality, lossless, gif_output. Shared by both processors. | ✓ |
| Full config struct | Bundle ALL non-path params including cover_only, cover_fallback, filter. Bigger refactor. | |
| Keep individual params | Add convert/quality/lossless as individual params. Accept clippy allow. | |
| You decide | Claude picks during planning. | |

**User's choice:** Extraction config struct (Recommended)
**Notes:** User selected the focused struct that bundles only conversion-related params, not all params.

### Question 2: Should the refactor also update docx::process_file()?

| Option | Description | Selected |
|--------|-------------|----------|
| Both DOCX and EPUB | Refactor both processors to share ExtractionConfig. Clean break. | ✓ |
| EPUB only | Only apply to epub.rs. Smaller diff but inconsistent. | |
| You decide | Claude picks during planning. | |

**User's choice:** Both DOCX and EPUB (Recommended)
**Notes:** User wants consistent interfaces across both processors.

---

## Cover-only + Conversion

### Question 3: When cover conversion fails, what should happen?

| Option | Description | Selected |
|--------|-------------|----------|
| Warn and extract raw | Same as DOCX pattern: warn, increment skipped, write original bytes. | |
| Skip the cover entirely | If cover can't be converted, don't extract it at all. | ✓ |
| You decide | Claude picks during planning. | |

**User's choice:** Skip the cover entirely
**Notes:** Stricter behavior than DOCX. User wants no file written if cover conversion fails.

### Question 4: Does "skip entirely" apply only to cover-only mode?

| Option | Description | Selected |
|--------|-------------|----------|
| Covers only | Cover-only mode skips on failure. All-images uses DOCX pattern. | ✓ |
| All EPUB images | All EPUB extraction skips on conversion failure. | |
| You decide | Claude picks during planning. | |

**User's choice:** Covers only (Recommended)
**Notes:** The skip-on-failure rule is scoped to cover-only mode. Regular extraction retains the warn+extract-raw pattern.

---

## Parameter Threading Depth

### Question 5: Should ExtractionConfig pass to internal helpers?

| Option | Description | Selected |
|--------|-------------|----------|
| Pass config to helpers | process_file() passes &ExtractionConfig to extract_all_images() and extract_cover_only(). | ✓ |
| Handle in process_file | Conversion logic stays in process_file(). Helpers return raw bytes. | |
| You decide | Claude picks during planning. | |

**User's choice:** Pass config to helpers (Recommended)
**Notes:** Conversion logic lives inside per-image loops in each helper, matching DOCX pattern.

---

## EPUB-specific Edge Cases

### Question 6: How should conversion handle MIME-derived extensions?

| Option | Description | Selected |
|--------|-------------|----------|
| Use MIME-derived extension | Extension from mime_to_extension() passed to try_convert(). Already compatible. | ✓ |
| Prefer path extension over MIME | Use file extension from resource path when available. | |
| You decide | Claude picks during planning. | |

**User's choice:** Use MIME-derived extension (Recommended)
**Notes:** No special handling needed. can_convert() checks same extensions that mime_to_extension() produces.

### Question 7: Cover images with unrecognized MIME — use defaulted "jpg" extension?

| Option | Description | Selected |
|--------|-------------|----------|
| Use defaulted extension | "jpg" fallback used for conversion. convert_image() detects from magic bytes anyway. | ✓ |
| Skip unrecognized MIME covers | Don't guess the format when conversion is active. | |
| You decide | Claude picks during planning. | |

**User's choice:** Use defaulted extension (Recommended)
**Notes:** Magic-byte detection in convert_image() handles the actual format. If decode fails, cover skipped per cover-only rule.

---

## Claude's Discretion

- Internal logic flow in extraction helpers for conversion decision path
- Whether to extract conversion logic into shared helper function
- Test strategy for conversion wiring
- How to update existing docx.rs tests for ExtractionConfig

## Deferred Ideas

None — discussion stayed within phase scope
