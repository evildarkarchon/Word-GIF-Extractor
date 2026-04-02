# Phase 2: Format Handling and Output Naming - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md -- this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 02-format-handling-and-output-naming
**Areas discussed:** Warning messages, Extension swap, Skip-vs-convert API

---

## Warning Messages

| Option | Description | Selected |
|--------|-------------|----------|
| File + format only | "Warning: Skipping conversion for document_1.wmf (WMF format not supported for conversion)" -- minimal, matches existing warning style | ✓ |
| File + format + target | "Warning: Cannot convert image1.wmf (WMF) to PNG -- extracting as-is" -- tells user what they asked for and why | |
| Terse format-only | "Warning: WMF not convertible" -- shortest possible, no filename | |

**User's choice:** File + format only (Recommended)
**Notes:** Matches existing `eprintln!("Warning: ...")` pattern in the codebase. Includes filename for debugging large batches.

---

## Extension Swap

| Option | Description | Selected |
|--------|-------------|----------|
| Callers pass correct ext | No changes to get_unique_output_path(). Callers pass target format extension when converted, original when not. Simplest approach. | ✓ |
| New helper: resolve_output_extension() | Shared utility in common.rs that takes (original_ext, target_format, was_converted) and returns correct extension | |
| Modify get_unique_output_path() | Add optional target_extension override parameter. Changes the shared API signature. | |

**User's choice:** Callers pass correct ext (Recommended)
**Notes:** No new utilities needed. The existing naming pipeline is already flexible enough.

---

## Skip-vs-Convert API

| Option | Description | Selected |
|--------|-------------|----------|
| High-level try_convert() | New function in convert.rs: try_convert(data, ext, format, quality) -> Result<ConversionResult>. Bundles can_convert + convert_image + extension resolution. | ✓ |
| OutputFormat::extension() only | Just add extension() method. Callers compose can_convert() + convert_image() themselves. More flexible, less convenient. | |
| Both: extension() + try_convert() | Add both the building block and the convenience wrapper. | |

**User's choice:** High-level try_convert() (Recommended)
**Notes:** One call does the full check-convert-or-skip flow. ConversionResult enum carries the correct extension in both variants.

---

## Claude's Discretion

- Internal implementation details of try_convert()
- Whether ConversionResult derives additional traits beyond Debug
- Test strategy for the new API surface

## Deferred Ideas

None -- discussion stayed within phase scope
