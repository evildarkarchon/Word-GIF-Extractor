# Phase 6: EPUB Pipeline Integration - Research

**Researched:** 2026-04-02
**Domain:** Rust refactoring -- threading conversion parameters through EPUB processor + config struct consolidation
**Confidence:** HIGH

## Summary

This phase is a code-level integration task with no new dependencies. The entire scope involves (1) creating an `ExtractionConfig` struct in `common.rs` to consolidate four conversion-related parameters, (2) refactoring `docx::process_file()` and `epub::process_file()` to accept `&ExtractionConfig`, (3) threading conversion logic into the EPUB processor's two extraction helpers (`extract_all_images` and `extract_cover_only`), and (4) updating the `main.rs` dispatch function to construct `ExtractionConfig` and remove `#[allow(clippy::too_many_arguments)]`.

The DOCX processor (`src/docx.rs`, lines 94-128) serves as the reference implementation. The conversion pattern is well-established: check `is_routed_gif` -> call `try_convert()` -> match on `ConversionResult` -> write bytes. The EPUB processor needs this same pattern inserted in two places (`extract_all_images` inner loop and `extract_cover_only` branches), with one behavioral difference: cover-only mode skips entirely on conversion failure (no raw fallback) per D-04.

**Primary recommendation:** Follow the DOCX pattern exactly for `extract_all_images()`, apply the stricter skip-on-failure variant for `extract_cover_only()`, and use a config struct to reduce parameter count across both processors.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Create `ExtractionConfig<'a>` struct in `src/common.rs` bundling: `convert: Option<OutputFormat>`, `quality: u8`, `lossless: bool`, `gif_output: Option<&'a Path>`. Follows the `ExtractionCounts` pattern already established in common.rs.
- **D-02:** Both `docx::process_file()` and `epub::process_file()` accept `&ExtractionConfig` instead of individual `convert`, `quality`, `lossless`, `gif_output` parameters. This removes the need for `#[allow(clippy::too_many_arguments)]` on the main dispatch function.
- **D-03:** The `main.rs` dispatch function (`process_file()`) constructs `ExtractionConfig` once and passes it to both processors. EPUB-specific params (`cover_only`, `cover_fallback`, `filter`) remain as individual parameters on `epub::process_file()` since they don't apply to DOCX.
- **D-04:** When cover conversion fails (corrupt image, decode error, or unsupported format skip), the cover is **skipped entirely** -- no file is written. This is stricter than the DOCX batch pattern.
- **D-05:** The "skip entirely on failure" rule applies **only to cover-only mode** (`-c`). Regular all-images extraction (`extract_all_images`) uses the DOCX pattern: warn to stderr, write raw bytes with original extension, increment `skipped` count.
- **D-06:** Cover-fallback (`--cover-fallback`) works normally with conversion: if no cover is found, fall back to `extract_all_images()` which uses the standard conversion pattern.
- **D-07:** `ExtractionConfig` is passed through from `process_file()` to both internal helpers: `extract_all_images()` and `extract_cover_only()`. Conversion logic lives inside the per-image loop in each helper, matching the DOCX pattern.
- **D-08:** Internal helpers replace their individual `gif_output` parameter with `&ExtractionConfig`, since `gif_output` is now part of the struct.
- **D-09:** The MIME-derived extension from `mime_to_extension()` is used as `source_ext` for `try_convert()`.
- **D-10:** For cover images with unrecognized MIME types, the existing "jpg" fallback extension is used for conversion.
- **D-11:** GIF routing takes priority over conversion (Phase 4 D-01/D-02).
- **D-12:** Warning printing uses `eprintln!("Warning: ...")` pattern (Phase 5 D-11).
- **D-13:** Summary reporting in `main.rs` already handles all conversion + GIF routing message combinations (Phase 5 D-01/D-04). No changes needed to main.rs finish messages.

### Claude's Discretion
- Internal logic flow in `extract_all_images()` and `extract_cover_only()` for the conversion decision path
- Whether to extract the conversion logic into a shared helper function used by both EPUB extraction paths
- Test strategy (unit tests for conversion wiring, integration-level tests, or both)
- How to update existing `docx.rs` tests to work with `ExtractionConfig`

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CONV-01 (EPUB extension) | User can convert all extracted images to a single target format via `--convert <jpg\|png\|webp>` -- extends to EPUB | ExtractionConfig struct consolidates parameters; epub.rs gets conversion logic matching docx.rs pattern |
| (Success 1) | `word-image-extractor book.epub --convert webp` extracts + converts all images | `extract_all_images()` gets try_convert() call in inner loop |
| (Success 2) | Cover-only + `--convert png` extracts and converts only the cover | `extract_cover_only()` gets conversion with skip-on-failure |
| (Success 3) | GIF routing works for EPUB extraction | `is_routed_gif` check before conversion, matching docx.rs |
| (Success 4) | All EPUB extraction modes work with conversion | Config struct threaded through all code paths |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `image` | 0.25.10 (resolved) | Image decoding and encoding for conversion | Already in Cargo.toml; no changes needed |
| `webp` | 0.3.1 (resolved) | Lossy WebP encoding | Already in Cargo.toml; no changes needed |
| `epub` | 2.1.5 (resolved) | EPUB document parsing and resource access | Already in Cargo.toml; no changes needed |

### Supporting
No new dependencies required. This phase is purely a refactoring and integration task.

**Installation:** No new packages to install.

## Architecture Patterns

### Recommended Project Structure
```
src/
    main.rs       # ExtractionConfig construction + dispatch (refactored)
    common.rs     # ExtractionConfig struct added here
    convert.rs    # No changes (stable API)
    docx.rs       # Refactored to accept &ExtractionConfig
    epub.rs       # Conversion logic added + &ExtractionConfig
```

### Pattern 1: ExtractionConfig Struct
**What:** A lifetime-parameterized struct bundling conversion-related parameters.
**When to use:** Pass through the dispatch chain from main.rs to format processors.
**Example:**
```rust
// In src/common.rs, following ExtractionCounts pattern
/// Configuration for image extraction and conversion behavior.
#[derive(Debug, Clone, Copy)]
pub struct ExtractionConfig<'a> {
    /// Target format for conversion (None = extract as-is)
    pub convert: Option<OutputFormat>,
    /// JPEG/WebP encoding quality (1-100)
    pub quality: u8,
    /// Use lossless WebP encoding
    pub lossless: bool,
    /// Separate output directory for GIF files
    pub gif_output: Option<&'a Path>,
}
```
**Note on derives:** `Clone` and `Copy` are possible because `Option<&'a Path>` is `Copy`, `Option<OutputFormat>` is `Copy` (OutputFormat derives Copy), `u8` is Copy, and `bool` is Copy. This matches the `ExtractionCounts` derive pattern. The struct holds only references and primitives.

### Pattern 2: DOCX Conversion Inner Loop (Reference)
**What:** The per-image conversion decision pattern established in Phase 5.
**When to use:** Replicate in epub.rs `extract_all_images()` for non-cover EPUB images.
**Example (from docx.rs lines 94-128):**
```rust
// Determine if this GIF is being routed (GIF routing takes priority)
let is_routed_gif = is_gif && config.gif_output.is_some();

// Attempt conversion if requested and not a routed GIF
let (final_data, final_ext) = if let Some(format) = config.convert {
    if is_routed_gif {
        (data, image.extension.clone())
    } else {
        match try_convert(&data, &image.extension, format, config.quality, config.lossless) {
            Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                counts.converted += 1;
                (converted_bytes, ext)
            }
            Ok(ConversionResult::Skipped(original_ext)) => {
                eprintln!("Warning: Skipping conversion for {} ...", doc_name);
                counts.skipped += 1;
                (data, original_ext)
            }
            Err(e) => {
                eprintln!("Warning: Conversion failed for image in {}: {}", doc_name, e);
                counts.skipped += 1;
                (data, image.extension.clone())
            }
        }
    }
} else {
    (data, image.extension.clone())
};
```

### Pattern 3: Cover-Only Skip-on-Failure
**What:** Stricter conversion behavior for cover-only mode -- skip the cover entirely if conversion fails.
**When to use:** In `extract_cover_only()` when processing the single cover image.
**Example:**
```rust
// In extract_cover_only(), after getting cover data:
let is_routed_gif = extension == "gif" && config.gif_output.is_some();

let (final_data, final_ext) = if let Some(format) = config.convert {
    if is_routed_gif {
        (data, extension.clone())
    } else {
        match try_convert(&data, &extension, format, config.quality, config.lossless) {
            Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                // counts.converted += 1; -- set on ExtractionCounts returned
                (converted_bytes, ext)
            }
            Ok(ConversionResult::Skipped(original_ext)) => {
                eprintln!("Warning: Cover image format '{}' not supported for conversion, skipping.", original_ext);
                return Ok(ExtractionCounts::default()); // D-04: skip entirely
            }
            Err(e) => {
                eprintln!("Warning: Cover conversion failed: {}", e);
                return Ok(ExtractionCounts::default()); // D-04: skip entirely
            }
        }
    }
} else {
    (data, extension)
};
```

### Anti-Patterns to Avoid
- **Duplicating the full conversion block without considering a helper:** Both `extract_all_images()` and `extract_cover_only()` need conversion logic. Consider whether a shared helper reduces duplication vs. complicates the different error-handling behavior (Claude's discretion).
- **Forgetting to update the `extension` variable after conversion:** The extension from `ConversionResult::Converted` must flow into `get_unique_output_path()` -- not the original MIME-derived extension.
- **Modifying convert.rs:** The conversion API is stable. All changes are in epub.rs, docx.rs, common.rs, and main.rs.
- **Adding `#[allow(clippy::too_many_arguments)]` to new functions:** The whole point of `ExtractionConfig` is to remove this. The main.rs dispatch should drop from 10 params to 7 after the refactor.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Conversion decision logic | New conversion API | `try_convert()` from `convert.rs` | Already handles can_convert check, decode, encode, result typing |
| File writing | Custom write logic | `write_image_to_file()` from `common.rs` | Handles BufWriter, flush, error context |
| Output path generation | Manual filename construction | `get_unique_output_path()` from `common.rs` | Handles seq numbering, collision avoidance, extension |
| GIF routing check | New routing logic | `is_gif && config.gif_output.is_some()` pattern | Established in both docx.rs and epub.rs already |

**Key insight:** This phase adds zero new algorithms. It threads existing APIs through EPUB code paths and consolidates parameters into a struct.

## Common Pitfalls

### Pitfall 1: Lifetime Mismatch on ExtractionConfig
**What goes wrong:** `ExtractionConfig<'a>` holds `Option<&'a Path>` for `gif_output`. If the struct is constructed in a scope that doesn't outlive the callee, the borrow checker rejects it.
**Why it happens:** The struct borrows from `args.gif_output` which lives for the duration of `main()`. As long as `ExtractionConfig` is constructed in `main()` and passed by reference, this is fine.
**How to avoid:** Construct `ExtractionConfig` in `main()` before the processing loop, using `args.gif_output.as_deref()` for the path reference. Pass `&config` to `process_file()`.
**Warning signs:** Compiler error about lifetime not living long enough.

### Pitfall 2: Forgetting to Thread Config to Cover-Fallback Path
**What goes wrong:** `extract_cover_only()` calls `extract_all_images()` in the cover-fallback path (line 407). If the config parameter isn't threaded through this call, conversion won't work when cover-fallback activates.
**Why it happens:** The fallback path is easy to miss -- it's deep in a nested match arm.
**How to avoid:** Ensure the `config` parameter replaces `gif_output` in the fallback call to `extract_all_images()`.
**Warning signs:** Compilation error (the function signature requires it), but worth noting for test coverage.

### Pitfall 3: Cover-Only Counts Not Including converted/skipped
**What goes wrong:** The returned `ExtractionCounts` from `extract_cover_only()` currently uses a manual struct literal (not `ExtractionCounts::default()`). When conversion succeeds, the `converted` field must be set to 1.
**Why it happens:** The current code creates `ExtractionCounts { extracted: 1, gifs_routed: 0, converted: 0, skipped: 0 }` manually.
**How to avoid:** Update the struct literal to include the correct `converted` count when conversion succeeds.
**Warning signs:** Conversion summary always shows 0 converted for cover-only EPUB runs.

### Pitfall 4: MIME Extension Compatibility with can_convert()
**What goes wrong:** If `mime_to_extension()` returns an extension string that `can_convert()` doesn't recognize, conversion is incorrectly skipped.
**Why it happens:** The two functions are in different modules and could drift.
**How to avoid:** Verify alignment. Current state: `mime_to_extension()` returns "jpg", "png", "gif", "bmp", "webp", "tiff", "ico", "svg", "emf", "wmf". `can_convert()` accepts "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "webp", "ico". The sets are compatible -- `mime_to_extension()` returns "tiff" (not "tif") and "jpg" (not "jpeg"), both of which `can_convert()` recognizes. SVG/EMF/WMF are correctly not in `can_convert()`.
**Warning signs:** Tests passing for DOCX but failing for EPUB conversion of the same format.

### Pitfall 5: Duplicate Cover Branches in extract_cover_only()
**What goes wrong:** `extract_cover_only()` has two nearly-identical branches (metadata cover at line 320 and filename fallback at line 366). Conversion logic must be added to BOTH branches with identical behavior.
**Why it happens:** The function has structural duplication that predates this phase.
**How to avoid:** Consider whether to extract a shared helper for the "we have cover data, now convert and write" path. Either way, both branches must get the same conversion treatment.
**Warning signs:** One cover detection method converts, the other doesn't.

## Code Examples

### ExtractionConfig Construction in main.rs
```rust
// In main() before the processing loop
let config = ExtractionConfig {
    convert: args.convert,
    quality,
    lossless: args.lossless,
    gif_output: args.gif_output.as_deref(),
};
```

### Updated main.rs Dispatch Signature
```rust
// Remove #[allow(clippy::too_many_arguments)]
fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    epub_filter: &EpubFilter,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    match get_document_type(input_path) {
        Some(DocumentType::Docx) => docx::process_file(
            input_path,
            output_base_dir,
            allowed_extensions,
            config,
        ),
        Some(DocumentType::Epub) => epub::process_file(
            input_path,
            output_base_dir,
            allowed_extensions,
            cover_only,
            cover_fallback,
            epub_filter,
            config,
        ),
        None => { /* unchanged */ }
    }
}
```

### Updated docx::process_file Signature
```rust
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    // Body: replace gif_output -> config.gif_output,
    //       convert -> config.convert,
    //       quality -> config.quality,
    //       lossless -> config.lossless
}
```

### Updated epub::process_file Signature
```rust
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    filter: &EpubFilter,
    config: &ExtractionConfig,
) -> Result<ExtractionCounts> {
    // EPUB-specific params stay as individuals per D-03
}
```

### EPUB extract_all_images Conversion Insertion Point
```rust
// In extract_all_images(), after reading resource data (current lines 240-243):
let (data, _mime) = doc.get_resource(&image.id)
    .ok_or_else(|| anyhow::anyhow!("Failed to get resource '{}'", image.id))?;

// GIF routing check (existing, unchanged)
let is_gif = image.extension == "gif";
let effective_output_dir = if let (true, Some(gif_dir)) = (is_gif, config.gif_output) {
    // ... existing GIF routing logic
};

// NEW: Conversion decision (mirrors docx.rs pattern)
let is_routed_gif = is_gif && config.gif_output.is_some();
let (final_data, final_ext) = if let Some(format) = config.convert {
    if is_routed_gif {
        (data, image.extension.clone())
    } else {
        match try_convert(&data, &image.extension, format, config.quality, config.lossless) {
            Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                counts.converted += 1;
                (converted_bytes, ext)
            }
            Ok(ConversionResult::Skipped(original_ext)) => {
                eprintln!("Warning: Skipping conversion for {} ({} format not supported for conversion)",
                    base_name, original_ext);
                counts.skipped += 1;
                (data, original_ext)
            }
            Err(e) => {
                eprintln!("Warning: Conversion failed for image in {}: {}", base_name, e);
                counts.skipped += 1;
                (data, image.extension.clone())
            }
        }
    }
} else {
    (data, image.extension.clone())
};

let output_path = get_unique_output_path(effective_output_dir, base_name, seq_index, total_images, &final_ext)?;
write_image_to_file(&output_path, &final_data)?;
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Individual params (gif_output, convert, quality, lossless) | `ExtractionConfig` struct | This phase | Reduces main.rs dispatch from 10 params to 7 |
| DOCX-only conversion | Both DOCX and EPUB conversion | This phase | CONV-01 fully delivered |
| `#[allow(clippy::too_many_arguments)]` on dispatch | Clean clippy (no allows) | This phase | Removes the only `#[allow]` annotation in codebase |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Built-in `#[cfg(test)]` / `cargo test` (Rust 1.94) |
| Config file | None -- uses standard Cargo test harness |
| Quick run command | `cargo test` |
| Full suite command | `cargo test` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ExtractionConfig | Struct construction and field access | unit | `cargo test common::tests::test_extraction_config` | Wave 0 |
| DOCX refactor | docx::process_file accepts ExtractionConfig (no regression) | unit | `cargo test` (existing tests adapted) | Existing (needs update) |
| EPUB all-images conversion | extract_all_images applies try_convert in inner loop | unit | `cargo test epub::tests::test_*` | Wave 0 |
| EPUB cover conversion (skip on fail) | extract_cover_only skips entirely on conversion failure | unit | `cargo test epub::tests::test_*` | Wave 0 |
| GIF routing + conversion | GIFs routed as-is even with --convert active | unit | `cargo test epub::tests::test_*` | Wave 0 |
| main.rs dispatch | process_file uses ExtractionConfig, no #[allow] | compilation | `cargo clippy` | Existing (adapted) |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test && cargo clippy`
- **Phase gate:** Full suite green + clippy clean before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Existing docx.rs tests need no changes because they do not call `process_file()` directly -- there are no tests in docx.rs that test the `process_file` signature. The current docx.rs tests are all in convert.rs which is unchanged.
- [ ] Unit tests for `ExtractionConfig` construction (verify fields, Debug derive)
- [ ] Verify `cargo clippy` is clean without `#[allow(clippy::too_many_arguments)]` after refactor
- [ ] Existing epub.rs tests are for `format_epub_base_name` and `mime_to_extension` -- they don't touch `process_file()` and require no changes
- [ ] New EPUB conversion tests would require real EPUB fixtures -- consider testing the conversion logic pattern at the `try_convert` level instead (already well-tested in convert.rs)

## Open Questions

1. **Shared helper for conversion logic in epub.rs**
   - What we know: `extract_all_images()` and `extract_cover_only()` need similar but not identical conversion logic (different error handling)
   - What's unclear: Whether the behavioral difference (warn+write-raw vs. skip-entirely) makes a shared helper awkward
   - Recommendation: Claude's discretion. A helper could take a callback or enum for the error behavior, but the code duplication is only ~15 lines in each path. Inline may be clearer.

2. **Testing EPUB conversion end-to-end**
   - What we know: No EPUB test fixtures exist in the repo. The conversion logic (`try_convert`) is thoroughly tested in convert.rs.
   - What's unclear: Whether to create test EPUBs or test at the unit level
   - Recommendation: Test the wiring (correct parameters passed to try_convert, correct counts returned) via unit tests that exercise the conversion decision logic. The actual conversion correctness is already validated by convert.rs tests.

## Sources

### Primary (HIGH confidence)
- `src/docx.rs` -- Reference implementation of conversion inner loop (lines 94-128)
- `src/epub.rs` -- Current EPUB processor, all integration targets identified with line numbers
- `src/common.rs` -- ExtractionCounts pattern (line 96), ExtractionConfig will follow same pattern
- `src/convert.rs` -- Stable conversion API: try_convert, ConversionResult, OutputFormat
- `src/main.rs` -- Dispatch function (line 297), summary reporting (lines 496-560)
- `.planning/phases/06-epub-pipeline-integration/06-CONTEXT.md` -- All 13 locked decisions
- `.planning/phases/05-docx-pipeline-integration/05-CONTEXT.md` -- Reference implementation decisions

### Secondary (MEDIUM confidence)
- None needed -- this is a codebase-internal refactoring task

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all crates already in Cargo.toml with known versions
- Architecture: HIGH - config struct pattern is established (ExtractionCounts), reference implementation exists (docx.rs)
- Pitfalls: HIGH - identified from direct code inspection, all integration points verified with line numbers

**Research date:** 2026-04-02
**Valid until:** Indefinite (codebase-internal, no external API changes)
