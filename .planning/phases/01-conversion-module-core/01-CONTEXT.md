# Phase 1: Conversion Module Core - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Build `src/convert.rs` — a standalone image conversion module that decodes source image bytes and re-encodes to a target format (JPEG, PNG, or WebP). Handles alpha compositing for JPEG, supports both lossy and lossless WebP, and provides a clean API for downstream integration in Phases 5-6. This module has no filesystem access — it is a pure byte-in, byte-out transformation.

</domain>

<decisions>
## Implementation Decisions

### WebP Encoding Strategy
- **D-01:** Default to lossy WebP encoding via the `webp` crate (wraps Google libwebp). Produces smaller files with quality control.
- **D-02:** Support lossless WebP via the `image` crate's built-in encoder, activated by a `--lossless` flag.
- **D-03:** The `--lossless` flag applies only to WebP output — PNG is already lossless, JPEG is always lossy.

### Conversion API Design
- **D-04:** `convert_image(data: &[u8], format: OutputFormat, quality: u8) -> Result<Option<Vec<u8>>>` — takes raw bytes, returns converted bytes. No filesystem access.
- **D-05:** Use an `OutputFormat` enum with variants `Jpg`, `Png`, `Webp` for type-safe target format selection.
- **D-06:** Expose `can_convert(extension: &str) -> bool` to let callers check before attempting conversion. Returns true for formats the `image` crate can decode (jpg, png, gif, bmp, tiff, webp, ico).
- **D-07:** Module returns `Vec<u8>` only — the caller handles writing to disk via existing `write_image_to_file()`.

### Alpha Compositing
- **D-08:** When converting to JPEG, composite alpha channel against a white background. This prevents the black-background issue from silent alpha stripping.
- **D-09:** When converting to PNG or WebP, preserve the alpha channel — both formats support transparency natively.
- **D-10:** Alpha compositing is only triggered when the source image actually has an alpha channel (check DynamicImage color type).

### Error vs Skip Behavior
- **D-11:** `convert_image` returns `Ok(None)` for unsupported source formats (SVG, WMF, EMF) — signals "skip gracefully" to the caller.
- **D-12:** `convert_image` returns `Err(...)` for corrupt/undecodable images — signals "something went wrong" so the caller can warn and continue.
- **D-13:** The `can_convert()` function aligns with `Ok(None)` behavior — formats that return false from `can_convert()` will return `Ok(None)` from `convert_image()`.

### Quality Defaults
- **D-14:** JPEG default quality is 85 (not the `image` crate's default of 75). Matches expectations for document-extracted images.
- **D-15:** WebP lossy default quality is 85 (consistent with JPEG default).

### Claude's Discretion
- Internal module structure (helper functions, how to organize decode/encode logic)
- Whether to use `image::load_from_memory` or `ImageReader` for decoding
- WebP crate API integration details (Encoder::from_image vs manual pixel access)
- Unit test strategy (which format combinations to test, test image generation)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Context
- `.planning/PROJECT.md` — Core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` — CONV-02 (alpha compositing), CONV-05 (JPEG quality 85)

### Research
- `.planning/research/STACK.md` — `image` crate feature flags, `webp` crate API, version recommendations
- `.planning/research/PITFALLS.md` — Alpha compositing pitfall (#1), WebP lossless limitation (#2), unsupported formats (#3)
- `.planning/research/ARCHITECTURE.md` — Module design, API shape, data flow

### Codebase
- `.planning/codebase/CONVENTIONS.md` — Naming patterns, error handling conventions, doc comment style
- `.planning/codebase/STRUCTURE.md` — Module layout, where to add new code

### Source (integration points)
- `src/common.rs` — `write_image_to_file()` (the downstream consumer of converted bytes), `get_supported_extensions()`
- `Cargo.toml` — Dependencies to update with `image` and `webp` crates

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `write_image_to_file(&Path, &[u8])` in `common.rs` — handles disk write with BufWriter; convert.rs outputs bytes that feed into this
- `normalize_format(fmt)` in `common.rs` — handles format aliases (jpg/jpeg, tiff/tif); may inform `OutputFormat` enum design
- `get_supported_extensions()` in `common.rs` — defines the full set of recognized image extensions

### Established Patterns
- All public functions return `anyhow::Result<T>`
- `///` doc comments on all public items
- Unit tests in `#[cfg(test)] mod tests {}` at bottom of each file
- Module files are flat in `src/` — `convert.rs` follows this pattern
- Error context uses `.with_context(|| format!("Failed to {action}: {detail}"))`

### Integration Points
- `mod convert;` declaration will go in `src/main.rs` (line ~8 area, alongside other module declarations)
- `Cargo.toml` needs `image` (with selective features) and `webp` crate additions
- convert.rs is called by docx.rs and epub.rs in later phases — API must be ergonomic for both

</code_context>

<specifics>
## Specific Ideas

- WebP lossy via `webp` crate by default, `--lossless` flag switches to `image` crate's lossless encoder
- `can_convert()` acts as a fast pre-check so callers can route unsupported formats to raw extraction without attempting decode
- Quality parameter (u8) passed to both JPEG and WebP lossy encoders for consistency

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 01-conversion-module-core*
*Context gathered: 2026-04-02*
