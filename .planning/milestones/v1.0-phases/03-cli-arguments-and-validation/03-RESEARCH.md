# Phase 3: CLI Arguments and Validation - Research

**Researched:** 2026-04-02
**Domain:** clap derive-based CLI argument definition and validation in Rust
**Confidence:** HIGH

## Summary

Phase 3 adds five new CLI flags (`--convert`, `--quality`, `--gif-only`, `--gif-output`, `--lossless`) to the existing `Args` struct in `src/main.rs` and derives `ValueEnum` on `OutputFormat` in `src/convert.rs` for type-safe parsing of the `--convert` argument. The scope is narrow: argument definitions, declarative clap validation attributes (`conflicts_with`, `requires`), one manual post-parse validation check, and tests.

The existing codebase already demonstrates all required clap patterns. The `cover_fallback` field at line 50 of `main.rs` uses `#[arg(long, requires = "cover_only")]`, which is the exact pattern needed for `--quality requires --convert` and `--lossless requires --convert`. The `ValueEnum` derive is re-exported from `clap` with the `derive` feature (already enabled in `Cargo.toml`), requiring zero dependency changes.

**Primary recommendation:** Add `ValueEnum` derive to `OutputFormat` in `convert.rs`, add five new fields to `Args` in `main.rs` with clap attributes for declarative validation, and add a small `validate_args()` function for the two cross-value checks that clap cannot express declaratively.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** All new flags get short forms: `-C` (convert), `-q` (quality), `-g` (gif-only), `-G` (gif-output), `-L` (lossless). Capital letters for conversion-related flags.
- **D-02:** Capital letters (`-C`, `-G`, `-L`) differentiate conversion-related flags from existing lowercase flags.
- **D-03:** `--quality` works with both `--convert jpg` and `--convert webp`. Invalid only with `--convert png`.
- **D-04:** Default quality remains 85 for both JPEG and WebP lossy. `--quality` overrides this default.
- **D-05:** Use clap's declarative attributes (`conflicts_with`, `requires`, `value_parser`) for all validation that clap can express.
- **D-06:** One manual check after parsing: `--quality` with `--convert png` produces an error. Similarly, `--lossless` with `--convert jpg` or `--convert png` produces an error.
- **D-07:** Consistent with existing `--cover-fallback` requires `--cover-only` pattern.
- **D-08:** `--lossless` (`-L`) valid only with `--convert webp`. Switches from lossy to lossless WebP encoding.
- **D-09:** `--lossless` and `--quality` are mutually exclusive when used with `--convert webp`. Use `conflicts_with` in clap.
- **D-10:** Use clap's `ValueEnum` derive on `OutputFormat` to parse `--convert` values directly into the enum.

### Claude's Discretion
- Exact error message wording for the manual `--quality` + `--convert png` check
- Whether to add `ValueEnum` derive directly on the existing `OutputFormat` or create a wrapper
- Test strategy for argument validation (unit tests vs integration tests)
- Argument ordering and grouping in `--help` output

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CONV-06 | User can override JPEG quality via `--quality <1-100>` | clap `value_parser!(u8).range(1..=100)` provides range validation; `requires = "convert"` ensures dependency; D-03 extends to WebP too |
| CONV-07 | `--quality` is only valid with `--convert jpg` (updated by D-03: also valid with `--convert webp`, invalid only with `--convert png`) | Declarative `requires` ensures `--convert` is present; manual post-parse check validates against `OutputFormat::Png` specifically |
| GIF-04 | `--convert` and `--gif-only` are mutually exclusive (error if both specified) | clap `conflicts_with = "gif_only"` on the `convert` field handles this declaratively |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `clap` | `4.5.53` (resolved) | CLI argument parsing with derive macros | Already in project; `derive` feature already enabled; `ValueEnum` re-exported |

### Supporting
No additional dependencies are needed. All required functionality (`ValueEnum`, `conflicts_with`, `requires`, `value_parser!`) is available in clap 4.5.x with the existing `derive` feature.

### Cargo.toml Changes
None required. The `clap = { version = "4.5.4", features = ["derive"] }` entry already provides everything needed. The `ValueEnum` derive macro is part of `clap_derive`, which is pulled in by the `derive` feature.

**Verification:**
```bash
cargo tree -p clap --depth 1
# clap v4.5.53
# ├── clap_builder v4.5.53
# └── clap_derive v4.5.49 (proc-macro)  <-- provides ValueEnum derive
```

## Architecture Patterns

### Recommended Changes

```
src/
├── main.rs      # Args struct: add 5 new fields + validate_args() function
└── convert.rs   # OutputFormat enum: add ValueEnum derive + clap import
```

### Pattern 1: ValueEnum on OutputFormat

**What:** Add `#[derive(ValueEnum)]` to the existing `OutputFormat` enum in `convert.rs`. This makes clap automatically parse string values ("jpg", "png", "webp") into enum variants.

**When to use:** When a CLI argument accepts one of a fixed set of string values that map to an existing enum.

**Example:**
```rust
// In src/convert.rs -- add clap::ValueEnum to derives
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// JPEG output (lossy, quality 1-100)
    Jpg,
    /// PNG output (lossless, preserves transparency)
    Png,
    /// WebP output (lossy by default, lossless optional)
    Webp,
}
```

**Key detail:** `ValueEnum`'s default `rename_all` is `kebab-case`. Since all variants are single words (`Jpg`, `Png`, `Webp`), kebab-case simply lowercases them to `jpg`, `png`, `webp`. No explicit `rename_all` attribute needed.

**Why directly on OutputFormat (not a wrapper):** The existing enum already has the exact variants needed. Adding `ValueEnum` is additive -- it doesn't change the existing API. The `Clone` derive (required by `ValueEnum`) is already present.

### Pattern 2: Declarative Validation via clap Attributes

**What:** Use `conflicts_with`, `requires`, and `value_parser` attributes on `#[arg(...)]` to express validation rules declaratively.

**Example:**
```rust
// In src/main.rs Args struct

/// Convert extracted images to specified format
#[arg(short = 'C', long, conflicts_with = "gif_only")]
convert: Option<OutputFormat>,

/// JPEG/WebP encoding quality (1-100, default: 85)
#[arg(short = 'q', long, requires = "convert",
      conflicts_with = "lossless",
      value_parser = clap::value_parser!(u8).range(1..=100))]
quality: Option<u8>,

/// Extract only GIF files (skip all other image formats)
#[arg(short = 'g', long, conflicts_with = "convert")]
gif_only: bool,

/// Separate output directory for GIF files
#[arg(short = 'G', long)]
gif_output: Option<PathBuf>,

/// Use lossless WebP encoding instead of lossy
#[arg(short = 'L', long, requires = "convert",
      conflicts_with = "quality")]
lossless: bool,
```

**Critical notes on clap `conflicts_with` / `requires`:**
- The string values use the **Rust field name** (snake_case), not the CLI flag name. Example: `requires = "cover_only"` not `requires = "cover-only"`.
- `conflicts_with` is **bidirectional** in clap -- defining `A.conflicts_with(B)` is sufficient. You do NOT also need `B.conflicts_with(A)`. But it is acceptable (and clearer for code readers) to define it on both sides.
- `requires` means the depended-upon flag must be present. `--quality requires --convert` means `--quality 90` alone is an error; `--quality 90 --convert jpg` is valid.

### Pattern 3: Manual Post-Parse Validation

**What:** A `validate_args()` function called immediately after `Args::parse()` for cross-value checks that clap cannot express.

**When to use:** When validation depends on one argument's *value*, not just its *presence*. Clap's `conflicts_with`/`requires` operate on presence/absence only.

**Example:**
```rust
/// Validates argument combinations that clap cannot check declaratively.
///
/// Checks that require knowing the VALUE of --convert (not just its presence):
/// - --quality with --convert png (PNG is lossless, quality is meaningless)
/// - --lossless with --convert jpg or --convert png (lossless only applies to WebP)
fn validate_args(args: &Args) -> Result<()> {
    if let Some(format) = &args.convert {
        if args.quality.is_some() && *format == OutputFormat::Png {
            anyhow::bail!(
                "--quality cannot be used with --convert png (PNG is a lossless format)"
            );
        }
        if args.lossless && *format != OutputFormat::Webp {
            anyhow::bail!(
                "--lossless can only be used with --convert webp"
            );
        }
    }
    Ok(())
}
```

**Placement:** Called at the start of `main()`, right after `Args::parse()`, before any file processing.

### Pattern 4: Help Text Grouping

**What:** Use `///` doc comments on each field to generate help text. Clap displays fields in declaration order in `--help`.

**Recommendation:** Group the new conversion/GIF flags together in the `Args` struct, after the existing extraction flags. Order:
1. Existing fields (inputs, named_inputs, output, recursive, formats, cover_only, cover_fallback, title, author)
2. Conversion flags (convert, quality, lossless) -- grouped together
3. GIF flags (gif_only, gif_output) -- grouped together

### Anti-Patterns to Avoid
- **Do not use `default_value` in clap for quality:** The default quality of 85 is set in the conversion module (Phase 1 D-14/D-15). The CLI should pass `None` when `--quality` is not specified, and the caller should use 85 as the default. Using `default_value = "85"` in clap would make `--quality` always "present" which breaks the `requires` validation.
- **Do not duplicate OutputFormat:** Adding a separate `CliOutputFormat` wrapper creates unnecessary mapping code. `ValueEnum` can be added directly to the existing enum.
- **Do not put validation in `process_file()`:** Argument validation belongs at the CLI layer, not deep in the processing pipeline.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Enum-to-string parsing for --convert | Manual `match` on string to enum | `#[derive(ValueEnum)]` on `OutputFormat` | Automatic help text, error messages, and tab completion |
| Mutual exclusivity (convert vs gif-only) | Manual if-then-error check | `#[arg(conflicts_with = "gif_only")]` | Consistent clap error formatting, less code |
| Dependency (quality requires convert) | Manual presence check | `#[arg(requires = "convert")]` | Clap generates standard error messages |
| Range validation (quality 1-100) | Manual bounds checking | `value_parser!(u8).range(1..=100)` | Clap generates "value not in range" errors |

**Key insight:** Clap's declarative attributes handle 90% of the validation. Only the cross-value checks (quality+png, lossless+non-webp) require manual code, because clap attributes can only reason about argument presence, not argument values.

## Common Pitfalls

### Pitfall 1: Field Name vs Flag Name in requires/conflicts_with
**What goes wrong:** Using `requires = "convert"` works, but using `requires = "gif-only"` fails because the clap derive system expects the Rust field name (`gif_only` with underscore), not the CLI flag name (`gif-only` with hyphen).
**Why it happens:** Clap derive auto-converts field names to kebab-case for the CLI flag, but the `requires`/`conflicts_with` attributes reference the internal argument ID which is the field name.
**How to avoid:** Always use the Rust field name (snake_case) in `requires` and `conflicts_with` strings. Match the existing `requires = "cover_only"` pattern at line 50.
**Warning signs:** Clap panics at runtime with "Argument or group 'xxx' specified in 'conflicts_with' not found" if the ID is wrong.

### Pitfall 2: value_parser Range With Option Type
**What goes wrong:** Using `value_parser = clap::value_parser!(u8).range(1..=100)` with an `Option<u8>` field. Some developers worry this won't work with `Option`.
**Why it happens:** Confusion about how clap's type system wraps value parsers.
**How to avoid:** This works correctly. Clap handles `Option<T>` wrapping automatically -- the `value_parser` applies to the inner `T`, and `Option` is handled by the arg's required/optional status. Since `--quality` is not mandatory, it should be `Option<u8>`.
**Warning signs:** None -- this is a misconception that doesn't actually cause problems if you do it correctly.

### Pitfall 3: Bidirectional conflicts_with Duplication
**What goes wrong:** Defining `conflicts_with` on both sides of a mutual exclusion (e.g., on both `convert` and `gif_only`), when only one side is needed.
**Why it happens:** Developers assume unidirectional semantics.
**How to avoid:** Clap's `conflicts_with` IS bidirectional -- defining it on one side is sufficient. However, defining it on both sides is not harmful and can improve readability. The CONTEXT decision D-05 says to use `conflicts_with` for convert/gif-only; defining it on the `convert` field is sufficient. For clarity, it can also be placed on `gif_only`.
**Warning signs:** None -- duplication is harmless but verbose.

### Pitfall 4: Default Value Breaking requires
**What goes wrong:** If `--quality` had `default_value = "85"`, clap would consider it "present" even when the user didn't specify it. Then `requires = "convert"` would always trigger because `quality` is always "present".
**Why it happens:** `default_value` makes the argument appear as if the user supplied it.
**How to avoid:** Use `Option<u8>` with no default. Apply the 85 default in application logic, not in clap. This matches the existing pattern where defaults are handled in `main()` logic (see CLAUDE.md conventions: "Default values handled in `main()` logic, not in clap attributes").
**Warning signs:** Error "the following required arguments were not provided: --convert" when running without any conversion flags.

### Pitfall 5: ValueEnum and Existing Derives
**What goes wrong:** Concern that adding `ValueEnum` to `OutputFormat` might conflict with existing derives or change behavior.
**Why it happens:** Unfamiliarity with the derive macro.
**How to avoid:** `ValueEnum` only adds implementations of the `clap::ValueEnum` trait. It doesn't affect `Debug`, `Clone`, `Copy`, `PartialEq`, or `Eq`. The only requirement is that `Clone` must already be derived (which it is). All existing code using `OutputFormat` continues to work unchanged.
**Warning signs:** None -- this is additive.

## Code Examples

Verified patterns from the existing codebase and clap API:

### Full Args Struct Addition
```rust
// Source: existing pattern in src/main.rs lines 23-60, extended with CONTEXT.md decisions

// Add to existing imports in main.rs:
use crate::convert::OutputFormat;

// New fields added to Args struct (after existing fields):

    /// Convert extracted images to specified format (jpg, png, webp)
    #[arg(short = 'C', long, conflicts_with = "gif_only")]
    convert: Option<OutputFormat>,

    /// JPEG/WebP encoding quality override (1-100, default: 85)
    #[arg(short = 'q', long, requires = "convert",
          conflicts_with = "lossless",
          value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: Option<u8>,

    /// Use lossless WebP encoding instead of lossy
    #[arg(short = 'L', long, requires = "convert",
          conflicts_with = "quality")]
    lossless: bool,

    /// Extract only GIF files (skip all other image formats)
    #[arg(short = 'g', long, conflicts_with = "convert")]
    gif_only: bool,

    /// Separate output directory for GIF files
    #[arg(short = 'G', long)]
    gif_output: Option<PathBuf>,
```

### ValueEnum Derive on OutputFormat
```rust
// Source: src/convert.rs line 16, adding clap::ValueEnum derive

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// JPEG output (lossy, quality 1-100)
    Jpg,
    /// PNG output (lossless, preserves transparency)
    Png,
    /// WebP output (lossy by default, lossless optional)
    Webp,
}
```

### Post-Parse Validation Function
```rust
// Source: new function in src/main.rs

/// Validates argument combinations that clap cannot check declaratively.
fn validate_args(args: &Args) -> Result<()> {
    if let Some(format) = &args.convert {
        if args.quality.is_some() && *format == OutputFormat::Png {
            anyhow::bail!(
                "--quality cannot be used with --convert png (PNG is a lossless format)"
            );
        }
        if args.lossless && *format != OutputFormat::Webp {
            anyhow::bail!(
                "--lossless can only be used with --convert webp"
            );
        }
    }
    Ok(())
}
```

### Unit Test Pattern Using try_parse_from
```rust
// Source: clap Parser trait provides try_parse_from for testing

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_flag_parses_format() {
        let args = Args::try_parse_from(["test", "--convert", "jpg"]).unwrap();
        assert_eq!(args.convert, Some(OutputFormat::Jpg));
    }

    #[test]
    fn test_convert_and_gif_only_conflict() {
        let result = Args::try_parse_from(["test", "--convert", "jpg", "--gif-only"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_quality_requires_convert() {
        let result = Args::try_parse_from(["test", "--quality", "90"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_quality_range_validation() {
        // 0 is below range
        let result = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "0"]);
        assert!(result.is_err());
        // 101 is above range
        let result = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "101"]);
        assert!(result.is_err());
        // 1 and 100 are valid bounds
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "1"]).unwrap();
        assert_eq!(args.quality, Some(1));
        let args = Args::try_parse_from(["test", "--convert", "jpg", "--quality", "100"]).unwrap();
        assert_eq!(args.quality, Some(100));
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual string matching for enum args | `#[derive(ValueEnum)]` | clap 4.0 (2022) | Auto-generates parsing, help text, error messages |
| `possible_values` attribute | `ValueEnum` derive | clap 4.0 (2022) | Type-safe enum parsing instead of string matching |
| `validator` function on arg | `value_parser` attribute | clap 4.0 (2022) | Composable, type-safe value parsers |

**Deprecated/outdated:**
- `possible_values(["jpg", "png", "webp"])`: Replaced by `ValueEnum` derive. Still works but less ergonomic.
- `validator(|s| s.parse::<u8>().map(|_| ()))`: Replaced by `value_parser!(u8).range(...)`. The old approach required manual error formatting.

## Open Questions

1. **Help text grouping**
   - What we know: Clap 4.x supports `help_heading` attribute for grouping args in `--help` output.
   - What's unclear: Whether grouping into sections (e.g., "Conversion Options", "GIF Options") would improve UX or is overkill for 5 new flags.
   - Recommendation: Start without `help_heading`. The flags are self-explanatory from their doc comments. Add grouping later if `--help` becomes crowded.

2. **Error message tone for manual validation**
   - What we know: Clap generates errors like "error: the argument '--quality <QUALITY>' cannot be used with '--convert png'". Manual errors should match this tone.
   - What's unclear: Whether to use `anyhow::bail!()` (project convention) or `clap::Error::raw()` (native clap error formatting).
   - Recommendation: Use `anyhow::bail!()` for consistency with the rest of the codebase. The error is printed via the existing error handling in `main()`. The slight formatting difference from clap errors is acceptable.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust standard test harness (`#[cfg(test)]` / `cargo test`) |
| Config file | None (standard Cargo setup) |
| Quick run command | `cargo test` |
| Full suite command | `cargo test` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONV-06 | `--quality 90 --convert jpg` sets quality to 90 | unit | `cargo test test_quality_with_convert_jpg -- --exact` | No -- Wave 0 |
| CONV-06 | `--quality` range 1-100 enforced | unit | `cargo test test_quality_range_validation -- --exact` | No -- Wave 0 |
| CONV-07 | `--quality` with `--convert png` produces error | unit | `cargo test test_quality_with_png_error -- --exact` | No -- Wave 0 |
| CONV-07 | `--quality` with `--convert webp` succeeds | unit | `cargo test test_quality_with_convert_webp -- --exact` | No -- Wave 0 |
| GIF-04 | `--convert` and `--gif-only` conflict | unit | `cargo test test_convert_and_gif_only_conflict -- --exact` | No -- Wave 0 |

Additional validation tests (not directly required by phase requirements but needed for completeness):
| Behavior | Test Type | Automated Command |
|----------|-----------|-------------------|
| `--convert` parses all three formats | unit | `cargo test test_convert_parses_all_formats` |
| `--quality` requires `--convert` | unit | `cargo test test_quality_requires_convert` |
| `--lossless` requires `--convert` | unit | `cargo test test_lossless_requires_convert` |
| `--lossless` conflicts with `--quality` | unit | `cargo test test_lossless_conflicts_with_quality` |
| `--lossless` with `--convert jpg` produces error | unit | `cargo test test_lossless_with_jpg_error` |
| `--lossless` with `--convert png` produces error | unit | `cargo test test_lossless_with_png_error` |
| Short flags work (`-C`, `-q`, `-g`, `-G`, `-L`) | unit | `cargo test test_short_flags` |
| `--gif-output` works independently | unit | `cargo test test_gif_output_independent` |
| Existing flags still work after changes | unit | `cargo test test_existing_flags_unchanged` |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/main.rs` test module -- add `#[cfg(test)] mod tests { ... }` (no test module exists in main.rs currently)
- [ ] All test functions listed above need to be created
- [ ] No additional framework or fixture setup needed -- `Args::try_parse_from` is available from the `Parser` derive already on `Args`

## Sources

### Primary (HIGH confidence)
- `src/main.rs` lines 23-60 -- Existing `Args` struct with all current clap patterns
- `src/main.rs` line 50 -- `#[arg(long, requires = "cover_only")]` pattern for dependent flags
- `src/convert.rs` lines 16-24 -- Existing `OutputFormat` enum with derives
- `Cargo.toml` line 8 -- `clap = { version = "4.5.4", features = ["derive"] }` confirms derive feature
- `target/doc/src/clap/lib.rs.html` line 93 -- `pub use clap_derive::{...ValueEnum}` confirms re-export
- `target/doc/src/clap_builder/builder/value_parser.rs.html` lines 2330-2334 -- `u8` support in `value_parser!`
- `target/doc/src/clap_builder/builder/arg.rs.html` lines 3941-3942 -- `conflicts_with` is bidirectional

### Secondary (MEDIUM confidence)
- clap 4.5.x general API knowledge -- `ValueEnum` default rename_all is kebab-case (verified by variant naming convention in clap_derive source)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- No new dependencies needed; all clap features verified in local docs
- Architecture: HIGH -- All patterns exist in the codebase; extending is mechanical
- Pitfalls: HIGH -- All pitfalls verified against actual clap source code and existing project patterns

**Research date:** 2026-04-02
**Valid until:** 2026-05-02 (stable -- clap 4.x API is mature)
