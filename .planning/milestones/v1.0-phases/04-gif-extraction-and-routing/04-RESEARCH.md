# Phase 4: GIF Extraction and Routing - Research

**Researched:** 2026-04-02
**Domain:** CLI argument logic, directory routing, return type refactoring (Rust)
**Confidence:** HIGH

## Summary

Phase 4 implements two orthogonal features: `--gif-only` filtering and `--gif-output <path>` routing. Both CLI flags already exist in the `Args` struct (defined in Phase 3). This phase wires them into the extraction pipeline.

The implementation is entirely within existing Rust code -- no new dependencies, no new crates, no external tools. The core changes are: (1) override `allowed_extensions` when `--gif-only` is set, (2) add GIF routing logic in the per-image extraction loops of `docx.rs` and `epub.rs`, (3) introduce `ExtractionCounts` return type in `common.rs`, and (4) update the finish message in `main.rs` to show split counts when `--gif-output` is active.

**Primary recommendation:** Implement `--gif-only` as a filter in `main.rs` (before calling processors), and `--gif-output` as a per-image routing decision inside each processor. Return `ExtractionCounts` struct from processors to enable split reporting.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** When `--gif-output` and `--convert` are both used, GIF files are written as-is (unconverted) to the GIF output directory. Non-GIF images are converted to the target format and written to the default output directory.
- **D-02:** GIF routing takes priority over conversion -- a GIF file is never converted when `--gif-output` is active. The routing decision is made before any conversion attempt.
- **D-03:** `--gif-only` is purely a filter -- it restricts `allowed_extensions` to `{"gif"}`. Output goes to the default output directory (or `--gif-output` if specified).
- **D-04:** `--gif-output` is purely routing -- it directs GIF files to a separate directory. It does not filter out non-GIF files (unless `--gif-only` is also used).
- **D-05:** When `--gif-only` is used without `--gif-output`, GIFs go to the default output directory (`-o`/`--output` or current dir).
- **D-06:** Split counts in the finish message only when `--gif-output` is active: `"Extracted N image(s), routed M GIF(s) to /path/to/gifs from K document(s)"`.
- **D-07:** When `--gif-only` is used alone (no `--gif-output`), the existing finish message format suffices since all extracted images are GIFs.
- **D-08:** Per-file progress bar stays unchanged -- GIF routing info appears only in the final summary message, not during per-file processing.
- **D-09:** Add `gif_only: bool` and `gif_output: Option<&Path>` as extra parameters to `docx::process_file()` and `epub::process_file()` signatures.
- **D-10:** Processors return `ExtractionCounts` struct instead of `usize`. The struct has named fields `extracted: usize` and `gifs_routed: usize`.
- **D-11:** `ExtractionCounts` lives in `src/common.rs` alongside other shared types (`ImageToExtract`).

### Claude's Discretion
- Where exactly `--gif-only` filtering is applied (main.rs before calling processors, or inside processors)
- Directory creation timing for `--gif-output` path (upfront or on first GIF found)
- Whether `ExtractionCounts` derives `Default`, `Add`, or other utility traits
- Test strategy and test file organization

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| GIF-01 | User can extract only GIF files via `--gif-only` flag (all non-GIF image formats are skipped) | Override `allowed_extensions` to `{"gif"}` in main.rs when `gif_only` is true. Flag already exists in Args (line 74-75). Extension filtering already works via the `allowed_extensions` parameter passed to processors. |
| GIF-02 | User can route extracted GIF files to a separate directory via `--gif-output <path>` | Per-image routing decision inside docx.rs and epub.rs extraction loops: check if extension == "gif" and `gif_output` is Some, then use gif output dir for `get_unique_output_path`. Flag already exists in Args (line 78-79). |
| GIF-03 | `--gif-output` works independently of `--gif-only` -- GIFs are routed to the GIF output directory even when extracting all formats | The routing logic in processors is independent of the `allowed_extensions` filter. When `--gif-output` is set without `--gif-only`, all format images are extracted, but GIFs go to the GIF directory and non-GIFs go to the default directory. |
</phase_requirements>

## Standard Stack

No new dependencies. This phase uses only existing project dependencies:

### Core (already in Cargo.toml)
| Library | Version (spec) | Resolved | Purpose in Phase 4 |
|---------|----------------|----------|---------------------|
| `clap` | `4.5.4` | `4.5.53` | Args struct already has `gif_only` and `gif_output` fields |
| `anyhow` | `1.0.82` | `1.0.100` | Error handling for directory creation |

### Supporting (already in Cargo.toml)
| Library | Version | Purpose in Phase 4 |
|---------|---------|---------------------|
| `std::fs` | stdlib | `create_dir_all` for GIF output directory |
| `std::collections::HashSet` | stdlib | Override `allowed_extensions` for `--gif-only` |

**No new dependencies needed.** No `Cargo.toml` changes required.

## Architecture Patterns

### Recommended Change Structure

```
src/
  common.rs    # ADD: ExtractionCounts struct
  docx.rs      # MODIFY: process_file signature + GIF routing in extraction loop
  epub.rs      # MODIFY: process_file signature + GIF routing in extraction loops
  main.rs      # MODIFY: --gif-only filter, process_file dispatch, accumulation, finish message
  convert.rs   # NO CHANGES
```

### Pattern 1: Filter at Orchestration Layer (--gif-only)

**What:** Override `allowed_extensions` in `main.rs` before the processing loop.
**When to use:** When a flag restricts what images to extract (not where they go).
**Rationale for this location:** `--gif-only` is logically equivalent to `-f gif`. It restricts the set of extensions before any processor runs. Applying it in `main.rs` means both docx.rs and epub.rs automatically respect it without any per-processor changes for this flag.

**Example integration point (main.rs lines 358-372):**
```rust
// After existing target_extensions logic:
if args.gif_only {
    target_extensions.clear();
    target_extensions.insert("gif");
}
```

This goes after the existing `target_extensions` population block but before the processing loop.

### Pattern 2: Route at Processor Layer (--gif-output)

**What:** Inside each processor's per-image extraction loop, choose output directory based on extension.
**When to use:** When a flag changes where images are written (not which images are extracted).
**Rationale for this location:** The routing decision requires knowledge of the current image's extension, which is only available inside the processor's loop. Both docx.rs and epub.rs need parallel routing logic.

**Example decision point (in the extraction loop):**
```rust
let output_dir = if image.extension == "gif" {
    if let Some(gif_dir) = gif_output {
        gif_dir
    } else {
        output_base_dir
    }
} else {
    output_base_dir
};
```

### Pattern 3: Structured Return Type (ExtractionCounts)

**What:** Replace `usize` return type with a struct carrying named counts.
**When to use:** When callers need more than one piece of information from the result.
**Rationale:** D-10 requires this. The struct enables split reporting in the finish message and is forward-compatible with Phase 5/6 (conversion counts).

**Struct definition in common.rs:**
```rust
/// Counts of images extracted during a single file processing operation.
#[derive(Debug, Default)]
pub struct ExtractionCounts {
    /// Total number of images extracted (includes GIFs routed)
    pub extracted: usize,
    /// Number of GIF files routed to the GIF output directory
    pub gifs_routed: usize,
}
```

### Anti-Patterns to Avoid

- **Duplicating the `--gif-only` filter inside both processors:** This should be handled once in `main.rs` by overriding `allowed_extensions`. The processors don't need to know about `gif_only`.
- **Creating GIF output directory unconditionally at startup:** Only create it when a GIF is actually found. This avoids empty directories when no GIFs exist in the input.
- **Adding `gif_only` as a processor parameter:** Per D-03, `--gif-only` is purely a filter on `allowed_extensions`. It does not need to be passed to processors. Only `gif_output: Option<&Path>` needs to be a processor parameter.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Unique filename generation in GIF output dir | Custom naming logic | `get_unique_output_path()` from common.rs | Already handles counter-based dedup, works with any base directory |
| Directory creation | Manual existence checks | `fs::create_dir_all()` | Idempotent, handles nested paths |
| Extension comparison | Custom string matching | `image.extension == "gif"` / `ext_lower == "gif"` | Both processors already have the lowercase extension in their loop variables |

## Common Pitfalls

### Pitfall 1: Double-Counting GIFs in Total
**What goes wrong:** GIFs routed to `--gif-output` are counted in both `extracted` and `gifs_routed`, leading to confusing totals in the finish message.
**Why it happens:** `gifs_routed` is a subset of `extracted`, not additive.
**How to avoid:** Define clearly that `extracted` includes ALL images (including routed GIFs). The finish message must not add them: "Extracted N image(s), routed M GIF(s) to /path" where M <= N.
**Warning signs:** If the message says "Extracted 5 image(s)" but the user only sees 3 files in the default output dir (the other 2 are GIFs in the GIF dir).

### Pitfall 2: Forgetting to Create GIF Output Directory
**What goes wrong:** `write_image_to_file` fails with "Failed to create output file" because the GIF output directory doesn't exist.
**Why it happens:** The existing `create_dir_all(output_base_dir)` only creates the default output directory. The GIF output directory needs its own `create_dir_all`.
**How to avoid:** Call `create_dir_all` for the GIF output directory before writing the first GIF to it. Can be done lazily (on first GIF) or eagerly (at processor entry if `gif_output` is Some).
**Warning signs:** Tests pass when the directory already exists but fail on clean runs.

### Pitfall 3: EPUB Cover-Only Mode + GIF Routing
**What goes wrong:** The cover extraction path in epub.rs (`extract_cover_only`) has separate logic from `extract_all_images`. GIF routing might be added to `extract_all_images` but forgotten in `extract_cover_only`.
**Why it happens:** EPUB has two extraction code paths, and both need the routing logic.
**How to avoid:** Both `extract_cover_only` and `extract_all_images` must check extension and route GIFs. Alternatively, refactor the write step into a shared helper that handles routing.
**Warning signs:** GIF covers are not routed to `--gif-output`.

### Pitfall 4: Sequence Numbering Across Two Directories
**What goes wrong:** When some images go to the default dir and GIFs go to the GIF dir, `get_unique_output_path` is called with sequential indices. The numbering gap (e.g., `doc_1.png`, `doc_3.png` with `doc_2.gif` in the GIF dir) may confuse users.
**Why it happens:** Sequential indices are based on the full image list, not the per-directory list.
**How to avoid:** This is expected behavior given the existing naming pattern. The CONTEXT.md does not require re-sequencing. Accept the numbering as-is -- the index reflects the image's position in the source document, which is actually informative.
**Warning signs:** User confusion, but this is acceptable behavior.

### Pitfall 5: `--gif-output` with `--cover-only` and No GIF Cover
**What goes wrong:** If the EPUB cover is a PNG and `--gif-output` is set (but `--gif-only` is not), the PNG cover goes to the default dir and nothing goes to the GIF dir. This is correct behavior, but `gifs_routed: 0` should not trigger the split message.
**How to avoid:** Only include the "routed M GIF(s)" text in the finish message when `gifs_routed > 0`.

## Code Examples

### ExtractionCounts Struct (common.rs)

```rust
/// Counts of images extracted during a single file processing operation.
///
/// `extracted` counts ALL images written to disk (including GIFs routed
/// to a separate directory). `gifs_routed` counts only those GIFs written
/// to the `--gif-output` directory specifically.
#[derive(Debug, Default)]
pub struct ExtractionCounts {
    /// Total number of images extracted (includes routed GIFs)
    pub extracted: usize,
    /// Number of GIF files routed to the GIF output directory
    pub gifs_routed: usize,
}
```

Deriving `Default` gives a zero-valued instance for early returns. Deriving `Debug` follows project conventions. No need for `Clone` (counts are cheap to copy as they're `usize`), but `Copy` could be derived if convenient. `Add` is not needed -- accumulation happens field-by-field in main.rs.

### DOCX GIF Routing (docx.rs extraction loop)

```rust
// Inside the extraction loop, after reading image data:
let is_gif = image.extension == "gif";
let effective_output_dir = if is_gif && gif_output.is_some() {
    gif_output.unwrap()
} else {
    output_base_dir
};

// Ensure the target directory exists (handles both paths)
fs::create_dir_all(effective_output_dir).context("Failed to create output directory")?;

let output_path = get_unique_output_path(
    effective_output_dir,
    &doc_name,
    seq_index,
    total_images,
    &image.extension,
)?;

write_image_to_file(&output_path, &data)?;

if is_gif && gif_output.is_some() {
    counts.gifs_routed += 1;
}
counts.extracted += 1;
```

### Main.rs Accumulation and Finish Message

```rust
// Accumulation in processing loop:
let mut total_counts = ExtractionCounts::default();
// ...
match process_file(...) {
    Ok(counts) => {
        total_counts.extracted += counts.extracted;
        total_counts.gifs_routed += counts.gifs_routed;
        if counts.extracted > 0 {
            total_documents += 1;
            // ... existing has_docx_images logic
        }
    }
    // ...
}

// Finish message with conditional split reporting:
if total_counts.extracted > 0 {
    let item_name = /* existing cover/image logic */;
    if total_counts.gifs_routed > 0 {
        // D-06: Split counts when --gif-output is active
        let gif_dir = args.gif_output.as_ref().unwrap();
        pb.finish_with_message(format!(
            "Extracted {} {}, routed {} GIF(s) to {} from {} document(s)",
            total_counts.extracted, item_name,
            total_counts.gifs_routed,
            gif_dir.display(),
            total_documents
        ));
    } else {
        // Existing message format
        pb.finish_with_message(format!(
            "Extracted {} {} from {} document(s)",
            total_counts.extracted, item_name, total_documents
        ));
    }
}
```

### Updated process_file Dispatch (main.rs)

```rust
fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    epub_filter: &EpubFilter,
    gif_output: Option<&Path>,
) -> Result<ExtractionCounts> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => {
            docx::process_file(input_path, output_base_dir, allowed_extensions, gif_output)
        }
        Some(DocumentType::Epub) => epub::process_file(
            input_path,
            output_base_dir,
            allowed_extensions,
            cover_only,
            cover_fallback,
            epub_filter,
            gif_output,
        ),
        None => {
            anyhow::bail!(
                "Unsupported file type: {}",
                input_path.display()
            );
        }
    }
}
```

Note: `gif_only` is NOT passed to processors (per anti-pattern above). It is applied in `main.rs` by overriding `target_extensions`.

## State of the Art

No external technology changes relevant. This phase is purely internal Rust code changes.

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `process_file` returns `usize` | Returns `ExtractionCounts` struct | This phase | All callers updated; main.rs accumulation changes from `+= count` to field-by-field |
| Single output directory per processor | Conditional output directory per image | This phase | Processors gain `gif_output: Option<&Path>` parameter |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` with `cargo test` |
| Config file | None (default Cargo test runner) |
| Quick run command | `cargo test` |
| Full suite command | `cargo test` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| GIF-01 | `--gif-only` restricts `allowed_extensions` to `{"gif"}` | unit | `cargo test --lib tests::test_gif_only_overrides_extensions -x` | No -- Wave 0 |
| GIF-01 | `--gif-only` combined with `--formats` (gif-only wins) | unit | `cargo test --lib tests::test_gif_only_overrides_formats -x` | No -- Wave 0 |
| GIF-02 | GIF images routed to `--gif-output` directory | unit | `cargo test --lib tests::test_gif_output_routing -x` | No -- Wave 0 |
| GIF-03 | `--gif-output` routes GIFs without filtering non-GIFs | unit | `cargo test --lib tests::test_gif_output_independent_of_gif_only -x` | No -- Wave 0 |
| D-06 | Split finish message when GIFs were routed | unit | `cargo test --lib tests::test_extraction_counts_accumulation -x` | No -- Wave 0 |
| D-10 | `ExtractionCounts` struct has correct fields and Default | unit | `cargo test --lib common::tests::test_extraction_counts_default -x` | No -- Wave 0 |
| D-09 | Processor signatures accept `gif_output` parameter | compile | `cargo test --no-run` | N/A (compile check) |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green (`cargo test` + `cargo clippy`) before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `ExtractionCounts` unit tests in `common::tests` -- covers D-10, D-11
- [ ] GIF filtering logic tests in `tests` module (main.rs) -- covers GIF-01
- [ ] GIF routing tests -- these are harder to unit test since they involve filesystem I/O. Integration tests with `tempdir` would be ideal but the project currently has no integration test infrastructure. Recommend testing via the `ExtractionCounts` return values and argument parsing tests.

Note: The project has no `tests/` directory for integration tests. All existing tests are in-module `#[cfg(test)]` blocks. This phase should follow the same pattern. Full end-to-end GIF routing tests (requiring actual DOCX/EPUB files) can be verified manually during execution.

## Integration Point Analysis

### File-by-File Change Map

**`src/common.rs`** (ADD only -- no existing code changes):
- Add `ExtractionCounts` struct after `ImageToExtract` (around line 89)
- Add unit tests for the new struct

**`src/docx.rs`** (MODIFY):
- Line 16: `process_file` signature gains `gif_output: Option<&Path>`, return type changes to `Result<ExtractionCounts>`
- Line 60: Move `create_dir_all` to be conditional/per-image (or keep and add GIF dir creation)
- Lines 64-81: Extraction loop gains routing logic (choose output dir based on extension)
- Return value changes from `Ok(total_images)` to `Ok(counts)`

**`src/epub.rs`** (MODIFY):
- Line 127: `process_file` signature gains `gif_output: Option<&Path>`, return type changes to `Result<ExtractionCounts>`
- Line 156: Pass `gif_output` through to `extract_cover_only`
- Line 166: Pass `gif_output` through to `extract_all_images`
- `extract_all_images` (line 176): Add routing logic in its extraction loop (lines 231-246)
- `extract_cover_only` (line 284): Add routing logic for cover image writes (lines 296-316, 321-338, 342-349)
- All return paths change from `Ok(N)` to `Ok(ExtractionCounts { ... })`

**`src/main.rs`** (MODIFY):
- After line 372: Add `--gif-only` filter (`target_extensions = {"gif"}`)
- Line 292-319: `process_file` dispatch gains `gif_output` parameter, return type changes
- Lines 408-409: `total_images` becomes `total_counts: ExtractionCounts` with field-by-field accumulation
- Line 440-447: `process_file` call gains `args.gif_output.as_deref()` argument
- Line 448: `Ok(count)` becomes `Ok(counts)` with struct field access
- Lines 466-486: Finish message logic gains conditional split reporting

**`src/convert.rs`** (NO CHANGES):
- Convert module is untouched in this phase. D-01/D-02 (GIF routing priority over conversion) will matter when conversion is integrated in Phase 5, but Phase 4 does not touch conversion.

## Open Questions

1. **Sequence numbering with split directories**
   - What we know: `get_unique_output_path` uses a sequential index based on position in the full image list. When images are split between two directories, there will be numbering gaps in each directory.
   - What's unclear: Whether this is acceptable UX.
   - Recommendation: Accept as-is. The CONTEXT.md does not mention re-sequencing, and the index reflects the image's position in the source document, which is useful information. Changing this would require two-pass logic (count per-directory first, then assign indices).

2. **Lazy vs eager GIF output directory creation**
   - What we know: The existing pattern calls `create_dir_all(output_base_dir)` before the extraction loop in both processors. For `--gif-output`, we need to also create that directory.
   - What's unclear: Whether to create it eagerly (at processor entry) or lazily (on first GIF encountered).
   - Recommendation: Lazy creation (on first GIF). This avoids creating an empty GIF directory when a document has no GIFs. Use a `let mut gif_dir_created = false;` guard to ensure `create_dir_all` is called only once.

## Sources

### Primary (HIGH confidence)
- `src/main.rs` -- current codebase, Args struct with gif_only/gif_output fields
- `src/docx.rs` -- current DOCX processor, extraction loop structure
- `src/epub.rs` -- current EPUB processor, dual extraction paths
- `src/common.rs` -- current shared utilities, ImageToExtract pattern
- `.planning/phases/04-gif-extraction-and-routing/04-CONTEXT.md` -- locked decisions D-01 through D-11

### Secondary (HIGH confidence)
- `.planning/REQUIREMENTS.md` -- GIF-01, GIF-02, GIF-03 requirement definitions
- `.planning/PROJECT.md` -- "GIF separation is a routing concern" design decision
- `.planning/STATE.md` -- research gap about `--gif-output` + `--convert` interaction resolved by D-01/D-02

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing code verified by reading source
- Architecture: HIGH -- all integration points verified by reading exact source lines, patterns follow existing codebase conventions
- Pitfalls: HIGH -- identified from direct code analysis of dual extraction paths (EPUB), directory creation patterns, and count accumulation logic

**Research date:** 2026-04-02
**Valid until:** 2026-05-02 (stable -- no external dependency changes)
