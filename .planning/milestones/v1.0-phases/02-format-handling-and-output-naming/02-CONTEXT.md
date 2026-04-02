# Phase 2: Format Handling and Output Naming - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Build the format handling and output naming utilities in `src/convert.rs` that let callers (Phases 5-6) cleanly handle unsupported formats and correct file extensions. This phase adds `ConversionResult` enum, `try_convert()` convenience function, and `OutputFormat::extension()` method. No changes to DOCX/EPUB processors -- those happen in Phases 5-6.

</domain>

<decisions>
## Implementation Decisions

### Warning Messages
- **D-01:** When an unsupported format is skipped during conversion, the warning includes the filename and format: `"Warning: Skipping conversion for {filename} ({FORMAT} format not supported for conversion)"`. Matches existing `eprintln!("Warning: ...")` pattern in the codebase.
- **D-02:** Warning printing is the caller's responsibility (not convert.rs). The convert module returns data; callers decide when/how to print warnings (e.g., using `pb.suspend()` when progress bars are active).

### Extension Replacement
- **D-03:** No changes to `get_unique_output_path()`. Callers pass the correct extension based on whether conversion happened. When conversion succeeds, pass the target format extension. When skipped, pass the original extension.
- **D-04:** `OutputFormat` gets an `extension()` method returning the target file extension string (`"jpg"`, `"png"`, `"webp"`).

### Skip-vs-Convert API
- **D-05:** A new `ConversionResult` enum with two variants: `Converted(Vec<u8>, String)` (converted bytes + target extension) and `Skipped(String)` (original extension preserved).
- **D-06:** A new `try_convert(data, source_ext, format, quality) -> Result<ConversionResult>` function that bundles `can_convert()` check + `convert_image()` call + extension resolution into one call. This is the primary API callers will use in Phases 5-6.
- **D-07:** `try_convert()` handles the `Ok(None)` return from `convert_image()` (unsupported format detected at decode time) by returning `Skipped`. Callers never need to deal with `Option` -- they match on `Converted` vs `Skipped`.

### Claude's Discretion
- Internal implementation details of `try_convert()` (how it composes `can_convert` and `convert_image`)
- Whether `ConversionResult` should derive additional traits beyond `Debug`
- Test strategy for the new API surface

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Context
- `.planning/PROJECT.md` -- Core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` -- CONV-03 (skip unsupported with warning), CONV-04 (correct extension)

### Prior Phase Context
- `.planning/phases/01-conversion-module-core/01-CONTEXT.md` -- Phase 1 decisions (D-04 through D-15) that this phase builds upon
- `.planning/phases/01-conversion-module-core/01-01-SUMMARY.md` -- What was built, deviations, patterns established

### Codebase
- `.planning/codebase/CONVENTIONS.md` -- Naming patterns, error handling, doc comment style
- `.planning/codebase/STRUCTURE.md` -- Module layout

### Source (integration points)
- `src/convert.rs` -- Where new types and functions will be added (OutputFormat, can_convert, convert_image already exist here)
- `src/common.rs` -- `get_unique_output_path()` (unchanged -- callers pass correct extension)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `convert_image()` in `src/convert.rs` -- returns `Result<Option<Vec<u8>>>`, the core conversion function that `try_convert()` wraps
- `can_convert()` in `src/convert.rs` -- pre-check for decodable extensions, used inside `try_convert()`
- `OutputFormat` enum in `src/convert.rs` -- needs `extension()` method added
- `get_unique_output_path()` in `src/common.rs` -- takes extension parameter, no changes needed

### Established Patterns
- All public functions return `anyhow::Result<T>`
- `Ok(None)` from `convert_image()` signals unsupported format (D-11 from Phase 1)
- `Err` from `convert_image()` signals corrupt/undecodable data (D-12 from Phase 1)
- Module doc comments with `//!`, function doc comments with `///`

### Integration Points
- `try_convert()` will be called by `docx::process_file()` (Phase 5) and `epub::process_file()` (Phase 6)
- Warning printing happens at the call site (in docx.rs/epub.rs), not in convert.rs

</code_context>

<specifics>
## Specific Ideas

- `try_convert()` is a convenience wrapper that eliminates the need for callers to deal with `Option<Vec<u8>>` from `convert_image()` -- the `ConversionResult` enum is more ergonomic
- The `Skipped` variant carries the original extension string so callers can use it directly in `get_unique_output_path()` without additional lookups
- `Converted` variant carries the target extension from `OutputFormat::extension()` so callers have everything they need in one match

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 02-format-handling-and-output-naming*
*Context gathered: 2026-04-02*
