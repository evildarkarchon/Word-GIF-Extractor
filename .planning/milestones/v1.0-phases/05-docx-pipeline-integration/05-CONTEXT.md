# Phase 5: DOCX Pipeline Integration - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Thread image format conversion and GIF routing through the DOCX processor (`src/docx.rs`) for end-to-end `--convert` operation on DOCX files. This phase also closes the lossless WebP gap in `src/convert.rs` (adding the encoding path that `--lossless` flag expects). After this phase, `word-image-extractor document.docx --convert png` produces converted images, with proper warnings for unsupported formats and detailed conversion statistics.

</domain>

<decisions>
## Implementation Decisions

### Conversion Summary Reporting
- **D-01:** When `--convert` is active, the finish message shows a detailed breakdown: `"Extracted N image(s), converted M, skipped K from D document(s)"`. This gives the user visibility into what conversion actually did.
- **D-02:** Conversion stats (converted/skipped counts) only appear in the finish message when `--convert` is active. Without `--convert`, the existing message format is unchanged.
- **D-03:** `ExtractionCounts` gains two new fields: `converted: usize` and `skipped: usize` to track conversion outcomes per-file.
- **D-04:** When both `--convert` and `--gif-output` are active, the finish message combines both: `"Extracted N image(s), converted M, skipped K, routed G GIF(s) to /path from D document(s)"`.

### Lossless WebP Encoding
- **D-05:** This phase adds the lossless WebP encoding path to `convert.rs`. `convert_image()` and `try_convert()` gain a `lossless: bool` parameter. A new `encode_webp_lossless()` function uses the `image` crate's built-in WebP encoder (already a dependency).
- **D-06:** The WebP encoding branch routes between lossy and lossless based on the `lossless` flag. When `lossless` is true and format is WebP, `encode_webp_lossless()` is called instead of `encode_webp_lossy()`.
- **D-07:** The `lossless` parameter is ignored for non-WebP formats (JPEG is always lossy, PNG is always lossless).

### Parameter Threading
- **D-08:** Conversion parameters are added as individual parameters to `docx::process_file()`: `convert: Option<OutputFormat>`, `quality: u8`, `lossless: bool`. This is consistent with the existing pattern (Phase 4 added `gif_output` the same way).
- **D-09:** The `process_file()` dispatch in `main.rs` threads the new parameters through to both DOCX and EPUB processors. The full signature becomes 7 parameters for DOCX.

### Conversion + GIF Routing Interaction (carried forward)
- **D-10:** GIF routing takes priority over conversion (Phase 4 D-01/D-02). When `--gif-output` is active, GIFs are written as-is (unconverted) to the GIF output directory. Non-GIF images are converted per `--convert`.
- **D-11:** Warning printing for skipped formats is the DOCX processor's responsibility (Phase 2 D-02). Uses `eprintln!("Warning: ...")` pattern.

### Claude's Discretion
- Internal logic flow in `docx::process_file()` for when to convert vs. route vs. skip
- Whether to refactor the per-image extraction loop or add conversion as a conditional step
- Test strategy for the integration (unit tests, integration tests, or both)
- How to handle the `lossless` flag in `try_convert()` call sites

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Context
- `.planning/PROJECT.md` -- Core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` -- CONV-01 (end-to-end conversion), traceability matrix

### Prior Phase Context
- `.planning/phases/01-conversion-module-core/01-CONTEXT.md` -- Conversion API design (D-04 through D-15), quality defaults, alpha compositing
- `.planning/phases/02-format-handling-and-output-naming/02-CONTEXT.md` -- ConversionResult enum (D-05), try_convert API (D-06), warning responsibility (D-02)
- `.planning/phases/03-cli-arguments-and-validation/03-CONTEXT.md` -- Flag definitions (D-01), lossless flag (D-08/D-09), ValueEnum on OutputFormat (D-10)
- `.planning/phases/04-gif-extraction-and-routing/04-CONTEXT.md` -- GIF routing priority (D-01/D-02), ExtractionCounts (D-10/D-11), summary reporting (D-06/D-07)

### Source (integration points)
- `src/convert.rs` -- `try_convert()`, `convert_image()`, `OutputFormat`, `ConversionResult` -- needs lossless WebP path added
- `src/docx.rs` -- `process_file()` -- main integration target, needs conversion logic threaded in
- `src/common.rs` -- `ExtractionCounts` (needs `converted`/`skipped` fields), `write_image_to_file()`, `get_unique_output_path()`
- `src/main.rs` -- `process_file()` dispatch (line 292), finish message logic (line 476), parameter threading

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `try_convert(data, source_ext, format, quality)` in `convert.rs` -- primary conversion API; returns `Converted(bytes, ext)` or `Skipped(original_ext)`. Needs `lossless` param added.
- `write_image_to_file(path, data)` in `common.rs` -- byte writer, works with both converted and raw bytes
- `get_unique_output_path(dir, name, seq, total, ext)` in `common.rs` -- handles naming; callers pass correct extension based on conversion result
- `ExtractionCounts` in `common.rs` -- currently has `extracted` and `gifs_routed`; gains `converted` and `skipped`
- `encode_webp_lossy(img, quality)` in `convert.rs` -- existing lossy path; lossless path follows same pattern using `image` crate

### Established Patterns
- Per-image loop in `docx.rs` already handles GIF routing with `is_gif` check (line 73)
- Archive data read into `Vec<u8>` before writing (line 92-94) -- conversion inserts between read and write
- `eprintln!("Warning: ...")` for non-fatal issues (no progress bar in docx.rs, so direct stderr is fine)
- Extension from `ConversionResult` variant feeds directly into `get_unique_output_path()`

### Integration Points
- `docx::process_file()` inner loop (line 68-102) -- per-image conversion decision point: after reading data, check if conversion is requested, call `try_convert()`, match on result
- `main.rs` dispatch function (line 292-321) -- threads new params from `Args` to docx/epub processors
- `main.rs` finish message (line 476-508) -- conditional stats formatting based on `--convert` presence
- `main.rs` accumulation loop (line 457-465) -- needs to accumulate `converted` and `skipped` from `ExtractionCounts`

</code_context>

<specifics>
## Specific Ideas

- Conversion decision in the per-image loop: after reading archive bytes, check `is_gif && gif_output.is_some()` first (GIF routing priority), then check `convert.is_some()` and call `try_convert()`. Match on `Converted` to get new bytes+extension, `Skipped` to use original bytes+extension.
- The `lossless` parameter to `convert_image()` is only meaningful when `format == Webp`. For JPEG/PNG branches, the parameter is simply not read.
- `ExtractionCounts` accumulation in main.rs already uses `+=` pattern; adding `converted` and `skipped` fields follows the same approach.

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 05-docx-pipeline-integration*
*Context gathered: 2026-04-02*
