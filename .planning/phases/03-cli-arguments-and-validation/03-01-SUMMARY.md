---
phase: 03-cli-arguments-and-validation
plan: 01
subsystem: cli
tags: [clap, ValueEnum, argument-validation, cli-flags]

# Dependency graph
requires:
  - phase: 02-format-handling-and-output-naming
    provides: OutputFormat enum in convert.rs
provides:
  - "--convert flag with OutputFormat ValueEnum integration"
  - "--quality flag with range validation (1-100)"
  - "--lossless flag for WebP lossless encoding"
  - "--gif-only flag for GIF-only extraction mode"
  - "--gif-output flag for separate GIF output directory"
  - "validate_args() for value-dependent argument validation"
  - "18 unit tests covering all new CLI flags and validation"
affects: [04-gif-separation-pipeline, 05-docx-conversion-integration, 06-epub-conversion-integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [clap-ValueEnum-for-enum-parsing, validate_args-for-value-dependent-checks, clap-conflicts_with-and-requires-for-declarative-validation]

key-files:
  created: []
  modified:
    - src/convert.rs
    - src/main.rs

key-decisions:
  - "ValueEnum derive on OutputFormat enables clap to parse --convert values directly into the enum"
  - "Declarative clap validation (conflicts_with, requires, value_parser range) handles presence-based rules; validate_args() handles value-dependent rules"
  - "Short flags chosen: -C (convert), -q (quality), -L (lossless), -g (gif-only), -G (gif-output)"

patterns-established:
  - "validate_args pattern: post-parse validation for rules that depend on argument values, not just presence"
  - "ValueEnum derive pattern: enum variants automatically become valid CLI values"

requirements-completed: [CONV-06, CONV-07, GIF-04]

# Metrics
duration: 3min
completed: 2026-04-02
---

# Phase 3 Plan 1: CLI Arguments and Validation Summary

**Five new CLI flags (--convert, --quality, --lossless, --gif-only, --gif-output) with declarative and manual validation, ValueEnum-derived OutputFormat parsing, and 18 unit tests**

## Performance

- **Duration:** 3 min
- **Started:** 2026-04-02T10:10:24Z
- **Completed:** 2026-04-02T10:13:09Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- Added ValueEnum derive to OutputFormat for direct clap parsing of --convert flag values (jpg, png, webp)
- Added 5 new Args fields with full declarative validation (conflicts_with, requires, value_parser range)
- Implemented validate_args() for value-dependent validation (quality+png conflict, lossless+non-webp conflict)
- Added 18 comprehensive unit tests covering all flag combinations, short flags, conflicts, ranges, and existing flag compatibility
- All 55 tests pass (18 new + 37 existing), clippy clean (exit 0)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add ValueEnum derive to OutputFormat and new CLI flags to Args** - `5362c02` (feat)

**Plan metadata:** (pending final commit)

## Files Created/Modified
- `src/convert.rs` - Added `use clap::ValueEnum` import and `ValueEnum` derive on OutputFormat enum
- `src/main.rs` - Added OutputFormat import, 5 new Args fields, validate_args() function, 18 unit tests, removed dead_code allow on convert module

## Decisions Made
- Kept bidirectional `conflicts_with` on convert/gif_only and quality/lossless for code readability even though clap handles either direction
- Placed conversion flags (convert, quality, lossless) before GIF flags (gif_only, gif_output) in Args struct for logical grouping
- Used `value_parser = clap::value_parser!(u8).range(1..=100)` for quality range enforcement at parse time

## Deviations from Plan

None - plan executed exactly as written.

**Note:** Removing `#[allow(dead_code)]` from `mod convert` (as specified in plan) exposes 9 clippy warnings for convert.rs functions not yet used outside tests. These are pre-existing built-ahead functions that will be integrated in Phase 5. Clippy exits with code 0 (warnings, not errors). The warnings will resolve naturally when Phase 5 integrates the conversion pipeline.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 5 CLI flags are parsed and validated, ready for Phase 4 (GIF separation) and Phase 5 (DOCX conversion integration)
- The `args.convert`, `args.quality`, `args.lossless`, `args.gif_only`, and `args.gif_output` fields are available in main() for downstream use
- `validate_args()` ensures invalid combinations are caught before processing begins

## Self-Check: PASSED

- All source files exist (src/convert.rs, src/main.rs)
- SUMMARY.md created at expected path
- Commit 5362c02 found in git log
- All 12 acceptance criteria verified

---
*Phase: 03-cli-arguments-and-validation*
*Completed: 2026-04-02*
