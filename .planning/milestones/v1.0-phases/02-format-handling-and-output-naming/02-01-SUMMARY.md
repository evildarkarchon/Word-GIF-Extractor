---
phase: 02-format-handling-and-output-naming
plan: 01
subsystem: conversion
tags: [image, conversion, api, enum, rust]

# Dependency graph
requires:
  - phase: 01-conversion-module-core
    provides: OutputFormat enum, can_convert(), convert_image(), test helpers
provides:
  - ConversionResult enum (Converted/Skipped variants)
  - OutputFormat::extension() method
  - try_convert() convenience function wrapping can_convert + convert_image
affects: [05-docx-integration, 06-epub-integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [result-enum-for-conversion-outcome, single-call-conversion-api]

key-files:
  created: []
  modified: [src/convert.rs]

key-decisions:
  - "ConversionResult derives only Debug (Clone is expensive for Vec<u8>, add later if needed)"
  - "OutputFormat::extension() returns &'static str (zero allocation, callers use .to_string() when needed)"
  - "try_convert() does not print warnings (D-02: callers own warning output)"

patterns-established:
  - "Result enum pattern: ConversionResult carries both data and extension in each variant, eliminating secondary lookups"
  - "Extension normalization: Skipped variant always carries lowercase extension via to_lowercase()"
  - "Composition wrapper: try_convert wraps can_convert + convert_image, converging two skip paths into Skipped"

requirements-completed: [CONV-03, CONV-04]

# Metrics
duration: 2min
completed: 2026-04-02
---

# Phase 2 Plan 1: Format Handling and Output Naming Summary

**ConversionResult enum, OutputFormat::extension(), and try_convert() wrapping can_convert + convert_image into a single-call API for Phases 5-6 integration**

## Performance

- **Duration:** 2 min
- **Started:** 2026-04-02T09:24:57Z
- **Completed:** 2026-04-02T09:27:01Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Added `ConversionResult` enum with `Converted(Vec<u8>, String)` and `Skipped(String)` variants for ergonomic conversion result handling
- Added `OutputFormat::extension()` returning `&'static str` for "jpg", "png", "webp"
- Added `try_convert()` function composing `can_convert()` + `convert_image()` into a single call with clear Skipped-vs-Err semantics
- 7 new unit tests covering all paths: supported conversion, unsupported extension skip, decode-time skip, correct extension, corrupt data error, case normalization

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): TDD failing tests** - `2b62c3a` (test)
2. **Task 1 (GREEN): Production implementation** - `c0ae17c` (feat)

_TDD task: RED (failing tests) then GREEN (implementation). No refactoring needed._

## Files Created/Modified
- `src/convert.rs` - Added ConversionResult enum, OutputFormat::extension() impl, try_convert() function, and 7 new tests

## Decisions Made
- `ConversionResult` derives only `Debug` -- `Clone` would be expensive for `Vec<u8>` image data and no caller needs it yet
- `OutputFormat::extension()` returns `&'static str` to avoid allocation; callers call `.to_string()` when constructing owned `ConversionResult` variants
- No warnings printed inside `try_convert()` -- per D-02, callers own warning output (they may need `pb.suspend()` for progress bar compatibility)
- No refactoring needed in REFACTOR phase -- implementation was already minimal and clean

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required

None - no external service configuration required.

## Known Stubs

None - all public API items are fully implemented and tested.

## Next Phase Readiness
- `ConversionResult`, `OutputFormat::extension()`, and `try_convert()` are ready for integration in Phases 5 (DOCX) and 6 (EPUB)
- Callers will match on `Converted(bytes, ext)` vs `Skipped(ext)` and pass the extension to `get_unique_output_path()`
- Warning printing per D-01 format is the caller's responsibility (Phases 5-6)
- All 37 tests pass (12 existing convert tests + 7 new + 18 other module tests)

## Self-Check: PASSED

- FOUND: src/convert.rs
- FOUND: 02-01-SUMMARY.md
- FOUND: 2b62c3a (RED commit)
- FOUND: c0ae17c (GREEN commit)
- FOUND: all 9 acceptance criteria patterns in src/convert.rs
- cargo test: 37 passed, 0 failed
- cargo clippy: clean
- cargo fmt --check: clean

---
*Phase: 02-format-handling-and-output-naming*
*Completed: 2026-04-02*
