# Domain Pitfalls

**Domain:** Rust image format conversion in a DOCX/EPUB extraction pipeline
**Researched:** 2026-04-01

## Critical Pitfalls

Mistakes that cause rewrites, data corruption, or user-visible breakage.

### Pitfall 1: RGBA-to-JPEG Alpha Channel Loss Produces Black/Corrupt Backgrounds

**What goes wrong:** PNG, GIF, BMP, ICO, and WebP images inside DOCX/EPUB files frequently contain an alpha (transparency) channel (RGBA). JPEG does not support transparency. When you call `DynamicImage::save("output.jpg")`, the `image` crate silently converts RGBA to RGB by dropping the alpha channel. The automatic conversion composes transparent pixels as black, producing images with black rectangles where transparency existed (e.g., logos with transparent backgrounds, diagrams on transparent canvases).

**Why it happens:** The `image` crate's JPEG encoder only supports `L8` (grayscale) and `Rgb8` color types. Its internal `make_compatible_img()` converts `Rgba8` to `Rgb8` automatically, but this conversion treats alpha=0 pixels as black (it discards alpha without compositing against a background color). Users expect transparent areas to become white (the common convention for document images).

**Consequences:** Every PNG/GIF with transparency extracted from DOCX/EPUB and converted to JPEG will have black patches where transparency was. This is the single most visible conversion defect.

**Prevention:**
1. Before JPEG encoding, check if the decoded `DynamicImage` is an RGBA variant.
2. If it is, composite against a white background before converting to `Rgb8`.
3. Implementation: create a white `RgbImage`, iterate pixels, alpha-blend each RGBA pixel onto white, then encode the resulting RGB image.
4. Alternatively, use `DynamicImage::to_rgb8()` explicitly and accept the black-background behavior, but document it -- this is almost certainly not what users want.

**Detection:** Test with any PNG that has transparency (common in DOCX diagrams and EPUB illustrations). If converted JPEG has black areas where transparency was, this pitfall was hit.

**Confidence:** HIGH -- verified via `image` crate source code showing `Rgba8 -> Rgb8` conversion path and JPEG encoder's `make_compatible_img()`.

**Phase:** Must be addressed in the core conversion implementation phase. This is not a polish item -- it affects the visual correctness of every transparent image.

---

### Pitfall 2: WebP Encoding is Lossless-Only -- Files May Be Larger Than Source

**What goes wrong:** The `image` crate (via `image-webp` v0.2.x) only supports **lossless** WebP encoding. When a user runs `--convert webp`, they likely expect smaller files (WebP's marketing is "25-34% smaller than JPEG"). Instead, converting a JPEG to lossless WebP produces files that are **larger** than the original JPEG, sometimes significantly so. This defeats the purpose of converting to WebP.

**Why it happens:** Lossless WebP preserves every pixel exactly, so it cannot be smaller than a lossy source format (JPEG). The `image-webp` crate explicitly states: "This crate only supports lossless encoding" and "adding lossy support is a non-trivial task." There is no quality parameter exposed.

**Consequences:**
- JPEG-to-WebP: output files are larger (often 2-5x larger).
- PNG-to-WebP: output files are typically smaller (lossless-to-lossless, WebP compresses better). This is the one case that works as expected.
- BMP/TIFF-to-WebP: output files are smaller (uncompressed-to-lossless).
- Users who specifically chose WebP for size savings from JPEG sources will be confused and disappointed.

**Prevention:**
1. **Document the limitation** clearly in `--help` text and warnings: "WebP conversion uses lossless encoding; files converted from JPEG may be larger than originals."
2. **Consider emitting a warning** when the source format is JPEG and the target is WebP, informing the user about expected size increase.
3. **Alternative (future):** The `webp` crate (libwebp bindings) supports lossy encoding with quality control. This could be added as an optional dependency behind a feature flag if users demand lossy WebP. However, it requires a native C library (libwebp-sys), which complicates cross-compilation and the pure-Rust story.
4. **Do not** try to work around this by re-compressing -- lossless is the only option available in the `image` crate ecosystem today.

**Detection:** Convert any JPEG file to WebP and compare file sizes. If WebP is larger, this is the expected (unfortunate) behavior.

**Confidence:** HIGH -- confirmed via `image-webp` documentation: "This crate only supports lossless encoding."

**Phase:** Should be addressed at the CLI/UX phase (warnings, documentation). The underlying limitation cannot be fixed without switching WebP encoding backends.

---

### Pitfall 3: SVG, WMF, and EMF Cannot Be Decoded -- Silent Failures in Conversion Pipeline

**What goes wrong:** DOCX files commonly contain WMF and EMF (Windows Metafile) images for vector graphics like SmartArt, charts, and clipart. EPUB files occasionally contain SVG. The `image` crate cannot decode any of these three formats. If the conversion pipeline attempts `image::load_from_memory()` on WMF/EMF/SVG bytes, it will return an error (not a panic, but a failure). If this error is not handled per-file, it could abort the entire batch.

**Why it happens:** WMF and EMF are Windows GDI command sequences (vector drawing instructions), not raster pixel data. SVG is XML-based vector markup. The `image` crate is a raster image library and has no vector rendering engine. These formats require entirely different parsing and rendering stacks.

**Consequences:**
- If the error propagates with `?`, a single WMF/EMF in a DOCX file aborts processing of that entire document.
- If errors are swallowed silently, users don't know why some images are missing from the output.
- WMF/EMF are very common in older DOCX files and corporate templates.

**Prevention:**
1. **Before attempting conversion**, check the source file extension against a list of unconvertible formats: `svg`, `wmf`, `emf`.
2. **Skip unconvertible files** with a clear warning: "Skipping {filename}: {format} format cannot be converted. Extracting original file instead."
3. **Extract the original bytes** to disk (the existing behavior) -- do not attempt decode/encode.
4. The PROJECT.md already identifies this: "WMF/EMF/SVG can't be decoded by image crate; warn and extract as-is." Implement this exactly.

**Detection:** Process any DOCX file created with Microsoft Office that contains SmartArt, charts, or inserted clip art -- these frequently embed WMF/EMF alongside PNG fallbacks.

**Confidence:** HIGH -- the `image` crate's supported format list does not include SVG, WMF, or EMF in any capacity.

**Phase:** Must be addressed in the core conversion implementation phase, specifically in the format-routing logic before the decode step.

---

### Pitfall 4: Animated GIFs Lose All Frames When Decoded as DynamicImage

**What goes wrong:** The `image::load_from_memory()` / `DynamicImage` path decodes only the **first frame** of an animated GIF. If a user extracts a GIF from a DOCX file and then converts it to PNG or WebP, they get a single static frame -- the animation is destroyed. This is especially bad because the project has a `--gif-only` mode, implying users care about GIFs specifically.

**Why it happens:** `DynamicImage` represents a single image. Animated GIFs contain multiple frames with timing and disposal metadata. The standard decode path (`image::open()`, `load_from_memory()`) returns only the first frame. Accessing all frames requires `GifDecoder` + `into_frames()` from the `image::codecs::gif` module.

**Consequences:**
- Converting an animated GIF to any format silently destroys the animation.
- Since `--gif-only` and `--convert` are mutually exclusive per the PROJECT.md, the conversion path should not apply to GIFs in `--gif-only` mode. However, if a user runs `--convert png` on a directory, animated GIFs will be silently converted to single-frame PNGs.

**Prevention:**
1. **When `--convert` is active and source is GIF:** detect whether the GIF is animated (has multiple frames). If animated, emit a warning and either:
   - (a) Skip conversion and extract the original GIF, or
   - (b) Convert only the first frame with a warning: "Animated GIF converted to single frame."
2. **Do not silently convert animated GIFs to static images** without user awareness.
3. Detection of animation: use `GifDecoder::new(reader)` then check if `into_frames()` yields more than one frame, or simply check the frame count from the decoder metadata.

**Detection:** Test with any animated GIF. If the output is a static image with no warning, the pitfall was hit.

**Confidence:** HIGH -- this is fundamental to how `DynamicImage` works; it is a single-image container.

**Phase:** Should be addressed in the conversion implementation phase. The `--gif-only` and `--convert` mutual exclusivity already partially mitigates this (GIF-only mode does not convert), but conversion mode still needs to handle GIF sources gracefully.

---

## Moderate Pitfalls

### Pitfall 5: JPEG Default Quality of 75 May Produce Visible Artifacts

**What goes wrong:** When using `DynamicImage::save("output.jpg")`, the `image` crate's `JpegEncoder` defaults to quality 75 (out of 100). For photographic images extracted from documents, quality 75 introduces visible JPEG compression artifacts, especially on text-heavy images (screenshots, scanned pages) and images with sharp edges (diagrams, logos).

**Why it happens:** `JpegEncoder::new()` internally calls `new_with_quality(w, 75)`. The `save()` method constructs a default encoder with no way to pass quality. Users have no control over JPEG quality through the simple `save()` API.

**Prevention:**
1. **Use `write_with_encoder(JpegEncoder::new_with_quality(&mut writer, quality))`** instead of `save()` for JPEG output.
2. **Default to quality 85-90** for document image extraction (these images are often already compressed; re-encoding at 75 compounds quality loss).
3. **Consider exposing a `--quality` CLI flag** (deferred per PROJECT.md "Out of Scope" for now, but architect the code to accept a quality parameter internally so it can be added later without refactoring).

**Detection:** Convert a PNG screenshot or diagram to JPEG and visually inspect. Blocky artifacts around text and edges indicate quality is too low.

**Confidence:** HIGH -- verified from `image` crate source: `JpegEncoder::new()` hardcodes quality 75.

**Phase:** Conversion implementation phase. Use `write_with_encoder` instead of `save()` for JPEG, even if the quality is still hardcoded at a higher default.

---

### Pitfall 6: File Extension Mismatch Between Source and Actual Format

**What goes wrong:** Images inside DOCX/EPUB archives sometimes have incorrect file extensions. A file named `image1.jpg` might actually contain PNG data (mismatched magic bytes). If you use the file extension to determine the source format for conversion, you may pass the wrong format hint to the decoder, causing decode failures.

**Why it happens:** Office applications and EPUB generators don't always set correct extensions. The archive just stores bytes with whatever filename the producer chose.

**Consequences:**
- `image::load_from_memory_with_format(bytes, ImageFormat::Jpeg)` will fail on a PNG file with a `.jpg` extension.
- Using `load_from_memory()` (guesses from magic bytes) avoids this, but is slightly slower and cannot detect TGA format.

**Prevention:**
1. **Use `image::load_from_memory()` or `ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.decode()?`** for decoding. Let the `image` crate detect format from magic bytes, not from the file extension.
2. **Use `load_from_memory_with_format()` only as a fallback** if the guessed format fails and you want to try the extension-based format.
3. Since the existing codebase already reads raw bytes from archives, `load_from_memory()` is the natural fit.

**Detection:** Intentionally test with a PNG file renamed to `.jpg` inside a DOCX archive. If decoding fails, extension-based format detection is being used.

**Confidence:** MEDIUM -- the issue is well-known in image processing; frequency in real DOCX/EPUB files is moderate but non-zero.

**Phase:** Conversion implementation phase, in the decode step.

---

### Pitfall 7: Output File Extension Must Match Target Format (Not Source)

**What goes wrong:** The existing codebase generates output filenames using the source image's extension (e.g., `document_1.png`). When conversion is active, the output filename must use the target format's extension (e.g., `document_1.jpg` for `--convert jpg`). If the code writes JPEG-encoded bytes to a file named `.png`, image viewers may fail to open it or display it incorrectly.

**Why it happens:** The current `get_unique_output_path()` function takes the source extension as a parameter. When conversion is added, this must be replaced with the target extension.

**Consequences:**
- Files with wrong extensions confuse image viewers, file managers, and downstream tools.
- If `DynamicImage::save()` is used (which infers format from extension), a `.png` extension would cause PNG encoding instead of JPEG encoding, defeating the entire conversion feature.

**Prevention:**
1. **When `--convert` is active**, always use the target format extension for output filenames, regardless of the source format.
2. **When `--convert` is not active**, preserve the original extension (existing behavior).
3. **Use `write_to()` with an explicit `ImageFormat`** rather than `save()`, so the output format is determined by the `--convert` flag, not by the filename extension.

**Detection:** Run `--convert jpg` on a PNG source and check that the output file is named `.jpg` and contains valid JPEG data.

**Confidence:** HIGH -- this is an architectural concern visible from the existing codebase structure.

**Phase:** Conversion implementation phase. This is a naming/routing change in the extraction pipeline.

---

### Pitfall 8: Double Memory for In-Memory Decode+Encode Pipeline

**What goes wrong:** The current pipeline reads archive entry bytes into a `Vec<u8>`, then writes them to disk. The conversion pipeline must: (1) read bytes into `Vec<u8>`, (2) decode into `DynamicImage` (which allocates width * height * 4 bytes for RGBA), (3) encode to a new `Vec<u8>` or write directly. For a 4000x3000 image, step 2 alone allocates ~48MB. With the source bytes also in memory, peak usage is ~50MB+ per image.

**Why it happens:** Image decoding inherently requires decompressing the full pixel buffer. This is unavoidable for format conversion.

**Consequences:**
- For typical document images (small, <1MB compressed), this is negligible.
- For high-resolution images in EPUB books (cover art, full-page illustrations), memory spikes could be significant.
- In batch processing of hundreds of files, memory should be reclaimed between images (Rust's ownership model helps here -- `DynamicImage` is dropped when it goes out of scope).

**Prevention:**
1. **Process images sequentially** (the existing loop structure already does this).
2. **Do not collect all decoded images into a Vec** -- decode, convert, write, then drop before processing the next image.
3. **Consider using `image::Limits`** to set a maximum decode dimension if untrusted/adversarial inputs are a concern: `let mut reader = ImageReader::new(cursor); reader.set_limits(Limits::default());`
4. For this tool's use case (user-owned DOCX/EPUB files), OOM from normal images is extremely unlikely. Malicious ZIP bombs with giant fake dimensions are the real threat, and `Limits` addresses that.

**Detection:** Process a DOCX containing a very large (e.g., 8000x6000) image and monitor memory usage. If it spikes above 200MB, consider adding limits.

**Confidence:** MEDIUM -- the risk is real but impact is low for the expected use case (personal document processing).

**Phase:** Conversion implementation phase. Use `Limits::default()` as a safety net; no need for custom limit tuning unless users report issues.

---

### Pitfall 9: ICO Files Contain Multiple Sizes -- Conversion Picks One Arbitrarily

**What goes wrong:** ICO files store multiple images at different resolutions (16x16, 32x32, 48x48, 256x256, etc.). When decoded via `image::load_from_memory()`, the `image` crate selects one size (typically the largest). If the user converts an ICO to PNG, they get a single resolution, losing the multi-size nature of the ICO.

**Why it happens:** `DynamicImage` is a single image. ICO's multi-image container model doesn't map to it.

**Consequences:** Minor -- ICO files are uncommon in DOCX/EPUB. When they do appear, getting the largest size is usually acceptable.

**Prevention:**
1. **Accept the default behavior** (largest size extracted) for ICO-to-other conversions.
2. **No special handling needed** unless users specifically request multi-size ICO extraction, which is out of scope.

**Detection:** Convert an ICO file with multiple sizes and verify the output is a single image at the expected resolution.

**Confidence:** MEDIUM -- verified ICO is in the supported format list, but multi-size behavior is inferred from ICO format specification rather than tested.

**Phase:** Not a priority. Accept default behavior.

---

## Minor Pitfalls

### Pitfall 10: TIFF Images May Have Unusual Color Spaces

**What goes wrong:** TIFF is a complex container format supporting CMYK, Lab, and other color spaces beyond RGB. The `image` crate handles common TIFF variants but may fail on exotic ones (e.g., CMYK TIFF from professional publishing workflows). A CMYK-to-RGB conversion had known rounding errors in older versions of the crate.

**Prevention:** Let decode errors for unusual TIFFs fall through to the existing error-handling path (warn and skip). Do not attempt special CMYK handling.

**Confidence:** MEDIUM -- CMYK TIFF rounding fix was noted in the changelog; current status is likely acceptable.

**Phase:** No special handling. Rely on `image` crate's built-in conversion.

---

### Pitfall 11: GIF-to-JPEG Loses Transparency AND Palette Precision

**What goes wrong:** GIF images use a 256-color palette. Decoding to `DynamicImage` expands to 8-bit RGBA, which is fine. But re-encoding to JPEG at quality 75 introduces compression artifacts that are disproportionately visible on the flat-color regions typical of GIF images (logos, diagrams, pixel art). The result looks much worse than the original GIF.

**Prevention:** Use a higher JPEG quality (90+) when the source format is GIF, or warn users that GIF-to-JPEG conversion degrades quality significantly for typical GIF content.

**Confidence:** MEDIUM -- this is general image processing knowledge, not specific to the `image` crate.

**Phase:** Conversion implementation phase, as a quality-tuning detail.

---

### Pitfall 12: Feature Flag Bloat from Default image Crate Features

**What goes wrong:** Adding `image = "0.25"` with default features pulls in codecs for AVIF, OpenEXR, HDR, Farbfeld, QOI, TGA, PNM, and DDS -- none of which appear in DOCX/EPUB files. This adds unnecessary compile time and binary size.

**Prevention:** Use `default-features = false` and explicitly enable only needed features:
```toml
[dependencies]
image = { version = "0.25", default-features = false, features = [
    "jpeg", "png", "gif", "bmp", "tiff", "webp", "ico"
] }
```
This covers all formats that appear in DOCX/EPUB files and all target conversion formats. The `image` crate documentation explicitly recommends this approach.

**Detection:** Check binary size and compile time before and after restricting features. Expect meaningful reduction in both.

**Confidence:** HIGH -- confirmed from `image` crate documentation and Cargo.toml feature flag system.

**Phase:** Dependency setup phase (adding `image` to `Cargo.toml`). Get this right from the start.

---

### Pitfall 13: BMP-to-WebP/PNG May Increase File Size When Source is Compressed BMP

**What goes wrong:** Some BMP files in DOCX archives use RLE compression. Decoding and re-encoding to PNG or lossless WebP may produce a slightly larger file. This is cosmetic, not a bug, but users may be confused.

**Prevention:** No action needed. The size difference is negligible for BMP, and the primary goal is format standardization, not compression.

**Confidence:** LOW -- BMP files in DOCX are rare, and RLE-compressed BMPs are even rarer.

**Phase:** Not a priority.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Adding `image` to Cargo.toml | Feature flag bloat (Pitfall 12) | Use `default-features = false` with explicit feature list |
| Core conversion: decode step | Extension/format mismatch (Pitfall 6) | Use `load_from_memory()` for magic-byte detection, not extension-based |
| Core conversion: decode step | SVG/WMF/EMF crash (Pitfall 3) | Check extension before decode; skip unconvertible formats |
| Core conversion: JPEG encode | Alpha channel black backgrounds (Pitfall 1) | Composite RGBA against white before encoding |
| Core conversion: JPEG encode | Low default quality (Pitfall 5) | Use `write_with_encoder` with quality 85+ |
| Core conversion: WebP encode | Lossless-only, larger than JPEG source (Pitfall 2) | Document limitation; emit warning for JPEG sources |
| Core conversion: GIF sources | Animated GIF frame loss (Pitfall 4) | Detect animation; skip or warn |
| Output file naming | Wrong extension on converted files (Pitfall 7) | Use target format extension, not source extension |
| Pipeline architecture | Memory spikes on large images (Pitfall 8) | Process sequentially; use `Limits::default()` |
| GIF-specific mode | No conversion pitfalls (`--gif-only` and `--convert` are exclusive) | Ensure mutual exclusivity is enforced at CLI parse time |

## Sources

- [image crate documentation (docs.rs)](https://docs.rs/image/latest/image/)
- [image crate ImageFormat enum (docs.rs)](https://docs.rs/image/latest/image/enum.ImageFormat.html)
- [image crate codecs format support table](https://docs.rs/image/latest/image/codecs/index.html)
- [image crate JpegEncoder (docs.rs)](https://docs.rs/image/latest/image/codecs/jpeg/struct.JpegEncoder.html)
- [image crate DynamicImage (docs.rs)](https://docs.rs/image/latest/image/enum.DynamicImage.html)
- [image-webp GitHub (lossless-only encoding)](https://github.com/image-rs/image-webp)
- [image crate CHANGES.md](https://github.com/image-rs/image/blob/main/CHANGES.md)
- [image crate GitHub (source code)](https://github.com/image-rs/image)
- [JPEG encoder source showing default quality 75](https://github.com/image-rs/image/blob/main/src/codecs/jpeg/encoder.rs)
- [JPEG ColorType panic issue #1476](https://github.com/image-rs/image/issues/1476)
- [WebP encoding suggestion issue #582](https://github.com/image-rs/image/issues/582)
- [Rust forum: RGBA to RGB conversion](https://users.rust-lang.org/t/image-cargo-quick-way-to-convert-imagebuffer-rgba-u8-vec-u8-to-imagebuffer-rgb-u8-vec-u8/55740)
- [load_from_memory documentation](https://docs.rs/image/latest/image/fn.load_from_memory.html)
- [guess_format documentation](https://docs.rs/image/latest/image/fn.guess_format.html)
- [Windows Metafile format overview](https://en.wikipedia.org/wiki/Windows_Metafile)
