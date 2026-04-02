# Phase 1: Conversion Module Core - Research

**Researched:** 2026-04-02
**Domain:** Rust image format conversion (decode/encode pipeline) using `image` and `webp` crates
**Confidence:** HIGH

## Summary

This phase builds `src/convert.rs` -- a pure byte-in, byte-out image conversion module with no filesystem access. The module decodes source image bytes using the `image` crate (v0.25.10) and re-encodes to one of three target formats: JPEG, PNG, or WebP. The `webp` crate (v0.3.1) provides lossy WebP encoding since the `image` crate's built-in WebP encoder is lossless-only.

The critical technical challenge is alpha compositing for JPEG conversion. When converting RGBA images (common in DOCX/EPUB -- PNG, GIF, WebP sources) to JPEG, the alpha channel must be composited against a white background. The `image` crate's automatic RGBA-to-RGB conversion strips alpha without compositing, producing black backgrounds where transparency existed. This is the most user-visible defect if not handled correctly.

The API design is locked by user decisions: `convert_image(data: &[u8], format: OutputFormat, quality: u8) -> Result<Option<Vec<u8>>>` with an `OutputFormat` enum (`Jpg`, `Png`, `Webp`) and a `can_convert(extension: &str) -> bool` pre-check function.

**Primary recommendation:** Build the module as three distinct internal stages (format check, decode, encode) with alpha compositing as a mandatory intermediate step before JPEG encoding. Use `image::load_from_memory()` for decoding (magic-byte detection, robust against mismatched extensions) and explicit encoders (`JpegEncoder::new_with_quality`, `PngEncoder::new`, `webp::Encoder::from_image`) for encoding.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Default to lossy WebP encoding via the `webp` crate (wraps Google libwebp). Produces smaller files with quality control.
- **D-02:** Support lossless WebP via the `image` crate's built-in encoder, activated by a `--lossless` flag.
- **D-03:** The `--lossless` flag applies only to WebP output -- PNG is already lossless, JPEG is always lossy.
- **D-04:** `convert_image(data: &[u8], format: OutputFormat, quality: u8) -> Result<Option<Vec<u8>>>` -- takes raw bytes, returns converted bytes. No filesystem access.
- **D-05:** Use an `OutputFormat` enum with variants `Jpg`, `Png`, `Webp` for type-safe target format selection.
- **D-06:** Expose `can_convert(extension: &str) -> bool` to let callers check before attempting conversion. Returns true for formats the `image` crate can decode (jpg, png, gif, bmp, tiff, webp, ico).
- **D-07:** Module returns `Vec<u8>` only -- the caller handles writing to disk via existing `write_image_to_file()`.
- **D-08:** When converting to JPEG, composite alpha channel against a white background.
- **D-09:** When converting to PNG or WebP, preserve the alpha channel.
- **D-10:** Alpha compositing is only triggered when the source image actually has an alpha channel (check DynamicImage color type).
- **D-11:** `convert_image` returns `Ok(None)` for unsupported source formats (SVG, WMF, EMF).
- **D-12:** `convert_image` returns `Err(...)` for corrupt/undecodable images.
- **D-13:** The `can_convert()` function aligns with `Ok(None)` behavior.
- **D-14:** JPEG default quality is 85 (not the `image` crate's default of 75).
- **D-15:** WebP lossy default quality is 85.

### Claude's Discretion
- Internal module structure (helper functions, how to organize decode/encode logic)
- Whether to use `image::load_from_memory` or `ImageReader` for decoding
- WebP crate API integration details (Encoder::from_image vs manual pixel access)
- Unit test strategy (which format combinations to test, test image generation)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CONV-02 | JPEG conversion composites alpha channels against a white background (transparent regions must not render as black) | Verified: `image` crate JPEG encoder only supports L8/Rgb8 color types. Its automatic RGBA-to-Rgb8 conversion strips alpha without compositing (produces black). Manual alpha compositing against white is required before encoding. See Architecture Patterns > Alpha Compositing Pattern. |
| CONV-05 | JPEG conversion uses quality 85 by default | Verified: `JpegEncoder::new()` hardcodes quality 75. Must use `JpegEncoder::new_with_quality(&mut writer, 85)` to override. The quality parameter is `u8` (1-100). |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Rust edition**: 2024 (requires Rust >= 1.85; currently using 1.94.0)
- **Error handling**: All fallible public functions return `anyhow::Result<T>`; use `.context()` / `.with_context()` for error context
- **Code style**: `cargo fmt` before committing; `cargo clippy` must be clean; no `#[allow(...)]` annotations
- **Naming**: `snake_case` for functions/variables, `PascalCase` for types/enums, `SCREAMING_SNAKE_CASE` for constants
- **Imports**: Explicit imports only (no glob `use x::*`); group by blank lines
- **Documentation**: `//!` module doc comment; `///` on all public items; comments describe WHAT, not HOW
- **Function design**: Pass `&[u8]` for byte data, `&str` for string parameters; use `Option<T>` for optional returns
- **Module design**: Flat in `src/`; public functions exported with `pub`; internal helpers private
- **Testing**: `#[cfg(test)] mod tests {}` at bottom of each file; standard `cargo test`
- **Windows warning**: Do not output any data to `nul` (creates undeletable file on Windows)

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `image` | 0.25.10 | Decode all source formats (JPEG, PNG, GIF, BMP, TIFF, WebP, ICO); encode to JPEG and PNG | De facto standard Rust image library (8.6M downloads/month); supports all DOCX/EPUB image formats; pure Rust; actively maintained by image-rs org |
| `webp` | 0.3.1 | Lossy WebP encoding with quality control | Only production-ready lossy WebP encoder for Rust; wraps Google libwebp via libwebp-sys; integrates directly with `image::DynamicImage` via `Encoder::from_image()` |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `anyhow` | 1.0.82 (existing) | Error handling for decode/encode failures | Already in Cargo.toml; used by all public functions in the project |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `image` for decoding | RIL v0.10 | Does not support BMP or TIFF decoding -- both appear in DOCX files |
| `webp` for lossy WebP | `webpx` v0.1.4 | Newer but much lower adoption; `webp` crate API is simpler and covers the use case |
| `webp` for lossy WebP | `image` crate built-in | Lossless only -- files from JPEG sources would be larger, not smaller |
| `image::load_from_memory` | `ImageReader::with_guessed_format` | `ImageReader` offers more control but `load_from_memory` is simpler and sufficient for archive-extracted bytes |

**Installation:**
```toml
# In Cargo.toml [dependencies]
image = { version = "0.25", default-features = false, features = [
    "jpeg",  # JPEG decode/encode
    "png",   # PNG decode/encode
    "gif",   # GIF decode (for conversion source)
    "bmp",   # BMP decode (common in DOCX)
    "tiff",  # TIFF decode (occasional in DOCX/EPUB)
    "webp",  # WebP decode + lossless encode
    "ico",   # ICO decode (requires bmp + png, both already enabled)
] }

webp = "0.3"  # Lossy WebP encoding (wraps Google libwebp)
```

**Version verification:**
- `image` 0.25.10 -- confirmed current via `cargo search image` on 2026-04-02
- `webp` 0.3.1 -- confirmed current via `cargo search webp` on 2026-04-02
- Feature flag `ico` requires `bmp` and `png` features (both already listed)
- `default-features = false` omits: rayon, avif, exr, hdr, ff, pnm, qoi, tga, dds -- none appear in DOCX/EPUB

## Architecture Patterns

### Recommended Module Structure

```
src/
    convert.rs   # NEW: image format conversion (byte-in, byte-out)
    common.rs    # Existing: path safety, filename gen, write_image_to_file()
    docx.rs      # Existing: DOCX processing (unchanged in this phase)
    epub.rs      # Existing: EPUB processing (unchanged in this phase)
    main.rs      # Existing: CLI + orchestration (add `mod convert;` declaration only)
```

### Pattern 1: Three-Stage Conversion Pipeline

**What:** The `convert_image` function internally executes three stages: (1) format pre-check, (2) decode, (3) encode. Each stage has a clear responsibility and failure mode.

**When to use:** Always -- this is the core pattern for the module.

**Example:**
```rust
// Source: Verified against image 0.25.10 docs and webp 0.3.1 docs
use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;

pub fn convert_image(data: &[u8], format: OutputFormat, quality: u8) -> Result<Option<Vec<u8>>> {
    // Stage 1: Pre-check -- can we decode this?
    // (caller may have already checked via can_convert(), but this is defense-in-depth)
    let img = match image::load_from_memory(data) {
        Ok(img) => img,
        Err(e) => {
            // Check if this is an unsupported format vs corrupt data
            if is_unsupported_format_error(&e) {
                return Ok(None); // D-11: unsupported -> Ok(None)
            }
            return Err(e).context("Failed to decode image"); // D-12: corrupt -> Err
        }
    };

    // Stage 2: Alpha compositing (for JPEG target only)
    // Stage 3: Encode to target format
    encode_to_format(&img, format, quality)
}
```

### Pattern 2: Alpha Compositing Against White Background

**What:** Before JPEG encoding, check if the decoded image has an alpha channel. If so, create a white RGB image and blend each pixel using alpha compositing formula.

**When to use:** Only when target format is JPEG and source image has alpha (D-08, D-10).

**Example:**
```rust
// Source: Verified against image 0.25.10 DynamicImage API
use image::{DynamicImage, Rgb, RgbImage, Rgba};

/// Composites an RGBA image onto a white background, producing an RGB image.
fn composite_on_white(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgb = RgbImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let Rgba([r, g, b, a]) = *pixel;
        let alpha = a as f32 / 255.0;
        let inv_alpha = 1.0 - alpha;

        // Blend against white (255, 255, 255)
        let out_r = (r as f32 * alpha + 255.0 * inv_alpha) as u8;
        let out_g = (g as f32 * alpha + 255.0 * inv_alpha) as u8;
        let out_b = (b as f32 * alpha + 255.0 * inv_alpha) as u8;

        rgb.put_pixel(x, y, Rgb([out_r, out_g, out_b]));
    }

    DynamicImage::ImageRgb8(rgb)
}
```

**When to call:**
```rust
// Only composite when target is JPEG AND source has alpha
let img_for_encode = if matches!(format, OutputFormat::Jpg) && img.color().has_alpha() {
    composite_on_white(&img)
} else {
    img
};
```

### Pattern 3: Dual WebP Encoding Path

**What:** Lossy WebP uses the `webp` crate's `Encoder::from_image()`. Lossless WebP uses the `image` crate's built-in `write_to()` with `ImageFormat::WebP`.

**When to use:** WebP target format; the encoding path depends on whether lossless mode is requested (D-01, D-02).

**Example:**
```rust
// Source: Verified against webp 0.3.1 Encoder docs
fn encode_webp_lossy(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let encoder = webp::Encoder::from_image(img)
        .map_err(|e| anyhow::anyhow!("WebP encoder error: {}", e))?;
    let webp_data = encoder.encode(quality as f32);
    Ok(webp_data.to_vec())
}

// Source: Verified against image 0.25.10 write_to docs
fn encode_webp_lossless(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::WebP)
        .context("Failed to encode lossless WebP")?;
    Ok(buf.into_inner())
}
```

**Note on `webp::Encoder::from_image`:** Returns `Result<Self, &str>` (not `anyhow::Error`), so use `.map_err(|e| anyhow::anyhow!("..."))` to convert. The error occurs for unsupported color types (rare with `DynamicImage` input).

**Note on `WebPMemory`:** The `encode()` method returns `WebPMemory` which implements `Deref<Target=[u8]>`. Call `.to_vec()` to get an owned `Vec<u8>` for the return value.

### Pattern 4: Encoding to JPEG and PNG

**What:** Use explicit encoder constructors for full control over quality and output format.

**Example:**
```rust
// Source: Verified against image 0.25.10 JpegEncoder and PngEncoder docs
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality);
    img.write_with_encoder(encoder)
        .context("Failed to encode JPEG")?;
    Ok(buf)
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = PngEncoder::new(&mut buf);
    img.write_with_encoder(encoder)
        .context("Failed to encode PNG")?;
    Ok(buf)
}
```

**Critical:** Do NOT use `img.write_to(&mut buf, ImageFormat::Jpeg)` -- this uses the default quality of 75, not 85. Always use the explicit `JpegEncoder::new_with_quality()` path.

### Anti-Patterns to Avoid

- **Using `DynamicImage::save()`**: Infers format from file extension and uses default encoder settings. The `convert.rs` module has no filesystem access; `save()` is doubly wrong here.
- **Using `DynamicImage::to_rgb8()` for JPEG alpha handling**: This drops alpha without compositing against a background -- transparent pixels become black. Always use the explicit alpha compositing pattern.
- **Using `image::write_to()` for JPEG**: Uses default quality 75. Use `write_with_encoder(JpegEncoder::new_with_quality(..., 85))`.
- **Checking file extension to determine source format**: Archive images sometimes have wrong extensions. Use `load_from_memory()` which detects format from magic bytes.
- **Putting conversion logic in `common.rs`**: Conversion is a distinct responsibility. Keep `common.rs` focused on file I/O and path utilities.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Image format decoding | Custom format parsers | `image::load_from_memory()` | Handles 7+ formats with magic-byte detection; battle-tested against malformed inputs |
| JPEG encoding with quality | Manual JPEG compression | `image::codecs::jpeg::JpegEncoder::new_with_quality()` | JPEG is a complex standard with DCT, quantization tables, Huffman coding |
| PNG encoding | Manual PNG compression | `image::codecs::png::PngEncoder::new()` | PNG has DEFLATE compression, filtering, interlacing |
| Lossy WebP encoding | FFI bindings to libwebp | `webp::Encoder::from_image()` | Safe wrapper over libwebp; handles pixel format conversion internally |
| Format detection from bytes | Magic byte matching | `image::load_from_memory()` (built-in) | Supports all format magic bytes; handles edge cases (BOM, offset headers) |

**Key insight:** Image format conversion is deceptively complex. Even "simple" JPEG encoding involves DCT transforms, chroma subsampling, and quantization. The `image` crate encapsulates all of this behind a clean API. The only custom logic needed is alpha compositing (a per-pixel arithmetic operation) and format routing.

## Common Pitfalls

### Pitfall 1: RGBA-to-JPEG Produces Black Backgrounds

**What goes wrong:** The `image` crate's JPEG encoder only supports L8 (grayscale) and Rgb8 color types. When given an RGBA image, its internal `make_compatible_img()` converts RGBA to Rgb8 by discarding the alpha channel. Transparent pixels (alpha=0) become black, not white. This affects every PNG, GIF, and WebP source image that has transparency.

**Why it happens:** The conversion treats alpha=0 as "the pixel color doesn't matter" and defaults to black (the zero-initialized color). Users expect transparent areas to become white (the convention for document-extracted images).

**How to avoid:** Before JPEG encoding, check `img.color().has_alpha()`. If true, run the alpha compositing pattern (blend each pixel against white using the alpha channel). This is required by D-08 and CONV-02.

**Warning signs:** Any converted JPEG with black rectangles where logos, diagrams, or illustrations had transparent backgrounds.

### Pitfall 2: JPEG Default Quality of 75 Produces Visible Artifacts

**What goes wrong:** `JpegEncoder::new()` defaults to quality 75. Re-encoding document-extracted images at quality 75 introduces visible compression artifacts, especially on text-heavy images (screenshots, scanned pages) and images with sharp edges.

**Why it happens:** The constructor hardcodes `new_with_quality(w, 75)`. The `write_to()` method with `ImageFormat::Jpeg` also uses this default.

**How to avoid:** Always use `JpegEncoder::new_with_quality(&mut buf, quality)` where quality defaults to 85 (D-14, CONV-05). Never use the `new()` constructor or `write_to()` for JPEG.

**Warning signs:** Blocky artifacts around text and sharp edges in converted JPEG files.

### Pitfall 3: WebP Lossless Produces Larger Files Than JPEG Sources

**What goes wrong:** The `image` crate's built-in WebP encoder (`image-webp`) is lossless only. Converting a JPEG to lossless WebP produces files 2-5x larger than the source, defeating the purpose of "convert to WebP for smaller files."

**Why it happens:** Lossless preserves every pixel exactly. A lossy source (JPEG) re-encoded losslessly cannot be smaller.

**How to avoid:** Default to lossy WebP via the `webp` crate (D-01). Only use lossless when explicitly requested via `--lossless` flag (D-02, implemented in a later phase). The `convert_image` function needs a way to signal lossy vs lossless -- this phase should include a `lossless: bool` parameter or handle it through the quality parameter (quality=100 could mean lossless, but this is ambiguous).

**Warning signs:** WebP output files that are larger than the source JPEG files.

### Pitfall 4: `webp::Encoder::from_image` Returns `Result<Self, &str>` Not `anyhow::Error`

**What goes wrong:** The `webp` crate's error type is `&str`, not a type implementing `std::error::Error`. Using `?` directly won't work with `anyhow::Result`.

**Why it happens:** The `webp` crate uses a simpler error model than the `image` crate.

**How to avoid:** Convert with `.map_err(|e| anyhow::anyhow!("WebP encoder error: {}", e))` before using `?`. This follows the existing codebase pattern for the `epub` crate's string errors (see `src/epub.rs` line 60).

**Warning signs:** Compile error about `&str` not implementing `Into<anyhow::Error>`.

### Pitfall 5: `load_from_memory` Cannot Detect TGA Format

**What goes wrong:** `image::load_from_memory()` cannot detect TGA format because TGA has no magic bytes. However, TGA images are essentially never found in DOCX/EPUB archives.

**Why it happens:** TGA is a headerless format; format detection from magic bytes is impossible.

**How to avoid:** This is a non-issue for this project. TGA is not in the supported extensions list and does not appear in document archives. No action needed.

**Warning signs:** None expected.

### Pitfall 6: Animated GIFs Lose All Frames When Decoded

**What goes wrong:** `image::load_from_memory()` returns only the first frame of an animated GIF as a `DynamicImage`. Converting an animated GIF to any format silently destroys the animation.

**Why it happens:** `DynamicImage` is a single-image container. Multi-frame GIFs require the `GifDecoder` + `into_frames()` API.

**How to avoid:** For this phase (conversion module core), accept first-frame behavior. The module is byte-in, byte-out and has no concept of animation. The caller (integration phases 5-6) will need to decide whether to skip animated GIF conversion. Document this limitation in the module's doc comments.

**Warning signs:** Animated GIF converted to static PNG/JPEG/WebP.

## Code Examples

Verified patterns from official sources:

### OutputFormat Enum Definition

```rust
// Follows project convention: PascalCase for enum, derive Debug + Clone
/// Target format for image conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// JPEG output (lossy, quality 1-100)
    Jpg,
    /// PNG output (lossless, preserves transparency)
    Png,
    /// WebP output (lossy by default, lossless optional)
    Webp,
}
```

### can_convert Function

```rust
// Source: Verified against image 0.25.10 supported formats list
/// Checks whether a source image extension can be decoded for conversion.
///
/// Returns `true` for formats the `image` crate can decode: jpg, jpeg, png,
/// gif, bmp, tiff, tif, webp, ico. Returns `false` for SVG, WMF, EMF, and
/// other unsupported formats.
pub fn can_convert(extension: &str) -> bool {
    matches!(
        extension.to_lowercase().as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "tif" | "webp" | "ico"
    )
}
```

### Decoding with Error Classification

```rust
// Source: Verified against image 0.25.10 load_from_memory docs
use image::ImageError;

fn decode_image(data: &[u8]) -> Result<Option<DynamicImage>> {
    match image::load_from_memory(data) {
        Ok(img) => Ok(Some(img)),
        Err(ImageError::Unsupported(_)) => Ok(None), // D-11
        Err(e) => Err(e).context("Failed to decode image"), // D-12
    }
}
```

**Note:** `ImageError::Unsupported` is the variant returned for unknown/unsupported formats. This provides a clean way to distinguish "can't decode this format" from "data is corrupt."

### Complete JPEG Encode with Alpha Compositing

```rust
// Source: Verified against image 0.25.10 JpegEncoder + DynamicImage APIs
fn encode_as_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    // D-08, D-10: Composite alpha against white for JPEG
    let img_for_encode = if img.color().has_alpha() {
        composite_on_white(img)
    } else {
        img.clone()
    };

    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut buf, quality); // D-14: quality 85
    img_for_encode
        .write_with_encoder(encoder)
        .context("Failed to encode JPEG")?;
    Ok(buf)
}
```

### Complete WebP Lossy Encode

```rust
// Source: Verified against webp 0.3.1 Encoder docs
fn encode_as_webp_lossy(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let encoder = webp::Encoder::from_image(img)
        .map_err(|e| anyhow::anyhow!("WebP encoder creation failed: {}", e))?;
    let webp_data = encoder.encode(quality as f32); // D-15: quality 85.0
    Ok(webp_data.to_vec()) // WebPMemory -> Vec<u8>
}
```

### Unit Test: Programmatic Test Image Generation

```rust
// No external test images needed -- generate them in-memory
#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    /// Creates a small RGBA PNG with a transparent region for testing
    fn create_test_rgba_png() -> Vec<u8> {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));     // red, opaque
        img.put_pixel(1, 0, Rgba([0, 255, 0, 128]));     // green, 50% alpha
        img.put_pixel(0, 1, Rgba([0, 0, 255, 0]));       // blue, fully transparent
        img.put_pixel(1, 1, Rgba([255, 255, 255, 255])); // white, opaque

        let dynamic = DynamicImage::ImageRgba8(img);
        let mut buf = Cursor::new(Vec::new());
        dynamic.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// Creates a small RGB JPEG for testing (no alpha)
    fn create_test_rgb_jpeg() -> Vec<u8> {
        use image::RgbImage;
        let mut img = RgbImage::new(4, 4);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x * 60) as u8, (y * 60) as u8, 128]);
        }
        let dynamic = DynamicImage::ImageRgb8(img);
        let mut buf = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
        dynamic.write_with_encoder(encoder).unwrap();
        buf
    }

    /// Creates a small BMP for testing
    fn create_test_bmp() -> Vec<u8> {
        use image::RgbImage;
        let img = RgbImage::new(4, 4);
        let dynamic = DynamicImage::ImageRgb8(img);
        let mut buf = Cursor::new(Vec::new());
        dynamic.write_to(&mut buf, ImageFormat::Bmp).unwrap();
        buf.into_inner()
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `image` 0.24 WebP support | `image` 0.25 with `image-webp` | 2024 | WebP decode + lossless encode built in; lossy still requires `webp` crate |
| `DynamicImage::save()` for output | `write_with_encoder()` for controlled encoding | Recommended since 0.24 | Full control over encoder settings (quality, compression) |
| `image::open()` for file decoding | `load_from_memory()` for byte decoding | Always available | Archive-extracted bytes never touch filesystem; `load_from_memory` is the natural fit |
| `webp` crate 0.2.x | `webp` 0.3.1 | 2024 | `from_image()` added for direct `DynamicImage` integration |

**Deprecated/outdated:**
- `image::save_buffer()`: Low-level; prefer `DynamicImage::write_with_encoder()` for type safety
- `image 0.24` WebP: Incomplete; 0.25 has proper support

## Open Questions

1. **Lossless WebP API Design**
   - What we know: D-02 says lossless WebP is activated by `--lossless` flag. The `convert_image` signature (D-04) takes `quality: u8` but lossless has no quality parameter.
   - What's unclear: Should `convert_image` take an additional `lossless: bool` parameter, or should the caller handle lossless encoding separately?
   - Recommendation: Add a `lossless: bool` parameter to `convert_image`. When `lossless && format == Webp`, use the `image` crate's lossless encoder. When `!lossless`, use the `webp` crate's lossy encoder. The `quality` parameter is ignored for lossless. This keeps the API simple and avoids splitting the caller's logic.

2. **Quality Parameter for Non-JPEG/Non-WebP Targets**
   - What we know: PNG is lossless and has no quality parameter. The `quality: u8` parameter in D-04 applies to JPEG and lossy WebP.
   - What's unclear: Should `convert_image` silently ignore the quality parameter for PNG, or should it warn?
   - Recommendation: Silently ignore. The caller controls which quality value to pass; the function signature is intentionally uniform. Document in the doc comment that quality only applies to JPEG and lossy WebP.

3. **`webp` Crate Behavior with Grayscale/Palette Images**
   - What we know: STATE.md flags "webp crate error handling with unusual color spaces (grayscale, palette-indexed) needs verification."
   - What's unclear: Does `Encoder::from_image()` handle `DynamicImage::ImageLuma8` or indexed-color variants?
   - Recommendation: Test during implementation. If `from_image()` fails for grayscale, convert to RGB first with `img.to_rgb8()` before passing to the WebP encoder. This is a LOW-risk issue since most document images are RGB/RGBA.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust compiler | Build | Yes | 1.94.0 | -- |
| Cargo | Build | Yes | 1.94.0 | -- |
| MSVC toolchain | `webp` crate C compilation | Yes | VS 2022 | -- |
| C compiler (cl.exe) | `libwebp-sys` build | Yes (via VS 2022, not in bash PATH but `cc` crate auto-detects) | MSVC | -- |

**Missing dependencies with no fallback:** None

**Missing dependencies with fallback:** None

**Note:** `cl.exe` is not in the bash shell PATH but the `cc` crate (used by `libwebp-sys`) locates MSVC via registry/vcvars detection. Cargo builds on Windows MSVC target handle this automatically. Verified: Rust target is `x86_64-pc-windows-msvc`.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Built-in Rust test harness (`#[cfg(test)]`, `cargo test`) |
| Config file | None needed (standard `cargo test`) |
| Quick run command | `cargo test --lib convert` |
| Full suite command | `cargo test` |

### Phase Requirements to Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONV-02 | RGBA PNG converted to JPEG has white background where transparency was (not black) | unit | `cargo test --lib convert::tests::test_jpeg_alpha_compositing_white_background -x` | Wave 0 |
| CONV-02 | Fully opaque PNG converted to JPEG has no alpha compositing step (passthrough) | unit | `cargo test --lib convert::tests::test_jpeg_opaque_no_compositing -x` | Wave 0 |
| CONV-05 | JPEG output uses quality 85 (verified by checking output differs from quality 75) | unit | `cargo test --lib convert::tests::test_jpeg_quality_85 -x` | Wave 0 |
| D-04 | convert_image returns Vec<u8> for valid inputs | unit | `cargo test --lib convert::tests::test_convert_returns_bytes -x` | Wave 0 |
| D-06 | can_convert returns true for decodable formats, false for SVG/WMF/EMF | unit | `cargo test --lib convert::tests::test_can_convert -x` | Wave 0 |
| D-11 | convert_image returns Ok(None) for unsupported format bytes | unit | `cargo test --lib convert::tests::test_unsupported_format_returns_none -x` | Wave 0 |
| D-12 | convert_image returns Err for corrupt/garbage bytes | unit | `cargo test --lib convert::tests::test_corrupt_data_returns_err -x` | Wave 0 |
| SC-5 | All source-to-target format combinations work | unit | `cargo test --lib convert::tests::test_format_matrix -x` | Wave 0 |
| SC-3 | WebP lossy output is smaller than lossless for photographic content | unit | `cargo test --lib convert::tests::test_webp_lossy_smaller_than_lossless -x` | Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test --lib convert`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/convert.rs` `#[cfg(test)] mod tests` -- all tests listed above (file does not exist yet)
- [ ] Test image generation helpers -- create PNG/JPEG/BMP/GIF/TIFF/WebP test images programmatically (no external test fixtures needed)

## Sources

### Primary (HIGH confidence)
- [image crate docs.rs](https://docs.rs/image/0.25.10/image/) -- DynamicImage API, JpegEncoder API, PngEncoder API, ImageFormat enum, load_from_memory, color types
- [image crate Cargo.toml (GitHub)](https://github.com/image-rs/image/blob/main/Cargo.toml) -- Feature flag definitions, ICO requires bmp+png
- [image crate JpegEncoder source (GitHub)](https://github.com/image-rs/image/blob/main/src/codecs/jpeg/encoder.rs) -- Confirmed default quality 75, supported color types L8/Rgb8
- [webp crate docs.rs](https://docs.rs/webp/0.3.1/webp/) -- Encoder::from_image, encode(quality: f32), WebPMemory Deref<[u8]>
- [image-webp GitHub](https://github.com/image-rs/image-webp) -- Confirmed lossless-only encoding

### Secondary (MEDIUM confidence)
- [crates.io cargo search](https://crates.io/) -- Version verification: image 0.25.10, webp 0.3.1 (2026-04-02)

### Tertiary (LOW confidence)
- `webp` crate behavior with grayscale/palette `DynamicImage` variants -- needs runtime verification during implementation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- `image` and `webp` crates are well-established, APIs verified against docs.rs
- Architecture: HIGH -- module design follows existing codebase patterns, locked by user decisions
- Pitfalls: HIGH -- alpha compositing and JPEG quality issues verified against source code; WebP lossless limitation confirmed in docs

**Research date:** 2026-04-02
**Valid until:** 2026-05-02 (stable crates, unlikely to change in 30 days)
