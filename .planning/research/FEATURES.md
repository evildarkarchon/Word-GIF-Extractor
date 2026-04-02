# Feature Landscape

**Domain:** CLI image extraction and format conversion (Rust)
**Researched:** 2026-04-01

## Table Stakes

Features users expect from a `--convert` flag on an image extraction tool. Missing any of these will make the feature feel broken or incomplete.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Convert to jpg, png, webp | These are the three formats users actually want when they say "convert." jpg for photos/size, png for lossless/transparency, webp for modern web. | Low | `image` crate supports encoding all three. WebP is lossless-only via `image` crate, which is acceptable for this use case (see Pitfalls). |
| Sensible default quality for JPEG | Users should not have to specify quality to get usable output. ImageMagick defaults vary; `sic` defaults to 80; `image` crate defaults to 75. Industry convention is 75-85. | Low | Use 85 as default -- slightly above `image` crate's 75, matching common "good quality" expectations for extracted document images. |
| Skip unsupported source formats with warning | DOCX/EPUB archives contain WMF, EMF, SVG that the `image` crate cannot decode. Halting the entire batch on one unsupported file is unacceptable. | Low | Print warning to stderr, extract the unsupported file as-is (raw bytes), continue processing. Already established as a key decision in PROJECT.md. |
| Alpha channel handling for JPEG conversion | PNG/WebP/GIF images with transparency converted to JPEG must not produce black backgrounds or corrupted output. JPEG has no alpha channel. | Medium | Must composite transparent pixels against a white background before JPEG encoding. The `image` crate's DynamicImage will strip alpha silently, typically resulting in black backgrounds -- this must be handled explicitly. |
| Converted file replaces original (no duplicates) | When `--convert png` is used, users expect only .png files in output. Keeping both original and converted creates confusion. Already a validated decision. | Low | Write converted bytes directly; never write the original when conversion is active. |
| Output file naming preserves base name | `document_1.jpg` converted to png should become `document_1.png`, not `document_1.jpg.png` or a random name. Consistent with existing naming scheme. | Low | Replace the extension in the existing `get_unique_output_path` flow. |
| Clear error messages for conversion failures | When an image can't be decoded (corrupted data, truncated archive entry), the error message must identify which file failed and why, then continue with the next image. | Low | Use anyhow context chaining: "Failed to convert image 3 from 'document.docx': unsupported format 'wmf'". |
| GIF-only extraction mode (`--gif-only`) | Simple format filter for a very common use case. Users extracting GIFs from documents don't want to sort through dozens of PNG/JPEG layout images. | Low | Equivalent to `--formats gif` but more discoverable and semantic. Internally, just set the allowed_extensions to {"gif"}. |
| GIF separate output directory (`--gif-output <path>`) | Users want GIFs routed to a different folder while other images go to the normal output. Common workflow for document image triage. | Low-Medium | Routing concern: when a file has .gif extension, write to gif_output instead of output. Must create directory if it doesn't exist. |
| Mutual exclusivity of `--convert` and `--gif-only` | These flags conflict logically. `--convert jpg` would convert GIFs away from GIF format, while `--gif-only` filters to GIFs. Allowing both is confusing. | Low | clap conflict annotation. Already a validated decision. |
| Progress indication during conversion | Conversion adds CPU time per image. Users need to see progress is happening, not wonder if the tool hung. Existing progress bars must continue to work. | Low | Existing indicatif progress bars cover extraction. Conversion happens inline per-image, so existing progress increments cover it. May want to add "(converting)" to progress message. |

## Differentiators

Features that go beyond basic expectations. Not required for v1, but add meaningful value if complexity is low.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| `--quality <1-100>` flag for JPEG output | Power users want control over file size vs. quality tradeoff. ImageMagick, sic, and every serious converter exposes this. Default of 85 is good, but override is valuable. | Low | Single `u8` clap argument, pass to `JpegEncoder::new_with_quality`. Only applies when `--convert jpg`. Ignore/warn if used with png/webp. |
| Conversion summary statistics | After batch run, show "Converted 47 images to PNG (3 skipped: unsupported format)". Gives users confidence the batch completed correctly. | Low | Count converted, skipped, failed. Print at end. Extends existing summary message. |
| `--gif-output` works without `--gif-only` | Extract all images normally, but route GIFs to a separate directory. More flexible than requiring both flags together. | Low | Just a routing rule: if image extension is gif and gif_output is set, use that path. No need for gif-only to be set. |
| Dry-run mode (`--dry-run`) | Show what would be extracted and converted without writing files. Useful for large batches where users want to preview. | Medium | Requires threading a dry-run flag through all extraction paths. Print planned actions instead of writing. |

## Anti-Features

Features to explicitly NOT build. These add complexity without proportional value for this tool's use case.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Multiple target formats per run (`--convert png,webp`) | Doubles output file count, complicates naming, unclear which file is "the" output. Users can run the tool twice. | Support one format per `--convert` invocation. Already decided in PROJECT.md. |
| Keep both original and converted files | Creates clutter, doubles disk usage, confusing to users. If they want originals, run without `--convert`. | Only output converted files when `--convert` is active. Already decided. |
| Lossy WebP encoding with quality parameter | The `image` crate only supports lossless WebP encoding. Adding lossy would require `libwebp-sys` (C dependency) or `zenwebp` (less mature). Not worth the dependency complexity. | Use `image` crate's lossless WebP encoder. Lossless WebP is still smaller than PNG for most images. Document that WebP output is lossless. |
| Image resizing during conversion | Scope creep. This tool extracts and optionally converts. Resizing is a different workflow (use ImageMagick, sic, or a dedicated tool). | Do not add `--resize`, `--width`, `--height`, etc. |
| Animated GIF frame extraction | Extracting individual frames from animated GIFs is a specialized workflow requiring proper frame compositing (disposal methods, transparency). The `gif-dispose` crate exists for this but it's a different tool. | Extract GIFs as-is. Do not split frames. |
| AVIF output format | `image` crate supports AVIF encoding but requires the `avif-native` feature with C dependencies (dav1d, rav1e). Adds significant build complexity for a niche format. | Support jpg, png, webp only. Can revisit if pure-Rust AVIF encoding matures. |
| Video extraction (MP4 embedded in DOCX) | Out of scope for an image extractor. Different codecs, different tools. | Ignore non-image media files in archives. |
| Interactive format selection | No TUI menus, no prompts. This is a batch CLI tool. | All configuration via flags. |
| SVG-to-raster conversion | SVG is a vector format. Rasterizing it requires a full rendering engine (resvg/tiny-skia). Heavy dependency for a niche case. | Extract SVG files as-is. Skip them during `--convert` with a warning. |

## Feature Dependencies

```
--convert <format> (core conversion)
  |-- Alpha channel handling (required for --convert jpg)
  |-- Skip unsupported formats (required: WMF/EMF/SVG can't be decoded)
  |-- Output naming (.ext replacement)
  |-- Quality default (85 for JPEG)
  |
  +-- --quality <n> (optional, enhances --convert jpg)

--gif-only (standalone filter)
  |-- (no dependencies, just restricts allowed_extensions to {"gif"})

--gif-output <path> (routing)
  |-- Directory creation if not exists
  |-- (independent of --gif-only; works with or without it)

--convert conflicts with --gif-only (mutual exclusion)
```

## MVP Recommendation

Prioritize (in implementation order):

1. **`--convert <format>`** with jpg/png/webp support -- this is the headline feature. Includes alpha-to-white compositing for JPEG, skip-and-warn for unsupported formats, and correct output naming.
2. **`--gif-only`** -- trivial to implement, just sets allowed_extensions = {"gif"}.
3. **`--gif-output <path>`** -- routing logic for GIF files to a separate directory.
4. **Mutual exclusivity** between `--convert` and `--gif-only` -- clap annotation.
5. **`--quality <n>`** for JPEG -- low effort, high value for power users.

Defer:
- **`--dry-run`**: Useful but adds complexity across all code paths. Better as a follow-up milestone.
- **Conversion summary statistics**: Nice polish but not blocking. Can add as part of any phase.

## Technical Notes

### image Crate Format Support (encoding)

| Format | Encoding | Quality Control | Notes |
|--------|----------|----------------|-------|
| JPEG | Yes | `JpegEncoder::new_with_quality(w, 1-100)`, default 75 | Use 85 as tool default |
| PNG | Yes | Lossless, no quality knob | `PngEncoder` with default compression |
| WebP | Lossless only | No quality knob | `WebPEncoder::new_lossless(w)`. Lossy requires libwebp-sys. |
| BMP | Yes | N/A | Not a conversion target (no user demand) |
| GIF | Yes | N/A | Not a conversion target (animated GIF encoding is complex) |
| TIFF | Yes | N/A | Not a conversion target (niche) |

### Formats That Cannot Be Decoded (conversion must skip)

| Format | Reason |
|--------|--------|
| WMF | Windows Metafile -- vector format, no Rust decoder |
| EMF | Enhanced Metafile -- vector format, no Rust decoder |
| SVG | Vector format -- requires rendering engine (resvg) to rasterize |

### Alpha Channel Handling Strategy

When converting to JPEG (which has no alpha channel):
1. Load image via `image::load_from_memory(bytes)`
2. Check if loaded `DynamicImage` has alpha via color type
3. If alpha present: create white background `RgbImage`, composite the RGBA image onto it
4. Encode the resulting RGB image as JPEG

Without this step, the `image` crate silently drops the alpha channel, causing transparent regions to render as black -- a well-known issue that every image conversion tool must handle.

## Sources

- [image crate codecs documentation (encoding/decoding table)](https://docs.rs/image/latest/image/codecs/index.html) -- HIGH confidence
- [JpegEncoder API and default quality of 75](https://docs.rs/image/latest/image/codecs/jpeg/struct.JpegEncoder.html) -- HIGH confidence, verified from source
- [WebPEncoder -- lossless only](https://docs.rs/image/latest/image/codecs/webp/struct.WebPEncoder.html) -- HIGH confidence
- [sic/imagineer -- Rust image CLI, JPEG default quality 80](https://github.com/foresterre/sic) -- MEDIUM confidence
- [ImageMagick convert CLI patterns](https://imagemagick.org/script/convert.php) -- MEDIUM confidence (reference for industry conventions)
- [Image compression strategy 2025](https://unifiedimagetools.com/en/articles/ultimate-image-compression-strategy-2025) -- LOW confidence (general guidance)
- [CSS-Tricks CLI image conversion patterns](https://css-tricks.com/converting-and-optimizing-images-from-the-command-line/) -- MEDIUM confidence
- [image crate DynamicImage alpha handling](https://docs.rs/image/latest/image/enum.DynamicImage.html) -- HIGH confidence
