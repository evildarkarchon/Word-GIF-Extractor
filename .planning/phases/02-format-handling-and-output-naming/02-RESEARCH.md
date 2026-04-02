# Phase 2: Format Handling and Output Naming - Research

**Researched:** 2026-04-02
**Domain:** Rust enum design, image format detection edge cases, conversion result API
**Confidence:** HIGH

## Summary

Phase 2 adds three items to the existing `src/convert.rs` module: a `ConversionResult` enum, a `try_convert()` convenience function, and an `extension()` method on `OutputFormat`. These are pure Rust constructs with no new dependencies -- all building blocks (`can_convert`, `convert_image`, `OutputFormat`) were established in Phase 1 and are verified working with 30 passing tests.

The scope is narrow and well-defined: wrap existing conversion primitives into a higher-level API that callers (Phases 5-6) will use. No filesystem I/O, no CLI changes, no modifications to other modules. The primary technical risk is getting the `ConversionResult` ergonomics right so callers can cleanly handle both converted and skipped cases in a single match expression.

**Primary recommendation:** Implement `ConversionResult` as a two-variant enum with `Converted(Vec<u8>, String)` and `Skipped(String)`, add `OutputFormat::extension()` returning `&'static str`, and compose `try_convert()` from `can_convert()` + `convert_image()` with clear error-vs-skip semantics.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Warning message format: `"Warning: Skipping conversion for {filename} ({FORMAT} format not supported for conversion)"`. Matches existing `eprintln!("Warning: ...")` pattern.
- **D-02:** Warning printing is the caller's responsibility (not convert.rs). The convert module returns data; callers decide when/how to print warnings.
- **D-03:** No changes to `get_unique_output_path()`. Callers pass the correct extension based on whether conversion happened.
- **D-04:** `OutputFormat` gets an `extension()` method returning the target file extension string (`"jpg"`, `"png"`, `"webp"`).
- **D-05:** A new `ConversionResult` enum with two variants: `Converted(Vec<u8>, String)` (converted bytes + target extension) and `Skipped(String)` (original extension preserved).
- **D-06:** A new `try_convert(data, source_ext, format, quality) -> Result<ConversionResult>` function that bundles `can_convert()` check + `convert_image()` call + extension resolution into one call.
- **D-07:** `try_convert()` handles the `Ok(None)` return from `convert_image()` (unsupported format detected at decode time) by returning `Skipped`. Callers never need to deal with `Option`.

### Claude's Discretion
- Internal implementation details of `try_convert()` (how it composes `can_convert` and `convert_image`)
- Whether `ConversionResult` should derive additional traits beyond `Debug`
- Test strategy for the new API surface

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CONV-03 | Unsupported source formats (SVG, WMF, EMF) are skipped during conversion with a warning to stderr, and extracted as raw bytes with their original extension | `try_convert()` returns `Skipped(original_ext)` for unsupported formats; `can_convert()` already returns `false` for svg/wmf/emf; `convert_image()` returns `Ok(None)` for unrecognized magic bytes. Both paths converge to `Skipped`. |
| CONV-04 | Converted output strips the original file extension and uses only the target format extension | `Converted` variant carries the target extension from `OutputFormat::extension()`; callers pass this extension to `get_unique_output_path()` which builds `{base}_{n}.{ext}` -- the original extension is never included. |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Rust edition 2024** (requires Rust >= 1.85) -- verified: rustc 1.94.0 installed
- **`image` crate** for conversion -- already added in Phase 1 (v0.25.10 resolved)
- **Conventions**: `snake_case` functions, `PascalCase` enums, `///` doc comments on all public items, `anyhow::Result<T>` for fallible functions
- **Error handling**: `Ok(None)` from `convert_image()` signals unsupported format; `Err` signals corrupt data
- **No `#[allow(...)]`** in production code (except the temporary `dead_code` on `mod convert` from Phase 1)
- **Tests**: `#[cfg(test)] mod tests {}` at bottom of file, standard Rust test harness
- **`cargo fmt`** before committing, **`cargo clippy`** must pass clean

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `image` | 0.25.10 | Image decode/encode (already added Phase 1) | No new deps needed |
| `webp` | 0.3.1 | Lossy WebP encoding (already added Phase 1) | No new deps needed |
| `anyhow` | 1.0.100 | Error handling with Result<T> | Already in codebase |

### Supporting
No new dependencies required for Phase 2. All work is pure Rust code additions to `src/convert.rs`.

### Alternatives Considered
None -- all decisions are locked. No dependency choices to make.

**Installation:** No new dependencies. Phase 1 already added all required crates.

## Architecture Patterns

### Where New Code Goes

All additions go in `src/convert.rs` alongside existing code:

```
src/convert.rs
  // Existing (Phase 1):
  //   OutputFormat enum (Jpg, Png, Webp)         -- add extension() method here
  //   can_convert(extension) -> bool
  //   convert_image(data, format, quality) -> Result<Option<Vec<u8>>>
  //   composite_on_white (private)
  //   encode_jpeg, encode_png, encode_webp_lossy (private)
  //   mod tests
  //
  // New (Phase 2):
  //   ConversionResult enum                       -- add after OutputFormat
  //   OutputFormat::extension() impl              -- add impl block after enum
  //   try_convert() function                      -- add after convert_image()
  //   new tests                                   -- add to existing mod tests
```

### Pattern 1: Result Enum for Conversion Outcome

**What:** `ConversionResult` is a two-variant enum that replaces the `Option<Vec<u8>>` pattern from `convert_image()` with named semantics.

**When to use:** When callers need to branch on "was this image converted or skipped?" and need different data for each path.

**Example:**
```rust
/// Result of attempting to convert an image to a target format.
///
/// Used by callers to determine whether to write converted bytes or
/// extract the original image as-is. The extension string in each
/// variant is ready to pass directly to `get_unique_output_path()`.
#[derive(Debug)]
pub enum ConversionResult {
    /// Image was successfully converted. Contains the converted bytes
    /// and the target format extension (e.g., "png", "jpg", "webp").
    Converted(Vec<u8>, String),
    /// Image was skipped (unsupported source format). Contains the
    /// original file extension to preserve in the output filename.
    Skipped(String),
}
```

**Why this shape:**
- `Converted(Vec<u8>, String)` -- caller writes the `Vec<u8>` using the `String` extension
- `Skipped(String)` -- caller writes original raw bytes using the `String` (original) extension
- Both variants carry the extension the caller needs for `get_unique_output_path()` -- no secondary lookups

### Pattern 2: Extension Method on OutputFormat

**What:** A method on the existing `OutputFormat` enum that returns the canonical file extension string.

**Example:**
```rust
impl OutputFormat {
    /// Returns the file extension for this output format.
    ///
    /// The returned string does not include a leading dot.
    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Jpg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }
}
```

**Why `&'static str`:** The extension strings are compile-time constants. Returning `&'static str` avoids allocation and matches how extensions are used throughout the codebase (e.g., `can_convert` takes `&str`, `get_unique_output_path` takes `&str`). The `Converted` variant stores a `String` (owned copy) because the enum needs to own its data, but the method itself is zero-cost.

### Pattern 3: try_convert Composition

**What:** A convenience function that composes `can_convert()` and `convert_image()` into a single call with clear semantics.

**Example:**
```rust
/// Attempts to convert image data to the target format.
///
/// This is the primary conversion API for callers. It checks whether the
/// source format is convertible, attempts conversion, and returns a
/// `ConversionResult` indicating whether the image was converted or skipped.
///
/// Returns `Skipped` when:
/// - `can_convert()` returns false for the source extension (pre-check)
/// - `convert_image()` returns `Ok(None)` (unsupported at decode time)
///
/// Returns `Err` when:
/// - The image data is corrupt or undecodable
pub fn try_convert(
    data: &[u8],
    source_ext: &str,
    format: OutputFormat,
    quality: u8,
) -> Result<ConversionResult> {
    // Fast path: extension not in decodable set
    if !can_convert(source_ext) {
        return Ok(ConversionResult::Skipped(source_ext.to_lowercase()));
    }

    // Attempt conversion
    match convert_image(data, format, quality)? {
        Some(converted_bytes) => {
            Ok(ConversionResult::Converted(
                converted_bytes,
                format.extension().to_string(),
            ))
        }
        None => {
            // Unsupported format detected at decode time (magic bytes don't match)
            Ok(ConversionResult::Skipped(source_ext.to_lowercase()))
        }
    }
}
```

**Key design points:**
- Two skip paths converge to the same `Skipped` variant (D-07): `can_convert()` pre-check and `convert_image()` returning `Ok(None)`
- The `?` on `convert_image()` propagates `Err` for corrupt data -- callers handle this at the processing loop level
- `source_ext.to_lowercase()` normalizes the extension in the `Skipped` variant for consistent output naming
- No warnings printed here -- D-02 says callers own warning output

### Anti-Patterns to Avoid
- **Printing warnings inside convert.rs:** D-02 explicitly states warnings are the caller's responsibility. The convert module is pure data transformation.
- **Returning `Option` from `try_convert`:** The whole point of `ConversionResult` is to eliminate the `Option` that callers found awkward in `convert_image()`.
- **Modifying `get_unique_output_path`:** D-03 locks this function as unchanged. Callers pass the right extension.
- **Adding filesystem I/O to convert.rs:** The module is byte-in, byte-out. File writing stays in `common.rs`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Format detection | Custom magic-byte parser | `can_convert()` + `image::load_from_memory()` | Already built in Phase 1, handles all edge cases |
| Extension mapping | Lookup table or HashMap | `OutputFormat::extension()` method | Three variants, exhaustive match, compile-time verified |
| Error-vs-skip disambiguation | Custom error types | Two-path design: `can_convert` pre-check + `Ok(None)` from `convert_image` | Already designed and tested in Phase 1 |

**Key insight:** Phase 2 is strictly a composition layer. Every building block already exists and is tested. The risk is in getting the composition API ergonomics right, not in the underlying logic.

## Common Pitfalls

### Pitfall 1: Extension Case Normalization
**What goes wrong:** `source_ext` arrives as "SVG" or "Svg" from archive entries; if `Skipped` preserves the original casing, downstream naming produces mixed-case extensions like `document_1.SVG`.
**Why it happens:** Archive entries use whatever case the authoring tool chose.
**How to avoid:** Always lowercase the extension in the `Skipped` variant: `source_ext.to_lowercase()`. The `can_convert()` function already lowercases internally, so the pre-check is safe regardless.
**Warning signs:** Test with uppercase extensions and verify output extension is lowercase.

### Pitfall 2: Forgetting the Ok(None) Path
**What goes wrong:** `try_convert()` only checks `can_convert()` and assumes conversion always succeeds. But `convert_image()` can return `Ok(None)` for data whose magic bytes don't match a supported format even though the file extension suggested otherwise (e.g., a file named `.png` that is actually an SVG).
**Why it happens:** Developers focus on the happy path and the pre-check, forgetting the decode-time fallback.
**How to avoid:** D-07 explicitly requires handling `Ok(None)` as `Skipped`. The `try_convert()` implementation must have a `None =>` arm in the match on `convert_image()`.
**Warning signs:** A test with mismatched extension/content should verify this path.

### Pitfall 3: Owned vs Borrowed Extension Strings
**What goes wrong:** `OutputFormat::extension()` returns `&'static str`, but `ConversionResult` stores `String`. Mixing these creates unnecessary clones or lifetime issues.
**Why it happens:** The enum must own its data (it's returned from a function), but the method returns a static reference.
**How to avoid:** Use `.to_string()` when constructing `Converted` from `format.extension()`. Use `source_ext.to_lowercase()` (which already returns `String`) for `Skipped`. Both paths produce owned strings cleanly.
**Warning signs:** Compiler errors about lifetimes in the enum variants.

### Pitfall 4: Not Testing Same-Format Conversion
**What goes wrong:** If source is PNG and target is PNG, `try_convert()` should still return `Converted` (the user asked for conversion). Developers might add a "skip if same format" optimization.
**Why it happens:** It seems wasteful to re-encode PNG to PNG.
**How to avoid:** Don't optimize this case. The user explicitly asked for `--convert png`. Same-format conversion may still change encoding parameters. Let Phase 1's `convert_image()` handle it -- it already works correctly for this case (verified in the format matrix test).
**Warning signs:** Special-case code checking if source extension matches target format.

## Code Examples

Verified patterns from the existing codebase:

### How Callers Will Use try_convert (Phase 5/6 Preview)
```rust
// In docx.rs or epub.rs (Phase 5/6 -- NOT this phase)
use crate::convert::{try_convert, ConversionResult, OutputFormat};

// Inside the extraction loop:
let result = try_convert(&image_data, &extension, format, quality)?;
let (bytes_to_write, output_ext) = match result {
    ConversionResult::Converted(bytes, ext) => (bytes, ext),
    ConversionResult::Skipped(ext) => {
        // Caller prints warning per D-01/D-02
        pb.suspend(|| {
            eprintln!(
                "Warning: Skipping conversion for {} ({} format not supported for conversion)",
                filename, ext.to_uppercase()
            );
        });
        (image_data, ext)
    }
};
let output_path = get_unique_output_path(output_dir, &base_name, i, total, &output_ext)?;
write_image_to_file(&output_path, &bytes_to_write)?;
```

### Existing Pattern: Enum with Derive Macros
```rust
// From src/convert.rs (Phase 1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Jpg,
    Png,
    Webp,
}
```

### Existing Pattern: Error Handling in convert_image
```rust
// From src/convert.rs (Phase 1) -- try_convert wraps this
let img = match image::load_from_memory(data) {
    Ok(img) => img,
    Err(ImageError::Unsupported(_)) => return Ok(None),  // -> Skipped
    Err(e) => return Err(e).context("Failed to decode image"),  // -> Err propagated
};
```

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` with standard test harness |
| Config file | None -- uses `cargo test` defaults |
| Quick run command | `cargo test convert::tests` |
| Full suite command | `cargo test` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONV-03 | `try_convert` returns `Skipped` for SVG/WMF/EMF extensions | unit | `cargo test convert::tests::test_try_convert_unsupported_extension -x` | Wave 0 |
| CONV-03 | `try_convert` returns `Skipped` when `convert_image` returns `Ok(None)` | unit | `cargo test convert::tests::test_try_convert_unsupported_at_decode -x` | Wave 0 |
| CONV-04 | `Converted` variant carries target format extension, not source | unit | `cargo test convert::tests::test_try_convert_correct_extension -x` | Wave 0 |
| CONV-04 | `OutputFormat::extension()` returns correct strings | unit | `cargo test convert::tests::test_output_format_extension -x` | Wave 0 |
| -- | `try_convert` returns `Converted` for supported format | unit | `cargo test convert::tests::test_try_convert_supported -x` | Wave 0 |
| -- | `try_convert` propagates `Err` for corrupt data | unit | `cargo test convert::tests::test_try_convert_corrupt_data -x` | Wave 0 |
| -- | Extension is lowercased in `Skipped` variant | unit | `cargo test convert::tests::test_try_convert_case_normalization -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test convert::tests`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green (all 30 existing + new tests) before verification

### Wave 0 Gaps
- All tests listed above are new and must be created as part of Phase 2 implementation
- No new test fixtures needed -- existing test helpers (`create_test_rgba_png`, `create_test_rgb_jpeg`, etc.) in `convert::tests` provide the test data
- No framework install needed -- standard Rust test harness already configured

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `convert_image` returns `Option<Vec<u8>>` | `try_convert` returns `ConversionResult` enum | Phase 2 (this phase) | Callers get named semantics instead of `Some`/`None` |
| Callers manually compose `can_convert` + `convert_image` + extension logic | Single `try_convert` call | Phase 2 (this phase) | Reduces caller boilerplate, eliminates error-prone composition |
| No way to get extension from `OutputFormat` | `OutputFormat::extension()` method | Phase 2 (this phase) | Callers don't need their own extension mapping |

**Note:** `convert_image()` and `can_convert()` remain public -- `try_convert()` is the recommended high-level API, but the low-level primitives stay available.

## Open Questions

1. **Should `ConversionResult` derive `Clone`?**
   - What we know: `Vec<u8>` implements `Clone`, `String` implements `Clone`, so `#[derive(Clone)]` would work. But cloning converted image bytes is expensive.
   - What's unclear: Will any caller need to clone a `ConversionResult`?
   - Recommendation: Start with `#[derive(Debug)]` only. Add `Clone` later if needed. This is within Claude's discretion per CONTEXT.md.

2. **Should the `Skipped` variant include the format name for warning messages?**
   - What we know: D-01 warning format needs the format name (e.g., "SVG"). The `Skipped` variant currently carries only the extension string.
   - What's unclear: Is `ext.to_uppercase()` sufficient for the warning, or should we carry a separate format name?
   - Recommendation: The extension IS the format name for our purposes. `"svg".to_uppercase()` produces `"SVG"` which matches D-01's pattern. No separate field needed.

## Sources

### Primary (HIGH confidence)
- `src/convert.rs` -- Phase 1 implementation, verified working with 30 tests
- `src/common.rs` -- `get_unique_output_path()` signature and extension parameter usage
- `.planning/phases/01-conversion-module-core/01-CONTEXT.md` -- Phase 1 decisions D-04 through D-13
- `.planning/phases/01-conversion-module-core/01-01-SUMMARY.md` -- Phase 1 completion status, patterns established
- `.planning/phases/02-format-handling-and-output-naming/02-CONTEXT.md` -- Phase 2 decisions D-01 through D-07

### Secondary (MEDIUM confidence)
- `image` crate v0.25.10 API -- `ImageError::Unsupported` variant behavior verified in existing tests
- `Cargo.toml` -- dependency versions confirmed via `cargo tree`

### Tertiary (LOW confidence)
None -- all findings are from primary sources (codebase and planning documents).

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing and verified
- Architecture: HIGH -- all decisions locked in CONTEXT.md, building on proven Phase 1 patterns
- Pitfalls: HIGH -- derived from direct code inspection of existing convert.rs and common.rs

**Research date:** 2026-04-02
**Valid until:** 2026-05-02 (stable -- no external dependencies or fast-moving APIs)
