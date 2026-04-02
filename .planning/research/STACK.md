# Technology Stack

**Project:** Word/EPUB Image Extractor -- Conversion & GIF Features
**Researched:** 2026-04-01

## Executive Summary

The `image` crate (v0.25.10) is the clear choice for image format conversion in this project. It handles decoding all formats found in DOCX/EPUB archives (JPEG, PNG, GIF, BMP, TIFF, WebP) and encoding to JPEG and PNG with quality control. However, its WebP encoder only supports lossless encoding. For lossy WebP output (smaller files, which is the entire point of converting to WebP), the `webp` crate (v0.3.1) must supplement it. This two-crate approach is standard in the Rust ecosystem and well-proven.

## Recommended Stack

### Image Decoding & Core Conversion

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| `image` | 0.25.10 | Decode any input format, encode to JPEG/PNG | De facto standard (8.6M downloads/month), supports all DOCX/EPUB image formats, pure Rust, actively maintained by image-rs org | HIGH |

**Cargo.toml addition:**
```toml
image = { version = "0.25", default-features = false, features = ["jpeg", "png", "gif", "bmp", "tiff", "webp"] }
```

**Rationale for selective features:** The default feature set includes AVIF, EXR, HDR, QOI, TGA, PNM, farbfeld, and rayon -- none of which appear in DOCX/EPUB files. Disabling them cuts compile time meaningfully (the `image` crate is one of the slower crates to compile). Enable only the six formats actually encountered in document archives.

**Note on `rayon`:** The default `rayon` feature enables multi-threaded decoding for some formats. Omit it. This is a sequential CLI tool processing one image at a time; rayon adds compile time and a thread pool for no benefit.

### Lossy WebP Encoding

| Technology | Version | Purpose | Why | Confidence |
|------------|---------|---------|-----|------------|
| `webp` | 0.3.1 | Lossy WebP encoding with quality control | Only way to get lossy WebP in Rust; wraps Google's libwebp via libwebp-sys; 360K downloads/month; integrates directly with `image::DynamicImage` | HIGH |

**Cargo.toml addition:**
```toml
webp = "0.3"
```

**Critical context:** The `image` crate's built-in WebP encoder (`image-webp`) only supports **lossless** encoding. The image-webp maintainers explicitly state: "This crate only supports lossless encoding. If you need lossy encoding, you'll have to use libwebp." Lossless WebP produces files often larger than the original PNG, defeating the purpose of `--convert webp`. The `webp` crate provides `Encoder::from_image(dynamic_image).encode(quality)` for lossy encoding with a 0-100 quality parameter.

**Build requirement:** The `webp` crate depends on `libwebp-sys`, which compiles Google's C library via the `cc` crate. This requires a C compiler (MSVC on Windows, which this project already has since it's building Rust with MSVC target). No cmake needed. Builds cleanly on Windows 11 with the standard Rust/MSVC toolchain.

### No Additional Dependencies Needed

The existing stack handles everything else:

| Existing Dep | Role in Conversion Feature |
|--------------|---------------------------|
| `clap` 4.5 | Already handles `--convert`, `--gif-only`, `--gif-output` args (just add new args) |
| `zip` 2.1 | Already extracts raw image bytes from archives |
| `anyhow` 1.0 | Already provides error context for decode/encode failures |
| `indicatif` 0.18 | Already provides progress bars (conversion adds a step to existing progress) |
| `walkdir` 2.5 | No change needed |
| `epub` 2.1 | No change needed |

## Alternatives Considered

### Full Alternatives to `image` Crate

| Alternative | Why Not | Confidence |
|-------------|---------|------------|
| **RIL** (Rust Imaging Library) v0.10 | Does not support BMP or TIFF decoding -- both appear in DOCX files. WebP support via libwebp-sys (same C dep as `webp` crate). Much smaller ecosystem (not widely adopted). Adds font rendering and gradient dependencies we don't need. | HIGH |
| **Photon** | High-level wrapper around the `image` crate -- adds a dependency layer for no benefit in a decode-then-encode workflow. Focused on image manipulation (filters, effects), not format conversion. | HIGH |
| **Kornia-rs** | Computer vision library (3D, real-time). Massive overkill for format conversion. Focused on image transformations, not codec work. | HIGH |
| **OpenCV bindings** | Requires system OpenCV installation. Enormous dependency for simple format conversion. Cross-platform build story is painful. | HIGH |

### WebP Encoding Alternatives

| Alternative | Why Not | Confidence |
|-------------|---------|------------|
| **image-webp** (pure Rust, used by `image` crate) | **Lossless only.** Explicitly does not support lossy encoding. Unsuitable for `--convert webp` where users expect smaller files. | HIGH |
| **webpx** v0.2 | Newer (Feb 2026), more complete API, but much lower adoption than `webp` crate. Fewer downloads, less battle-tested. The `webp` crate's simpler API covers our needs. Could revisit if `webp` crate becomes unmaintained. | MEDIUM |
| **zenwebp** | Fork of image-webp with added lossy encoding. Niche, low adoption, unclear maintenance trajectory. The `webp` crate is more established. | MEDIUM |
| **Direct libwebp-sys** | Raw unsafe FFI bindings. The `webp` crate provides safe wrappers over exactly this. No reason to go lower-level. | HIGH |

## Key API Patterns

### Decode from Archive Bytes

```rust
use image::load_from_memory;

// bytes: Vec<u8> extracted from ZIP entry
let img = image::load_from_memory(&bytes)?;  // Auto-detects format from magic bytes
```

`load_from_memory` uses magic-byte detection, which is exactly right for archive-extracted images where the file extension in the ZIP path may be unreliable.

### Encode to JPEG

```rust
use image::codecs::jpeg::JpegEncoder;

let mut output = Vec::new();
let encoder = JpegEncoder::new_with_quality(&mut output, 85); // quality: 1-100
img.write_with_encoder(encoder)?;
```

### Encode to PNG

```rust
use image::codecs::png::PngEncoder;

let mut output = Vec::new();
let encoder = PngEncoder::new(&mut output); // PNG is lossless, no quality param needed
img.write_with_encoder(encoder)?;
```

### Encode to WebP (Lossy)

```rust
use webp::Encoder;

let encoder = Encoder::from_image(&img).expect("unsupported color type");
let webp_data = encoder.encode(85.0); // quality: 0.0-100.0
// webp_data derefs to &[u8]
```

**Note:** The `webp` crate's `Encoder::from_image` takes a reference to `image::DynamicImage` directly -- the two crates integrate cleanly.

## Format Support Matrix

What the `image` crate can decode (relevant to DOCX/EPUB content):

| Format | Decode | Encode | Notes |
|--------|--------|--------|-------|
| JPEG | Yes | Yes (quality 1-100) | Primary target format |
| PNG | Yes | Yes (compression level, filter) | Primary target format |
| GIF | Yes | Yes | Decode-only needed for conversion; GIF extraction is separate |
| BMP | Yes | Yes | Common in older DOCX files |
| TIFF | Yes | Yes | Occasional in DOCX/EPUB |
| WebP | Yes | Lossless only | Use `webp` crate for lossy encoding |
| SVG | **No** | No | Vector format; cannot be rasterized by `image` crate |
| WMF | **No** | No | Windows metafile; no Rust decoder exists |
| EMF | **No** | No | Enhanced metafile; no Rust decoder exists |

**Unsupported formats (SVG, WMF, EMF):** These appear occasionally in DOCX files. The conversion pipeline should skip them with a warning message, extracting the raw file as-is without conversion. This matches the PROJECT.md constraint: "WMF/EMF/SVG can't be decoded by image crate; warn and extract as-is."

## Installation

```toml
# In Cargo.toml [dependencies]

# Image decoding (all DOCX/EPUB formats) and JPEG/PNG encoding
image = { version = "0.25", default-features = false, features = [
    "jpeg",  # JPEG decode/encode
    "png",   # PNG decode/encode  
    "gif",   # GIF decode (for conversion source)
    "bmp",   # BMP decode (common in DOCX)
    "tiff",  # TIFF decode (occasional in DOCX/EPUB)
    "webp",  # WebP decode (lossless encode only -- lossy via webp crate)
] }

# Lossy WebP encoding (wraps Google's libwebp)
webp = "0.3"
```

**Total new dependencies added: 2** (plus their transitive deps). The `image` crate pulls in pure-Rust codec crates. The `webp` crate pulls in `libwebp-sys` which compiles C code at build time.

## Build Impact

| Concern | Impact | Mitigation |
|---------|--------|------------|
| Compile time | `image` crate is moderately slow to compile (~30-60s first build) | Selective features cut this significantly; incremental builds fast after first |
| Binary size | Adds codec implementations; moderate increase | Already using `strip = true` and `lto = true` in release profile |
| C compiler requirement | `webp` crate needs MSVC C compiler on Windows | Already present in this project's toolchain (Rust MSVC target) |
| Cross-compilation | `libwebp-sys` auto-builds bundled C source for target | Works with standard cross toolchains; no system lib needed |

## Version Pinning Strategy

Pin to minor version (`0.25`, `0.3`) not patch version. The `image` crate follows semver strictly within 0.25.x. The `webp` crate is stable at 0.3.x.

Do not pin to exact versions (e.g., `=0.25.10`) -- this prevents receiving bugfix patches and creates dependency resolution conflicts.

## Sources

- [image crate on crates.io](https://crates.io/crates/image) -- v0.25.10, 8.6M downloads/month
- [image crate docs](https://docs.rs/image/latest/image/) -- API reference, format support
- [image crate CHANGES.md](https://docs.rs/crate/image/latest/source/CHANGES.md) -- v0.25.10 changelog
- [image-webp on GitHub](https://github.com/image-rs/image-webp) -- confirms lossless-only encoding
- [WebPEncoder docs](https://docs.rs/image/latest/image/codecs/webp/struct.WebPEncoder.html) -- "only lossless encoding"
- [webp crate on lib.rs](https://lib.rs/crates/webp) -- v0.3.1, 360K downloads/month
- [webp Encoder docs](https://docs.rs/webp/0.3.1/webp/struct.Encoder.html) -- lossy quality API
- [image crate Cargo.toml on GitHub](https://github.com/image-rs/image/blob/main/Cargo.toml) -- feature flag definitions
- [RIL on GitHub](https://github.com/jay3332/ril) -- v0.10, no BMP/TIFF
- [Rust forum: image processing recommendations](https://users.rust-lang.org/t/looking-for-image-processing-crate-recommendation-introduction/123968)
