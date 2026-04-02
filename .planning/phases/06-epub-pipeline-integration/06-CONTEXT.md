# Phase 6: EPUB Pipeline Integration - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Thread image format conversion and GIF routing through the EPUB processor (`src/epub.rs`) for end-to-end `--convert` operation on EPUB files, including cover-only mode. After this phase, `word-image-extractor book.epub --convert webp` produces converted images across all EPUB extraction modes (all images, cover only, metadata-filtered). This phase also introduces an `ExtractionConfig` struct to consolidate conversion-related parameters across both DOCX and EPUB processors.

</domain>

<decisions>
## Implementation Decisions

### Config struct refactor
- **D-01:** Create `ExtractionConfig<'a>` struct in `src/common.rs` bundling: `convert: Option<OutputFormat>`, `quality: u8`, `lossless: bool`, `gif_output: Option<&'a Path>`. Follows the `ExtractionCounts` pattern already established in common.rs.
- **D-02:** Both `docx::process_file()` and `epub::process_file()` accept `&ExtractionConfig` instead of individual `convert`, `quality`, `lossless`, `gif_output` parameters. This removes the need for `#[allow(clippy::too_many_arguments)]` on the main dispatch function.
- **D-03:** The `main.rs` dispatch function (`process_file()`) constructs `ExtractionConfig` once and passes it to both processors. EPUB-specific params (`cover_only`, `cover_fallback`, `filter`) remain as individual parameters on `epub::process_file()` since they don't apply to DOCX.

### Cover-only + conversion interaction
- **D-04:** When cover conversion fails (corrupt image, decode error, or unsupported format skip), the cover is **skipped entirely** -- no file is written. This is stricter than the DOCX batch pattern. Rationale: if you asked for one converted cover and it can't be converted, there's nothing useful to write.
- **D-05:** The "skip entirely on failure" rule applies **only to cover-only mode** (`-c`). Regular all-images extraction (`extract_all_images`) uses the DOCX pattern: warn to stderr, write raw bytes with original extension, increment `skipped` count.
- **D-06:** Cover-fallback (`--cover-fallback`) works normally with conversion: if no cover is found, fall back to `extract_all_images()` which uses the standard conversion pattern (warn + extract raw on failure).

### Parameter threading depth
- **D-07:** `ExtractionConfig` is passed through from `process_file()` to both internal helpers: `extract_all_images()` and `extract_cover_only()`. Conversion logic lives inside the per-image loop in each helper, matching the DOCX pattern.
- **D-08:** Internal helpers replace their individual `gif_output` parameter with `&ExtractionConfig`, since `gif_output` is now part of the struct.

### EPUB-specific extension handling
- **D-09:** The MIME-derived extension from `mime_to_extension()` is used as `source_ext` for `try_convert()`. This works correctly because `can_convert()` checks the same extensions that `mime_to_extension()` produces (e.g., "jpg" from "image/jpeg", "svg" from "image/svg+xml").
- **D-10:** For cover images with unrecognized MIME types, the existing "jpg" fallback extension is used for conversion. `convert_image()` detects the actual format from magic bytes, so the extension is just a pre-check hint. If decode fails, the cover is skipped per D-04.

### Carried forward from prior phases
- **D-11:** GIF routing takes priority over conversion (Phase 4 D-01/D-02). GIFs are written as-is to `gif_output` directory when set, even with `--convert` active.
- **D-12:** Warning printing uses `eprintln!("Warning: ...")` pattern (Phase 5 D-11). No progress bar inside EPUB processor.
- **D-13:** Summary reporting in `main.rs` already handles all conversion + GIF routing message combinations (Phase 5 D-01/D-04). No changes needed to main.rs finish messages.

### Claude's Discretion
- Internal logic flow in `extract_all_images()` and `extract_cover_only()` for the conversion decision path
- Whether to extract the conversion logic into a shared helper function used by both EPUB extraction paths
- Test strategy (unit tests for conversion wiring, integration-level tests, or both)
- How to update existing `docx.rs` tests to work with `ExtractionConfig`

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Context
- `.planning/PROJECT.md` -- Core value, constraints, key decisions (especially "Converted-only output" and "Skip unsupported formats")
- `.planning/REQUIREMENTS.md` -- CONV-01 (extends to EPUB), traceability matrix

### Prior Phase Context
- `.planning/phases/01-conversion-module-core/01-CONTEXT.md` -- Conversion API design, quality defaults, alpha compositing
- `.planning/phases/02-format-handling-and-output-naming/02-CONTEXT.md` -- ConversionResult enum, try_convert API, warning responsibility
- `.planning/phases/04-gif-extraction-and-routing/04-CONTEXT.md` -- GIF routing priority (D-01/D-02), ExtractionCounts design (D-10/D-11)
- `.planning/phases/05-docx-pipeline-integration/05-CONTEXT.md` -- DOCX conversion pattern (reference implementation), lossless WebP, parameter threading, summary reporting

### Source (integration points)
- `src/common.rs` -- `ExtractionCounts` (line 96), `write_image_to_file()` (line 160), `get_unique_output_path()` (line 109) -- `ExtractionConfig` struct will be added here
- `src/convert.rs` -- `try_convert()` (line 122), `ConversionResult` (line 34), `OutputFormat` (line 18) -- stable API, no changes needed
- `src/epub.rs` -- `process_file()` (line 128), `extract_all_images()` (line 180), `extract_cover_only()` (line 308) -- main integration targets
- `src/docx.rs` -- `process_file()` (line 20) -- needs refactor to accept `ExtractionConfig` instead of individual params
- `src/main.rs` -- `process_file()` dispatch (line 297), `ExtractionConfig` construction site, `#[allow(clippy::too_many_arguments)]` removal

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `try_convert(data, source_ext, format, quality, lossless)` in `convert.rs` -- primary conversion API, returns `ConversionResult`
- `write_image_to_file(path, data)` in `common.rs` -- byte writer, works with both converted and raw bytes
- `get_unique_output_path(dir, name, seq, total, ext)` in `common.rs` -- handles naming with correct extension from conversion result
- `ExtractionCounts` in `common.rs` -- already has `converted`/`skipped` fields from Phase 5
- `mime_to_extension()` in `epub.rs` -- maps MIME types to file extensions, compatible with `can_convert()` extension set
- DOCX conversion pattern in `docx.rs` lines 94-128 -- reference implementation for the per-image conversion loop

### Established Patterns
- Per-image GIF routing check in `extract_all_images()` (epub.rs line 245-254) -- conversion inserts after this check
- Per-image GIF routing in `extract_cover_only()` (epub.rs lines 335-340, 380-385) -- same pattern
- DOCX inner loop (docx.rs line 94-128): `is_routed_gif` check → `try_convert()` → match on result → write
- `eprintln!("Warning: ...")` for non-fatal issues in EPUB processor

### Integration Points
- `epub::process_file()` signature (line 128) -- add `&ExtractionConfig` param, remove `gif_output`
- `extract_all_images()` inner loop (lines 238-270) -- per-image conversion decision point
- `extract_cover_only()` cover-found branches (lines 321-363, 366-404) -- conversion + skip-on-failure logic
- `docx::process_file()` signature (line 20) -- refactor to accept `&ExtractionConfig`
- `main.rs` process_file dispatch (line 297) -- construct `ExtractionConfig`, update both call sites

</code_context>

<specifics>
## Specific Ideas

- The conversion decision in `extract_all_images()` follows the DOCX pattern exactly: after reading resource data and determining output dir (GIF routing), check `config.convert.is_some()` and `is_routed_gif`, then call `try_convert()` and match on the result.
- In `extract_cover_only()`, the conversion + skip-on-failure pattern differs: on `Skipped` or `Err`, return `Ok(ExtractionCounts::default())` instead of writing raw bytes. This implements the "covers skipped entirely on failure" decision.
- `ExtractionConfig` construction happens once in `main()` before the processing loop, then passed by reference through dispatch to each processor.
- The `docx.rs` refactor is mechanical: replace 4 individual params with `&ExtractionConfig`, update internal references from `convert` to `config.convert`, `quality` to `config.quality`, etc.

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 06-epub-pipeline-integration*
*Context gathered: 2026-04-02*
