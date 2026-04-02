# Project Research Summary

**Project:** Word/EPUB Image Extractor -- Conversion & GIF Features
**Domain:** Rust CLI image extraction and format conversion
**Researched:** 2026-04-01
**Confidence:** HIGH

## Executive Summary

This project adds image format conversion (`--convert jpg|png|webp`) and GIF-specific routing (`--gif-only`, `--gif-output`) to an existing Rust CLI tool that extracts images from DOCX and EPUB archives. The recommended approach uses the `image` crate (v0.25) for decoding all raster formats and encoding to JPEG/PNG, supplemented by the `webp` crate (v0.3) for lossy WebP encoding -- a well-proven two-crate combination in the Rust ecosystem. Total new dependencies: two crates. The existing architecture has a clean seam for insertion: conversion slots in between "raw bytes extracted from archive" and "bytes written to disk" as an optional transformation step, housed in a new `src/convert.rs` module.

The single biggest technical risk is RGBA-to-JPEG alpha channel handling: the `image` crate silently composites transparent pixels as black when converting to JPEG, producing visually broken output for any image with transparency (common in document graphics). This must be handled explicitly by compositing against a white background before JPEG encoding. The second major concern is that the `image` crate's built-in WebP encoder is lossless-only, meaning JPEG-to-WebP conversion produces *larger* files -- the `webp` crate solves this with lossy encoding and quality control. Both issues have straightforward solutions that should be implemented from the start, not retrofitted.

The feature set is well-scoped: three conversion targets (JPG, PNG, WebP), GIF filtering and routing, a quality parameter for JPEG, and graceful skip-and-warn for unconvertible formats (WMF, EMF, SVG). Anti-features are clearly identified -- no multi-format output, no image resizing, no animated GIF frame extraction, no AVIF. The architecture research provides exact function signatures and a build order that produces testable increments at each step.

## Key Findings

### Recommended Stack

Two new crate dependencies cover all conversion needs. The existing dependencies (clap, zip, anyhow, indicatif, walkdir, epub) require no changes.

**Core technologies:**
- `image` v0.25 (with selective features: jpeg, png, gif, bmp, tiff, webp): Decode all raster formats found in DOCX/EPUB, encode to JPEG and PNG. De facto standard at 8.6M downloads/month. Use `default-features = false` to avoid pulling in AVIF, EXR, HDR, and other irrelevant codecs that bloat compile time.
- `webp` v0.3: Lossy WebP encoding with quality control (0-100). Wraps Google's libwebp via libwebp-sys. Integrates directly with `image::DynamicImage` via `Encoder::from_image()`. Requires a C compiler at build time (MSVC, already present in the project toolchain).

**Key version requirements:** `image` must be 0.25.x (WebP encoding support added in 0.24; 0.25 is current stable). The `webp` crate at 0.3.x is stable and the only mature option for lossy WebP in Rust.

**Alternatives rejected:** RIL (missing BMP/TIFF decode), Photon (unnecessary wrapper), image-webp lossless-only encoder, direct libwebp-sys (unsafe FFI, no benefit over `webp` crate's safe API).

### Expected Features

**Must have (table stakes):**
- `--convert <jpg|png|webp>` with correct output naming (target extension, not source)
- Alpha-to-white compositing for JPEG conversion (prevents black backgrounds)
- Skip unconvertible formats (WMF/EMF/SVG) with warning, extract as-is
- JPEG quality default of 85 (not the crate's default of 75, which produces visible artifacts)
- `--gif-only` mode (syntactic sugar for `--formats gif`)
- `--gif-output <path>` for routing GIFs to a separate directory
- Mutual exclusivity of `--convert` and `--gif-only` (clap `conflicts_with`)
- Progress indication during conversion (extend existing indicatif bars)
- Clear per-file error messages that do not abort the batch

**Should have (differentiators):**
- `--quality <1-100>` flag for JPEG output control
- Conversion summary statistics (converted/skipped/failed counts)
- `--gif-output` works independently of `--gif-only` (route GIFs while extracting all formats)

**Defer (v2+):**
- `--dry-run` mode (threads a flag through all code paths, moderate complexity)
- Lossy-vs-lossless WebP toggle (the `webp` crate handles lossy; lossless is a niche need)
- AVIF output (requires C dependencies, niche demand)
- Image resizing, animated GIF frame extraction, SVG rasterization

### Architecture Approach

Conversion inserts as an optional transformation step in the existing pipeline. A new `src/convert.rs` module is a pure data transformer: it takes raw bytes + source extension + target format and returns converted bytes + new extension (or `None` for unconvertible formats). It never touches the filesystem. GIF routing is a simple directory-selection helper in `common.rs`, not a separate module. Both DOCX and EPUB processors gain ~12 lines of new code each to call conversion before writing.

**Major components:**
1. `src/convert.rs` (NEW) -- Image decoding via magic bytes, format conversion, alpha compositing for JPEG, unsupported format detection. Pure data in/data out.
2. `src/common.rs` (MODIFIED) -- Add `resolve_output_dir()` for GIF routing. 5-line pure function.
3. `src/main.rs` (MODIFIED) -- Add `--convert`, `--gif-only`, `--gif-output`, `--quality` CLI args with clap annotations.
4. `src/docx.rs` (MODIFIED) -- Call `convert::convert_image()` before `write_image_to_file()`. ~12 lines.
5. `src/epub.rs` (MODIFIED) -- Same pattern as docx.rs, but more write paths to update (cover extraction, all-images extraction).

**Key patterns:** Optional transformation returning `Option<(Vec<u8>, &str)>` (converted/skipped/failed three-state), configuration threading (no global state), extension-based routing as a pure function.

**Anti-patterns to avoid:** No trait abstraction for two processors (premature), no conversion inside `write_image_to_file` (violates SRP), no separate GIF pipeline (it is a routing concern, not a processing concern).

### Critical Pitfalls

1. **RGBA-to-JPEG black backgrounds** -- The `image` crate drops alpha, compositing transparent pixels as black. Composite against white background before JPEG encoding. Must be in the core conversion implementation.
2. **WebP lossless-only in `image` crate** -- JPEG-to-WebP produces larger files. Use the `webp` crate for lossy encoding with quality control. This is the entire reason the `webp` crate is a dependency.
3. **SVG/WMF/EMF decode failure** -- These vector formats cannot be decoded by the `image` crate. Check extension before attempting decode; skip with warning and extract original bytes.
4. **Animated GIF frame loss** -- `DynamicImage` loads only the first frame. When `--convert` targets a GIF source, warn the user that animation is lost. The `--gif-only`/`--convert` mutual exclusivity partially mitigates this.
5. **JPEG quality 75 default** -- The crate default is too low for document images. Use `JpegEncoder::new_with_quality()` at 85, not `save()` which hardcodes 75.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Foundation -- Dependencies and Conversion Module

**Rationale:** `convert.rs` is standalone with no dependencies on other project modules. It can be built and unit-tested in isolation. Getting dependencies right (selective `image` features, `webp` crate) is the first step.
**Delivers:** `src/convert.rs` with `ConvertTarget` enum, `can_convert()`, `convert_image()`, and `target_extension()`. Cargo.toml with both new dependencies. Unit tests for: JPEG encoding with alpha compositing, PNG encoding, WebP lossy encoding, unsupported format skip, magic-byte detection.
**Addresses features:** Core conversion logic (table stakes), alpha channel handling, unsupported format handling, JPEG quality default.
**Avoids pitfalls:** Pitfall 1 (alpha/black backgrounds), Pitfall 2 (WebP lossless), Pitfall 3 (SVG/WMF/EMF), Pitfall 5 (JPEG quality), Pitfall 6 (extension mismatch -- uses magic bytes), Pitfall 12 (feature flag bloat).

### Phase 2: CLI Arguments and GIF Routing

**Rationale:** CLI args must exist before processors can receive them. GIF routing helper is a small pure function that can be tested independently. These are prerequisites for integration.
**Delivers:** `--convert`, `--gif-only`, `--gif-output`, `--quality` args in `main.rs` with clap annotations and conflict rules. `resolve_output_dir()` in `common.rs`. Unit tests for routing logic and argument validation.
**Addresses features:** All CLI-facing features, mutual exclusivity, `--gif-only` as format filter, `--quality` parameter.
**Avoids pitfalls:** Pitfall 7 (output extension must match target format -- extension logic is set here).

### Phase 3: DOCX Integration

**Rationale:** DOCX processor has a simpler write loop than EPUB (single extraction path). Integrating conversion here first serves as a proving ground before tackling the more complex EPUB paths.
**Delivers:** `docx::process_file()` updated to accept conversion and GIF routing parameters. Conversion applied to extracted images. Integration tests with real DOCX files containing PNG, JPEG, GIF, BMP, and WMF images.
**Addresses features:** End-to-end conversion for DOCX, GIF routing for DOCX.
**Avoids pitfalls:** Pitfall 4 (animated GIF warning), Pitfall 8 (sequential processing, no memory accumulation).

### Phase 4: EPUB Integration

**Rationale:** EPUB has multiple extraction paths (all images, cover only, cover by filename). Each must be updated to support conversion and GIF routing. More complex, so it comes after DOCX is proven.
**Delivers:** `epub::process_file()` updated with conversion and routing. Integration tests across all EPUB extraction modes.
**Addresses features:** End-to-end conversion for EPUB, GIF routing for EPUB.
**Avoids pitfalls:** Same as Phase 3, plus ensuring cover-only mode interacts correctly with conversion.

### Phase 5: Polish -- Warnings, Statistics, and Documentation

**Rationale:** User-facing polish (summary statistics, WebP size warnings, animated GIF warnings, help text) is best added after core functionality works end-to-end.
**Delivers:** Conversion summary statistics, WebP-from-JPEG size warning, animated GIF frame-loss warning, updated `--help` text, README documentation.
**Addresses features:** Conversion summary statistics (differentiator), all warning messages.
**Avoids pitfalls:** Pitfall 2 (user-facing WebP documentation), Pitfall 4 (animated GIF user awareness).

### Phase Ordering Rationale

- **Dependency-driven:** convert.rs has no project dependencies, so it comes first. CLI args depend on nothing but are needed by processors. DOCX is simpler than EPUB, so it integrates first.
- **Architecture-aligned:** Each phase maps to a clear component boundary (convert module, CLI layer, DOCX processor, EPUB processor, UX layer).
- **Risk-front-loaded:** The hardest technical problems (alpha compositing, WebP lossy encoding, unsupported format handling) are all in Phase 1, which has the best testability. If something goes wrong, it is caught early.
- **Incrementally shippable:** After Phase 3, the tool can convert DOCX images. After Phase 4, EPUB too. Phase 5 is polish.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1 (convert.rs):** Needs research into `webp` crate's `Encoder::from_image()` edge cases (color space handling, error modes). Also needs research into alpha compositing implementation -- the pixel-by-pixel blend loop vs. using `DynamicImage::into_rgba8()` and creating a white background image.

Phases with standard patterns (skip research):
- **Phase 2 (CLI args):** Well-documented clap derive patterns. No research needed.
- **Phase 3 (DOCX integration):** Straightforward insertion of conversion call before write. Existing code structure is clear.
- **Phase 4 (EPUB integration):** Same pattern as Phase 3, just applied to more code paths. May need light research on how cover-only extraction interacts with conversion.
- **Phase 5 (polish):** Standard CLI output patterns. No research needed.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | `image` crate is the undisputed standard. `webp` crate is the only viable lossy WebP option. Both verified from official docs and crates.io metrics. |
| Features | HIGH | Feature set is well-defined by PROJECT.md constraints. Table stakes and anti-features are clear. No ambiguity on scope. |
| Architecture | HIGH | Existing codebase has a clean insertion point. Function signatures, data flow, and module boundaries are fully specified. |
| Pitfalls | HIGH | All critical pitfalls verified against `image` crate source code and documentation. Alpha handling and WebP lossless issues are well-documented in the ecosystem. |

**Overall confidence:** HIGH

### Gaps to Address

- **Animated GIF detection performance:** The recommended approach (decode with GifDecoder, check frame count) requires decoding the GIF twice -- once to check animation, once via `load_from_memory` for conversion. Consider whether a lightweight header-only check is feasible, or whether the double-decode cost is acceptable for the uncommon case.
- **`webp` crate error handling:** The `Encoder::from_image()` call returns a `Result` but the error types are not well-documented. Need to verify behavior with unusual color spaces (grayscale, palette-indexed) during Phase 1 implementation.
- **EPUB cover + conversion interaction:** When `--cover-only` and `--convert` are both active, should the cover image be converted? Almost certainly yes, but this needs explicit validation during Phase 4.
- **`--gif-output` + `--convert` interaction:** When both are active, GIFs routed to the GIF output directory should be written as-is (unconverted). This is a design decision identified in architecture research that needs confirmation during Phase 2 CLI design.
- **Parameter count growth:** `epub::process_file` will have 8 parameters after changes. Architecture research flags this as approaching the threshold for a config struct refactor. Decide during Phase 4 whether to bundle parameters.

## Sources

### Primary (HIGH confidence)
- [image crate docs (docs.rs)](https://docs.rs/image/latest/image/) -- format support, API surface, encoding capabilities
- [image crate codecs](https://docs.rs/image/latest/image/codecs/index.html) -- encoding/decoding support matrix
- [JpegEncoder API](https://docs.rs/image/latest/image/codecs/jpeg/struct.JpegEncoder.html) -- quality parameter, default of 75
- [WebPEncoder docs](https://docs.rs/image/latest/image/codecs/webp/struct.WebPEncoder.html) -- confirms lossless-only
- [image-webp GitHub](https://github.com/image-rs/image-webp) -- "only supports lossless encoding"
- [webp crate docs](https://docs.rs/webp/0.3.1/webp/struct.Encoder.html) -- lossy encoding API, `from_image` integration
- [image crate on crates.io](https://crates.io/crates/image) -- v0.25.10, adoption metrics
- [webp crate on lib.rs](https://lib.rs/crates/webp) -- v0.3.1, adoption metrics
- [image crate GitHub Cargo.toml](https://github.com/image-rs/image/blob/main/Cargo.toml) -- feature flag definitions

### Secondary (MEDIUM confidence)
- [sic/imagineer CLI](https://github.com/foresterre/sic) -- JPEG default quality 80, industry comparison
- [ImageMagick convert patterns](https://imagemagick.org/script/convert.php) -- CLI convention reference
- [Rust forum: image processing recommendations](https://users.rust-lang.org/t/looking-for-image-processing-crate-recommendation-introduction/123968)
- [Rust forum: PNG/JPEG to WebP](https://users.rust-lang.org/t/converting-png-jpeg-image-to-webp/71080)
- [gif crate](https://crates.io/crates/gif) -- animated GIF frame handling

### Tertiary (LOW confidence)
- [Image compression strategy 2025](https://unifiedimagetools.com/en/articles/ultimate-image-compression-strategy-2025) -- general guidance on quality defaults

---
*Research completed: 2026-04-01*
*Ready for roadmap: yes*
