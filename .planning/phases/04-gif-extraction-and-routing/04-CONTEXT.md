# Phase 4: GIF Extraction and Routing - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement GIF-only filtering (`--gif-only`) and GIF output routing (`--gif-output <path>`) in the extraction pipeline. `--gif-only` restricts extraction to GIF files only. `--gif-output` routes GIF files to a separate output directory while non-GIF files go to the default output. These two flags are orthogonal -- they can be used independently or together. No conversion logic changes -- GIF routing is purely a directory routing concern.

</domain>

<decisions>
## Implementation Decisions

### GIF Routing with Conversion
- **D-01:** When `--gif-output` and `--convert` are both used, GIF files are written as-is (unconverted) to the GIF output directory. Non-GIF images are converted to the target format and written to the default output directory. This closes the STATE.md research gap.
- **D-02:** GIF routing takes priority over conversion -- a GIF file is never converted when `--gif-output` is active. The routing decision is made before any conversion attempt.

### Flag Orthogonality
- **D-03:** `--gif-only` is purely a filter -- it restricts `allowed_extensions` to `{"gif"}`. Output goes to the default output directory (or `--gif-output` if specified).
- **D-04:** `--gif-output` is purely routing -- it directs GIF files to a separate directory. It does not filter out non-GIF files (unless `--gif-only` is also used).
- **D-05:** When `--gif-only` is used without `--gif-output`, GIFs go to the default output directory (`-o`/`--output` or current dir).

### Summary Reporting
- **D-06:** Split counts in the finish message only when `--gif-output` is active: `"Extracted N image(s), routed M GIF(s) to /path/to/gifs from K document(s)"`.
- **D-07:** When `--gif-only` is used alone (no `--gif-output`), the existing finish message format suffices since all extracted images are GIFs.
- **D-08:** Per-file progress bar stays unchanged -- GIF routing info appears only in the final summary message, not during per-file processing.

### Processor API Shape
- **D-09:** Add `gif_only: bool` and `gif_output: Option<&Path>` as extra parameters to `docx::process_file()` and `epub::process_file()` signatures. Follows the existing pattern of adding parameters as needed.
- **D-10:** Processors return `ExtractionCounts` struct instead of `usize`. The struct has named fields `extracted: usize` and `gifs_routed: usize` for readable call sites and future extensibility (Phase 5/6 may add conversion counts).
- **D-11:** `ExtractionCounts` lives in `src/common.rs` alongside other shared types (`ImageToExtract`).

### Claude's Discretion
- Where exactly `--gif-only` filtering is applied (main.rs before calling processors, or inside processors)
- Directory creation timing for `--gif-output` path (upfront or on first GIF found)
- Whether `ExtractionCounts` derives `Default`, `Add`, or other utility traits
- Test strategy and test file organization

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Context
- `.planning/PROJECT.md` -- Core value, constraints, key decisions (especially "GIF separation is a routing concern")
- `.planning/REQUIREMENTS.md` -- GIF-01 (gif-only filter), GIF-02 (gif-output routing), GIF-03 (gif-output independent of gif-only)

### Prior Phase Context
- `.planning/phases/01-conversion-module-core/01-CONTEXT.md` -- Conversion API design (D-04 through D-07), convert_image returns bytes
- `.planning/phases/02-format-handling-and-output-naming/02-CONTEXT.md` -- ConversionResult enum (D-05), try_convert API (D-06), warning printing is caller's responsibility (D-02)
- `.planning/phases/03-cli-arguments-and-validation/03-CONTEXT.md` -- Flag definitions (D-01), clap attributes (D-05/D-06), ValueEnum on OutputFormat (D-10)

### Source (integration points)
- `src/main.rs` lines 62-80 -- Args struct with `gif_only` and `gif_output` fields already defined
- `src/main.rs` lines 292-319 -- `process_file()` dispatch function that needs signature update
- `src/main.rs` lines 408-486 -- Main processing loop and finish message (routing and reporting changes)
- `src/docx.rs` lines 16-84 -- `docx::process_file()` that needs GIF routing logic
- `src/epub.rs` -- `epub::process_file()` that needs GIF routing logic
- `src/common.rs` lines 82-89 -- `ImageToExtract` struct (model for `ExtractionCounts`)
- `src/common.rs` lines 92-140 -- `get_unique_output_path()` used for both normal and GIF output paths

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `get_unique_output_path()` in `common.rs` -- handles naming and deduplication; works with any output directory, so it handles GIF output path with no changes
- `write_image_to_file()` in `common.rs` -- byte writer; works with any path
- `ImageToExtract` in `common.rs` -- carries `extension` field, which is the key for GIF routing decisions (check if extension == "gif")
- `Args` struct in `main.rs` -- already has `gif_only: bool` and `gif_output: Option<PathBuf>` fields from Phase 3

### Established Patterns
- `fs::create_dir_all(output_base_dir)` called before extraction loop in processors
- `process_file()` dispatch in `main.rs` passes through all params to format-specific processors
- Extension checking: `allowed_extensions.contains(ext_lower.as_str())` pattern in both docx.rs and epub.rs
- Return type accumulation: main.rs accumulates `total_images += count` in the processing loop

### Integration Points
- `process_file()` in `main.rs` (line 292) dispatches to docx/epub -- needs updated signature and return type
- Main processing loop (line 422) accumulates counts -- needs to handle `ExtractionCounts` struct
- Finish message logic (line 466) -- needs conditional split-count reporting when `gif_output` is active
- `docx::process_file()` inner extraction loop (line 64) -- per-image routing decision point
- `epub::process_file()` inner extraction logic -- per-image routing decision point

</code_context>

<specifics>
## Specific Ideas

- `--gif-only` filtering can be implemented by overriding `allowed_extensions` to `{"gif"}` in main.rs before calling processors, keeping processor logic unchanged for this flag
- GIF routing decision inside processors: after reading image bytes, check if extension is "gif" and `gif_output` is Some -- if so, write to GIF output dir instead of default
- `ExtractionCounts` struct enables Phase 5/6 to add `converted: usize` and `skipped: usize` fields without another return type change

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 04-gif-extraction-and-routing*
*Context gathered: 2026-04-02*
