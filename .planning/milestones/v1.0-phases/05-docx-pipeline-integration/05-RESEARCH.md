# Phase 5: DOCX Pipeline Integration - Research

**Researched:** 2026-04-02
**Domain:** Rust CLI integration -- threading image conversion and GIF routing through DOCX processor
**Confidence:** HIGH

## Summary

Phase 5 integrates the conversion module (`src/convert.rs`) and GIF routing logic into the DOCX extraction pipeline (`src/docx.rs`), closes the lossless WebP encoding gap, updates `ExtractionCounts` with conversion tracking fields, and implements conversion-aware summary reporting in `main.rs`. This is the first end-to-end verification that `--convert` actually works for users.

The codebase is well-structured for this integration. All building blocks exist: `try_convert()` returns `ConversionResult::Converted` or `Skipped`, `write_image_to_file()` accepts arbitrary bytes, `get_unique_output_path()` accepts an extension parameter, and `ExtractionCounts` is already in `common.rs` ready for field expansion. The DOCX processor's inner loop (lines 68-102 of `docx.rs`) already reads archive data into a `Vec<u8>` before writing -- conversion inserts naturally between the read and write steps.

**Primary recommendation:** Implement in three logical stages: (1) add lossless WebP encoding + `lossless` parameter to convert.rs, (2) thread conversion into docx.rs per-image loop with GIF routing priority, (3) update ExtractionCounts and main.rs summary reporting. All three are tightly coupled and should be a single plan.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** When `--convert` is active, the finish message shows: `"Extracted N image(s), converted M, skipped K from D document(s)"`.
- **D-02:** Conversion stats only appear when `--convert` is active. Without `--convert`, existing message format is unchanged.
- **D-03:** `ExtractionCounts` gains two new fields: `converted: usize` and `skipped: usize`.
- **D-04:** When both `--convert` and `--gif-output` are active, the finish message combines: `"Extracted N image(s), converted M, skipped K, routed G GIF(s) to /path from D document(s)"`.
- **D-05:** This phase adds lossless WebP encoding to convert.rs. `convert_image()` and `try_convert()` gain a `lossless: bool` parameter. A new `encode_webp_lossless()` function uses the `image` crate's built-in WebP encoder.
- **D-06:** WebP encoding branch routes between lossy and lossless based on `lossless` flag.
- **D-07:** The `lossless` parameter is ignored for non-WebP formats.
- **D-08:** Conversion parameters added as individual parameters to `docx::process_file()`: `convert: Option<OutputFormat>`, `quality: u8`, `lossless: bool`. Consistent with Phase 4's approach.
- **D-09:** The `process_file()` dispatch in `main.rs` threads new parameters through to both DOCX and EPUB processors. Full signature becomes 7 parameters for DOCX.
- **D-10:** GIF routing takes priority over conversion. When `--gif-output` is active, GIFs are written as-is to GIF output directory; non-GIF images are converted per `--convert`.
- **D-11:** Warning printing for skipped formats is the DOCX processor's responsibility. Uses `eprintln!("Warning: ...")` pattern.

### Claude's Discretion
- Internal logic flow in `docx::process_file()` for when to convert vs. route vs. skip
- Whether to refactor the per-image extraction loop or add conversion as a conditional step
- Test strategy for the integration (unit tests, integration tests, or both)
- How to handle the `lossless` flag in `try_convert()` call sites

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CONV-01 | User can convert all extracted images to a single target format via `--convert <jpg\|png\|webp>` | `try_convert()` API exists and returns `ConversionResult`; threading it into docx.rs inner loop provides end-to-end conversion. Lossless WebP encoding path needs adding to convert.rs. ExtractionCounts needs `converted`/`skipped` fields. Main.rs dispatch and summary reporting need updating. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `image` | 0.25.10 | Image decoding + lossless WebP encoding (`WebPEncoder::new_lossless`) | Already a dependency; provides `codecs::webp::WebPEncoder` for lossless path |
| `webp` | 0.3.x | Lossy WebP encoding (`Encoder::encode`) | Already a dependency; wraps libwebp for lossy quality control |
| `zip` | 2.4.2 | DOCX archive traversal | Already used by docx.rs |
| `anyhow` | 1.0.100 | Error handling with context | Already used throughout |

### Supporting
No new dependencies needed. All required crates are already in `Cargo.toml`.

**Installation:** No new packages -- all dependencies already present.

## Architecture Patterns

### Current Source File Layout (pre-phase)
```
src/
  main.rs     ~511 lines  Entry point, CLI, orchestration, progress, summary
  common.rs   ~269 lines  Shared types (ExtractionCounts, ImageToExtract), utilities
  convert.rs  ~677 lines  Conversion API: try_convert, convert_image, can_convert, encoders
  docx.rs     ~106 lines  DOCX ZIP traversal and extraction (main integration target)
  epub.rs     ~501 lines  EPUB processing (unchanged this phase, updated in Phase 6)
```

### Integration Pattern: Conversion in Per-Image Loop

The DOCX processor follows a two-pass approach:
1. **Collection pass** (lines 36-55): Scan archive entries, collect `ImageToExtract` structs with index + extension
2. **Extraction pass** (lines 68-102): For each collected image, read bytes from archive, determine output directory, write to disk

Conversion inserts into the extraction pass between "read bytes" and "write to disk":

```
Read archive bytes (existing)
  -> GIF routing check (existing -- is_gif && gif_output.is_some())
  -> Conversion check (NEW -- convert.is_some() && !routed_gif)
     -> try_convert() call
     -> Match on Converted: use new bytes + new extension
     -> Match on Skipped: warn + use original bytes + original extension
  -> Write to disk (existing)
```

### Pattern: Parameter Threading

The dispatch function in `main.rs` (line 292) already passes 7 params to `epub::process_file()`. The DOCX processor currently takes 4 params and grows to 7 with the addition of `convert: Option<OutputFormat>`, `quality: u8`, `lossless: bool`.

Current `docx::process_file()` signature:
```rust
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    gif_output: Option<&Path>,
) -> Result<ExtractionCounts>
```

New signature (per D-08):
```rust
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    gif_output: Option<&Path>,
    convert: Option<OutputFormat>,
    quality: u8,
    lossless: bool,
) -> Result<ExtractionCounts>
```

### Pattern: Lossless WebP Encoding via `image` Crate

The `image` crate's `WebPEncoder::new_lossless(w)` produces a lossless WebP. It implements `ImageEncoder` which provides `write_image()`. The usage pattern matches the existing `encode_png()` and `encode_jpeg()` helpers:

```rust
use image::codecs::webp::WebPEncoder;

fn encode_webp_lossless(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut buf);
    img.write_with_encoder(encoder)
        .context("Failed to encode lossless WebP")?;
    Ok(buf)
}
```

### Pattern: `convert_image()` with Lossless Flag

The `convert_image()` function's WebP branch currently calls `encode_webp_lossy()`. With the `lossless` parameter, it routes:

```rust
OutputFormat::Webp => {
    if lossless {
        encode_webp_lossless(&img)?
    } else {
        encode_webp_lossy(&img, quality)?
    }
}
```

The `lossless` flag is simply not read by the JPEG and PNG branches (D-07).

### Pattern: try_convert() Signature Update

`try_convert()` gains the `lossless` parameter and passes it through to `convert_image()`:

```rust
pub fn try_convert(
    data: &[u8],
    source_ext: &str,
    format: OutputFormat,
    quality: u8,
    lossless: bool,
) -> Result<ConversionResult>
```

### Pattern: ExtractionCounts Field Expansion

`ExtractionCounts` already derives `Debug, Default, Clone, Copy`. Adding two `usize` fields preserves all derive capabilities:

```rust
#[derive(Debug, Default, Clone, Copy)]
pub struct ExtractionCounts {
    pub extracted: usize,
    pub gifs_routed: usize,
    pub converted: usize,   // NEW
    pub skipped: usize,      // NEW
}
```

Accumulation in main.rs follows the existing `+=` pattern:
```rust
total_counts.converted += counts.converted;
total_counts.skipped += counts.skipped;
```

### Anti-Patterns to Avoid
- **Converting routed GIFs:** GIF routing takes priority (D-10). Never call `try_convert()` on a GIF that is being routed to `--gif-output`. The routing check MUST come before the conversion check.
- **Printing warnings from convert.rs:** Warning printing is the caller's responsibility (Phase 2 D-02). The `docx.rs` module prints warnings via `eprintln!("Warning: ...")`.
- **Cloning ConversionResult:** `ConversionResult::Converted` contains `Vec<u8>` which is expensive to clone. Use it by-value (move semantics).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Format conversion | Custom image codec wrappers | `try_convert()` from convert.rs | Already handles can_convert check, decode, encode, and extension resolution |
| Lossless WebP encoding | Manual pixel buffer manipulation | `image::codecs::webp::WebPEncoder::new_lossless()` | Built into the `image` crate, follows same pattern as PNG/JPEG encoders |
| Unique filename generation | Custom dedup logic | `get_unique_output_path()` from common.rs | Already handles collision avoidance with counter-based approach |
| Output format extension | Hardcoded strings | `OutputFormat::extension()` | Returns `&'static str`, already tested |

## Common Pitfalls

### Pitfall 1: GIF Routing vs. Conversion Ordering
**What goes wrong:** A GIF image gets converted (e.g., to PNG) when `--convert png --gif-output /gifs` is active, instead of being routed as-is to the GIF directory.
**Why it happens:** The conversion check runs before the GIF routing check, or the two checks are not mutually exclusive.
**How to avoid:** Check `is_gif && gif_output.is_some()` FIRST. Only attempt conversion when the image is NOT being routed as a GIF.
**Warning signs:** GIF files appearing in the main output directory instead of `--gif-output` when both flags are active.

### Pitfall 2: Conversion Error Aborting Batch
**What goes wrong:** A corrupt image causes `try_convert()` to return `Err`, and the `?` operator propagates it, aborting all remaining images in the DOCX.
**Why it happens:** Using `?` on the `try_convert()` result without catch-and-continue logic.
**How to avoid:** Catch conversion errors per-image, print a warning, and continue to the next image. Only fatal errors (archive read failure) should propagate.
**Warning signs:** A DOCX with one corrupt image and many valid images only producing output for images before the corrupt one.

### Pitfall 3: ExtractionCounts `extracted` Double-Counting
**What goes wrong:** Both `extracted` and `converted` are incremented, making `extracted` an inaccurate total.
**Why it happens:** Misunderstanding the semantics: `extracted` counts ALL images written to disk (converted or not), while `converted` counts only those that went through conversion.
**How to avoid:** Always increment `extracted` for every image written. Increment `converted` only when `ConversionResult::Converted` is matched. Increment `skipped` only when `ConversionResult::Skipped` is matched and conversion was attempted.
**Warning signs:** Sum of `converted + skipped` not equaling total attempted conversions.

### Pitfall 4: Existing Test Breakage from Signature Changes
**What goes wrong:** Changing `convert_image()` and `try_convert()` signatures breaks all existing tests in `convert.rs`.
**Why it happens:** The 63 existing tests call these functions without the new `lossless` parameter.
**How to avoid:** Update all existing call sites to pass `lossless: false` (preserving current behavior). The new parameter is additive.
**Warning signs:** `cargo test` failing immediately after signature change.

### Pitfall 5: Lossless Flag Passed to Non-WebP Formats
**What goes wrong:** No error, but potential confusion if `lossless: true` is passed with `OutputFormat::Jpg`.
**Why it happens:** The CLI validation in `validate_args()` prevents this at the user level, but internal code might pass `lossless: true` unconditionally.
**How to avoid:** Per D-07, the `lossless` parameter is simply ignored for non-WebP formats. The code should NOT error on this combination internally -- `validate_args()` handles user-facing validation. Internal callers just pass `args.lossless` through.

### Pitfall 6: Wrong Extension After Conversion
**What goes wrong:** A converted image gets the original extension (e.g., `document_1.bmp` instead of `document_1.png` after BMP->PNG conversion).
**Why it happens:** The code uses `image.extension` (original) instead of the extension from `ConversionResult::Converted`.
**How to avoid:** Match on `ConversionResult` and use the extension from the `Converted` variant for `get_unique_output_path()`. For `Skipped`, use the original extension.

## Code Examples

### Lossless WebP Encoder (new function in convert.rs)
```rust
// Source: image crate v0.25.10 docs (docs.rs/image/0.25/image/codecs/webp/struct.WebPEncoder.html)
use image::codecs::webp::WebPEncoder;

/// Encodes a DynamicImage as lossless WebP using the image crate's built-in encoder.
fn encode_webp_lossless(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut buf);
    img.write_with_encoder(encoder)
        .context("Failed to encode lossless WebP")?;
    Ok(buf)
}
```
**Confidence:** HIGH -- Verified `WebPEncoder::new_lossless` exists in image 0.25.10 via local cargo doc build and docs.rs.

### Conversion Decision Logic in docx.rs Inner Loop
```rust
// Pseudocode for the per-image extraction logic
let is_gif = image.extension == "gif";

// Step 1: GIF routing check (PRIORITY -- D-10)
let effective_output_dir = if let (true, Some(gif_dir)) = (is_gif, gif_output) {
    // ... create gif dir, route to gif_dir
    gif_dir
} else {
    output_base_dir
};

// Step 2: Read archive bytes
let mut data = Vec::new();
file.read_to_end(&mut data)?;

// Step 3: Conversion (only if requested AND not a routed GIF)
let (final_data, final_ext) = if let Some(format) = convert {
    if is_gif && gif_output.is_some() {
        // GIF is being routed -- skip conversion, use original
        (data, image.extension.clone())
    } else {
        match try_convert(&data, &image.extension, format, quality, lossless) {
            Ok(ConversionResult::Converted(converted_bytes, ext)) => {
                counts.converted += 1;
                (converted_bytes, ext)
            }
            Ok(ConversionResult::Skipped(original_ext)) => {
                eprintln!("Warning: Skipping conversion for {} ({} format not supported)",
                    doc_name, original_ext);
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

// Step 4: Write with correct extension
let output_path = get_unique_output_path(effective_output_dir, &doc_name, seq_index, total_images, &final_ext)?;
write_image_to_file(&output_path, &final_data)?;
counts.extracted += 1;
```
**Confidence:** HIGH -- Based on verified existing code patterns and locked CONTEXT.md decisions.

### Summary Message Logic in main.rs
```rust
// Conditional formatting based on --convert and --gif-output presence
if total_counts.extracted > 0 {
    let has_convert = args.convert.is_some();
    let has_gif_routing = total_counts.gifs_routed > 0;

    match (has_convert, has_gif_routing) {
        (true, true) => {
            // D-04: Combined message
            let gif_dir = args.gif_output.as_ref().unwrap();
            format!("Extracted {} {}, converted {}, skipped {}, routed {} GIF(s) to {} from {} document(s)",
                total_counts.extracted, item_name,
                total_counts.converted, total_counts.skipped,
                total_counts.gifs_routed, gif_dir.display(),
                total_documents)
        }
        (true, false) => {
            // D-01: Conversion stats only
            format!("Extracted {} {}, converted {}, skipped {} from {} document(s)",
                total_counts.extracted, item_name,
                total_counts.converted, total_counts.skipped,
                total_documents)
        }
        (false, true) => {
            // Existing GIF routing message (no conversion)
            // ... unchanged from current code
        }
        (false, false) => {
            // Existing default message (no conversion, no GIF routing)
            // ... unchanged from current code
        }
    }
}
```
**Confidence:** HIGH -- Directly from locked decisions D-01, D-02, D-04.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `convert_image()` with 3 params | `convert_image()` with 4 params (+ lossless) | This phase | All existing callers and tests need `lossless: false` added |
| `try_convert()` with 4 params | `try_convert()` with 5 params (+ lossless) | This phase | All existing callers and tests need `lossless: false` added |
| `ExtractionCounts` with 2 fields | `ExtractionCounts` with 4 fields | This phase | Default derive means new fields are 0 by default -- safe expansion |
| DOCX extraction writes raw bytes only | DOCX extraction optionally converts before writing | This phase | Core behavioral change for CONV-01 |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` with `cargo test` |
| Config file | None -- standard Rust test harness |
| Quick run command | `cargo test` |
| Full suite command | `cargo test` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONV-01a | DOCX with `--convert png` produces PNG files | integration | `cargo test docx_convert` | Wave 0 |
| CONV-01b | Mixed formats: supported converted, unsupported extracted raw with warning | unit | `cargo test convert_skip` | Wave 0 |
| CONV-01c | GIF routing works with conversion: `--gif-output` separates GIFs from converted files | unit | `cargo test gif_routing_with_convert` | Wave 0 |
| CONV-01d | Conversion error on single image does not abort batch | unit | `cargo test conversion_error_continues` | Wave 0 |
| CONV-01e | Lossless WebP encoding path works | unit | `cargo test encode_webp_lossless` | Wave 0 |
| CONV-01f | `convert_image()` and `try_convert()` accept lossless parameter | unit | `cargo test` (existing tests updated) | Existing -- needs parameter update |
| CONV-01g | ExtractionCounts tracks converted and skipped | unit | `cargo test extraction_counts` | Existing -- needs field update |
| CONV-01h | Summary message includes conversion stats when `--convert` active | unit | `cargo test summary_message` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/convert.rs` -- new tests for `encode_webp_lossless()`, `convert_image()` with `lossless: true`, `try_convert()` with `lossless` param
- [ ] `src/convert.rs` -- existing tests updated to pass `lossless: false` parameter
- [ ] `src/common.rs` -- test for `ExtractionCounts` with `converted` and `skipped` fields
- [ ] No integration test infrastructure for end-to-end DOCX processing (would require test DOCX fixture files -- may be manual-only)

**Note:** True end-to-end integration testing (creating a DOCX, extracting, converting) requires fixture files. The per-image conversion logic can be unit tested by verifying the decision flow. The actual `docx::process_file()` function requires real DOCX files which are not present in the test suite. Testing strategy should focus on:
1. Unit tests for convert.rs API changes (lossless WebP, parameter threading)
2. Unit tests for ExtractionCounts field expansion
3. Unit tests for summary message formatting logic
4. Manual verification with real DOCX files for end-to-end validation

## Project Constraints (from CLAUDE.md)

- **Rust edition 2024** -- uses `let` chains, requires Rust >= 1.85
- **`image` crate** for conversion with selective feature flags (already configured)
- **`webp` crate** for lossy WebP (already configured)
- **Error handling:** All public functions return `anyhow::Result<T>`, use `.context()` for error messages
- **Warning pattern:** `eprintln!("Warning: ...")` for non-fatal issues
- **No `#[allow(...)]` annotations** -- all clippy warnings must be resolved
- **Run `cargo fmt` before committing**
- **Run `cargo clippy` to check**
- **Function design:** `&Path` for paths, `&HashSet<&str>` for extensions, `Option<T>` for optional values
- **Doc comments:** `///` on all public functions, `//!` module-level comments
- **No output to `nul`** (Windows-specific constraint from global CLAUDE.md)
- **When writing tests, always verify the APIs of what is going to be tested**

## Open Questions

1. **Integration test strategy for DOCX processing**
   - What we know: No test DOCX fixture files exist in the repo. The `docx::process_file()` function requires real DOCX files.
   - What's unclear: Whether creating minimal test DOCX files programmatically (via `zip` crate) is worth the effort for this phase.
   - Recommendation: Focus on unit tests for the conversion logic and parameter threading. Manual verification with real DOCX files covers the integration gap. Phase 6 (EPUB) will face the same question.

2. **Conversion error semantics: Err vs Skipped**
   - What we know: `try_convert()` returns `Err` for corrupt data and `Skipped` for unsupported formats. Both should be non-fatal in the DOCX loop.
   - What's unclear: Whether corrupt-data errors should increment `skipped` (as shown in code example) or a separate counter.
   - Recommendation: Increment `skipped` for both `Skipped` and `Err` cases, since both result in the original image being written. This keeps the summary math simple: `converted + skipped = total conversion attempts`.

## Sources

### Primary (HIGH confidence)
- `src/convert.rs` -- Current `try_convert()`, `convert_image()`, `ConversionResult` API verified by reading source
- `src/docx.rs` -- Current extraction loop verified by reading source (lines 68-102)
- `src/common.rs` -- Current `ExtractionCounts` struct verified by reading source (lines 96-102)
- `src/main.rs` -- Current dispatch (lines 292-321) and summary (lines 476-508) verified by reading source
- `image` crate 0.25.10 -- `WebPEncoder::new_lossless` verified via local `cargo doc` build
- `webp` crate 0.3.x -- `Encoder::encode_lossless()` confirmed via docs.rs (not used per D-05, but available as fallback)
- Phase CONTEXT.md files (01, 02, 04, 05) -- All design decisions verified by reading files

### Secondary (MEDIUM confidence)
- docs.rs/image/0.25/image/codecs/webp/struct.WebPEncoder.html -- API docs for lossless WebP encoder

### Tertiary (LOW confidence)
- None -- all findings verified with primary sources.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies already in Cargo.toml, no new crates needed
- Architecture: HIGH -- all integration points read and verified from source code
- Pitfalls: HIGH -- derived from existing code patterns and locked decisions
- Lossless WebP: HIGH -- `WebPEncoder::new_lossless` verified in locally built docs for image 0.25.10

**Research date:** 2026-04-02
**Valid until:** 2026-05-02 (stable -- no fast-moving dependencies)
