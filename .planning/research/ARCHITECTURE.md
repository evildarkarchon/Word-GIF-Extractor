# Architecture Patterns

**Domain:** Image conversion pipeline for a Rust CLI tool
**Researched:** 2026-04-01

## Current Architecture (Baseline)

The existing codebase follows a clean four-file layout:

```
src/main.rs  -- CLI parsing, file collection, dispatch loop, progress bars
src/common.rs -- Path safety, filename generation, write_image_to_file()
src/docx.rs  -- DOCX processor: ZIP traversal -> image extraction
src/epub.rs  -- EPUB processor: metadata extraction, cover detection, image extraction
```

**Current data flow (per file):**
```
Archive bytes --> format filter --> raw image bytes --> write_image_to_file() --> disk
```

Both `docx::process_file()` and `epub::process_file()` share a common terminal step: they extract raw bytes from an archive entry and call `common::write_image_to_file(&output_path, &data)` to write them. This is the seam where conversion inserts.

## Recommended Architecture

### New Module: `src/convert.rs`

A single new module handles all conversion logic. It sits between "raw bytes extracted" and "bytes written to disk" as a transformation step.

**Why a dedicated module:** Conversion is a cross-cutting concern used by both DOCX and EPUB processors. It does not belong in either format-specific module, and `common.rs` should stay focused on file I/O primitives and path utilities. Conversion involves image decoding, format negotiation, and encoding -- a distinct responsibility.

### New Module: `src/gif.rs` (NOT recommended -- use routing in common instead)

GIF separation is NOT a processing concern -- it is a routing concern (which output directory to use). It does not warrant its own module. The routing decision ("is this a GIF and does the user want GIFs separated?") belongs in a small helper function in `common.rs` or directly in the output path computation.

### Updated Module Structure

```
src/main.rs    -- CLI parsing (adds --convert, --gif-only, --gif-output args),
                  file collection, dispatch loop, progress bars
src/common.rs  -- Path safety, filename generation, write_image_to_file(),
                  output directory routing (GIF separation logic)
src/convert.rs -- NEW: image format conversion (decode -> encode pipeline)
src/docx.rs    -- DOCX processor (calls convert when --convert is set)
src/epub.rs    -- EPUB processor (calls convert when --convert is set)
```

## Component Boundaries

| Component | Responsibility | Communicates With | Does NOT Do |
|-----------|---------------|-------------------|-------------|
| `main.rs` | CLI args, file collection, dispatch, progress | All other modules | Image decoding, format conversion, archive traversal |
| `common.rs` | Path safety, filename gen, file writing, output directory routing | `std::fs`, `std::path` | Image processing, archive I/O, CLI parsing |
| `convert.rs` | Decode raw bytes to `DynamicImage`, encode to target format, report unsupported formats | `image` crate, `std::io::Cursor` | File I/O, path decisions, archive traversal |
| `docx.rs` | ZIP traversal for DOCX archives, entry filtering | `zip` crate, `common.rs`, `convert.rs` | EPUB handling, CLI parsing |
| `epub.rs` | EPUB metadata, cover detection, resource iteration | `epub` crate, `common.rs`, `convert.rs` | DOCX handling, CLI parsing |

### Boundary Rules

1. **`convert.rs` is pure data transformation.** It takes `&[u8]` (raw image bytes) + source extension + target format and returns `Result<(Vec<u8>, String)>` (converted bytes + new extension). It never touches the filesystem.
2. **`common.rs` owns all filesystem writes.** The existing `write_image_to_file()` remains the single point of disk I/O.
3. **Format processors orchestrate the pipeline.** Each processor (`docx.rs`, `epub.rs`) calls `convert::maybe_convert()` on the raw bytes before calling `write_image_to_file()`. This keeps the conversion step optional and controlled by each processor.
4. **`main.rs` passes configuration down, not behavior.** The `--convert` target format and `--gif-output` path are parsed in `main.rs` and threaded through as arguments to the processors. `main.rs` does not call conversion functions directly.

## Data Flow

### Without Conversion (existing behavior, unchanged)

```
Archive --> raw bytes --> write_image_to_file(output_path, &data) --> disk
```

### With --convert (new pipeline)

```
Archive --> raw bytes --> convert::convert_image(&data, &src_ext, target_format)
                              |
                              +--> Unsupported format (wmf/emf/svg)?
                              |        --> return Ok(None) + eprintln warning
                              |
                              +--> Decode via image::load_from_memory(&data)
                              |        --> DynamicImage
                              |
                              +--> Encode via img.write_to(Cursor, target_format)
                              |        --> Vec<u8> of converted data
                              |
                              +--> return Ok(Some((converted_bytes, new_extension)))
                          |
                          v
                   write_image_to_file(output_path_with_new_ext, &converted_bytes)
                          |
                          v
                        disk
```

### With --gif-only (filtering + routing)

```
Archive --> raw bytes --> extension == "gif"?
                              |
                              +--> No: skip this image entirely
                              |
                              +--> Yes: write_image_to_file(output_path, &data) --> disk
```

This is handled by the existing format filtering mechanism. `--gif-only` is syntactic sugar for `--formats gif`. No new data flow needed.

### With --gif-output (directory routing)

```
Archive --> raw bytes --> extension == "gif"?
                              |
                              +--> Yes: output_dir = gif_output_dir
                              |
                              +--> No:  output_dir = normal output_dir
                          |
                          v
                   write_image_to_file(routed_output_path, &data) --> disk
```

The routing decision happens in the format processors when computing the output path, before calling `write_image_to_file()`. A helper in `common.rs` encapsulates this: `resolve_output_dir(base_dir, gif_dir, extension) -> &Path`.

## Detailed Component Design

### `convert.rs` -- The Conversion Module

```rust
// Public API surface (3 functions)

/// The target format for conversion
pub enum ConvertTarget {
    Jpg,
    Png,
    Webp,
}

/// Check whether a source extension can be converted by the image crate.
/// Returns false for wmf, emf, svg (unsupported by image crate).
pub fn can_convert(source_extension: &str) -> bool;

/// Convert raw image bytes to the target format.
/// Returns Ok(Some((bytes, extension))) on success,
/// Ok(None) if the source format is unsupported (caller should write original),
/// Err on decode/encode failure.
pub fn convert_image(
    data: &[u8],
    source_extension: &str,
    target: ConvertTarget,
) -> Result<Option<(Vec<u8>, &'static str)>>;

/// Map ConvertTarget to file extension string
pub fn target_extension(target: &ConvertTarget) -> &'static str;
```

**Internal flow of `convert_image()`:**
1. Check `can_convert(source_extension)` -- if false, return `Ok(None)`.
2. Call `image::load_from_memory(data)?` to get a `DynamicImage`.
3. Create a `Cursor<Vec<u8>>` for the output buffer.
4. Call `img.write_to(&mut cursor, image_format)` where `image_format` maps from `ConvertTarget`.
5. Return `Ok(Some((cursor.into_inner(), target_extension)))`.

**Why `load_from_memory` over `ImageReader`:** The raw bytes already come from a known source (archive entry with a known extension). `load_from_memory` does content-based format guessing which is sufficient. If guessing fails, `ImageReader::new(Cursor::new(data)).with_guessed_format()?.decode()?` is the fallback. Both approaches work; `load_from_memory` is simpler.

**Critical: Animated GIF handling.** When the `image` crate loads a GIF via `load_from_memory`, it returns only the first frame as a `DynamicImage`. This is acceptable for the `--convert` use case (converting GIF to PNG/JPG/WebP produces a still image). The tool does NOT attempt to preserve animation. If a user wants the animated GIF, they should not use `--convert` on GIF files, or use `--gif-only` to route GIFs to a separate directory.

### `common.rs` -- Additions

```rust
/// Determines the output directory for an image based on its extension
/// and whether GIF separation is enabled.
/// If gif_output_dir is Some and the extension is "gif", returns the gif dir.
/// Otherwise returns the base output dir.
pub fn resolve_output_dir<'a>(
    base_dir: &'a Path,
    gif_output_dir: Option<&'a Path>,
    extension: &str,
) -> &'a Path;
```

This is a 5-line function. It keeps the routing logic centralized rather than duplicated in both format processors.

### `main.rs` -- CLI Additions

New `Args` fields:
```rust
/// Convert extracted images to the specified format (jpg, png, or webp)
#[arg(long, conflicts_with = "gif_only")]
convert: Option<String>,

/// Extract only GIF images (shorthand for --formats gif)
#[arg(long, conflicts_with = "convert")]
gif_only: bool,

/// Separate output directory for GIF files
#[arg(long)]
gif_output: Option<PathBuf>,
```

**Mutual exclusivity** between `--convert` and `--gif-only` is handled by clap's `conflicts_with` attribute -- no manual validation code needed.

**`--gif-only` implementation:** In `main.rs`, when `gif_only` is true, override `target_extensions` to contain only `{"gif"}`. This reuses the existing format filtering pipeline with zero changes to the processors.

### Format Processor Changes

Both `docx.rs` and `epub.rs` need the same small modification to their write loops. The pattern is identical:

**Before (current):**
```rust
write_image_to_file(&output_path, &data)?;
```

**After (with conversion support):**
```rust
let (final_data, final_ext) = if let Some(target) = convert_target {
    match convert::convert_image(&data, &image.extension, target)? {
        Some((converted, ext)) => (converted, ext.to_string()),
        None => {
            eprintln!("Warning: Cannot convert {} format, extracting as-is", image.extension);
            (data, image.extension.clone())
        }
    }
} else {
    (data, image.extension.clone())
};

let output_dir = common::resolve_output_dir(output_base_dir, gif_output_dir, &final_ext);
let output_path = get_unique_output_path(output_dir, &base_name, seq_index, total_images, &final_ext)?;
write_image_to_file(&output_path, &final_data)?;
```

This is roughly 12 lines of new code per processor. The conversion step is optional (gated by `convert_target`), and unsupported formats fall through gracefully.

## Patterns to Follow

### Pattern 1: Optional Transformation Step

**What:** The conversion step returns `Option<(Vec<u8>, &str)>` rather than `Result<(Vec<u8>, &str)>`. `None` means "this format cannot be converted, proceed with original bytes." `Err` means "conversion was attempted but failed."

**When:** Any time a processing step may not apply to all inputs (e.g., WMF/EMF/SVG cannot be decoded by the `image` crate).

**Why:** This three-state return (converted, skipped, failed) gives callers clean control flow without catching errors for expected skip conditions.

```rust
match convert::convert_image(&data, &ext, target)? {
    Some((bytes, new_ext)) => { /* use converted */ }
    None => { /* warn and use original */ }
}
// Err propagates via ? as usual
```

### Pattern 2: Configuration Threading

**What:** New CLI options (`convert`, `gif_output`) are parsed in `main.rs` and passed as parameters to format processors. Processors do not read global state.

**When:** Always. This codebase has no global state and should stay that way.

**Why:** Makes each function's dependencies explicit. Testability. No hidden coupling.

### Pattern 3: Extension-Based Routing

**What:** Output directory selection is a pure function of (base_dir, gif_dir, extension). No other inputs needed.

**When:** Whenever the output location depends on the content type rather than the source document type.

**Why:** Keeps routing logic trivially testable and reusable across DOCX and EPUB processors.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Trait Abstraction for Two Processors

**What:** Introducing a `trait DocumentProcessor` or `trait ImagePipeline` for DOCX and EPUB.

**Why bad:** The two processors have fundamentally different signatures (`epub::process_file` takes 6 parameters including EPUB-specific filter/cover flags; `docx::process_file` takes 3). A trait would either require a fat config struct that bundles unrelated options, or force DOCX to accept and ignore EPUB-specific parameters. The current `match` dispatch is cleaner.

**Instead:** Keep the `match` dispatch. If a third format is added later, a trait might become warranted, but with two formats and divergent signatures, it is premature.

### Anti-Pattern 2: Conversion Inside `write_image_to_file`

**What:** Making `write_image_to_file` aware of conversion (e.g., adding a `convert_to: Option<ImageFormat>` parameter).

**Why bad:** Violates single responsibility. `write_image_to_file` is a file I/O primitive used everywhere. Adding conversion logic to it couples image processing with disk I/O, makes it harder to test, and means you cannot write unconverted bytes through the same function without special-casing.

**Instead:** Conversion happens before the write call. The write function receives already-final bytes.

### Anti-Pattern 3: GIF Routing as a Separate Pipeline

**What:** Creating a separate code path for GIF files that diverges early in the processing pipeline.

**Why bad:** Duplicates extraction logic. GIF files are extracted identically to other images -- the only difference is the output directory. A separate pipeline means maintaining two extraction paths.

**Instead:** GIF routing is a single-line directory decision at the point where the output path is computed. Same extraction, same write, different directory.

### Anti-Pattern 4: Converting GIFs When --gif-output Is Set

**What:** Applying `--convert` to GIF files that are being routed to a separate GIF directory.

**Why bad:** If the user specified `--gif-output`, they want the actual GIF files. Converting them to PNG in the GIF output directory defeats the purpose.

**Instead:** When both `--convert` and `--gif-output` are active, GIF files routed to the GIF output directory should be written as-is (unconverted). Non-GIF images go through conversion as normal. This is a design decision to document in the CLI help text.

## Updated Function Signatures (After Changes)

### `docx::process_file`

```rust
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    convert_target: Option<&ConvertTarget>,  // NEW
    gif_output_dir: Option<&Path>,           // NEW
) -> Result<usize>
```

### `epub::process_file`

```rust
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    filter: &EpubFilter,
    convert_target: Option<&ConvertTarget>,  // NEW
    gif_output_dir: Option<&Path>,           // NEW
) -> Result<usize>
```

### `main.rs::process_file` (dispatch wrapper)

```rust
fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    allowed_extensions: &HashSet<&str>,
    cover_only: bool,
    cover_fallback: bool,
    epub_filter: &EpubFilter,
    convert_target: Option<&ConvertTarget>,  // NEW
    gif_output_dir: Option<&Path>,           // NEW
) -> Result<usize>
```

**Note:** The growing parameter count on `epub::process_file` (now 8) is approaching the threshold where a config struct would improve ergonomics. This is not yet critical but is worth monitoring. A future refactor could bundle `cover_only`, `cover_fallback`, `filter`, `convert_target`, and `gif_output_dir` into an `ExtractionConfig` struct.

## Unsupported Format Handling Strategy

The `image` crate cannot decode these formats commonly found in DOCX/EPUB archives:

| Format | Found In | Why Unsupported | What To Do |
|--------|----------|-----------------|------------|
| WMF | DOCX (Word clipart, old diagrams) | Windows metafile, vector format | Skip conversion, extract raw bytes, warn |
| EMF | DOCX (embedded charts, diagrams) | Enhanced metafile, vector format | Skip conversion, extract raw bytes, warn |
| SVG | EPUB (some publishers use inline SVG) | Vector format, XML-based | Skip conversion, extract raw bytes, warn |

The `can_convert()` function in `convert.rs` returns `false` for these. The caller (`docx.rs` or `epub.rs`) then writes the original bytes with the original extension and prints a warning. The user gets all their images -- just some are not converted.

## Dependency Addition

```toml
[dependencies]
image = { version = "0.25", default-features = true }
```

The default features include all the format codecs needed (PNG, JPEG, GIF, WebP, BMP, TIFF, ICO). The `rayon` default feature adds parallel processing in some decoders, which is a free performance win for a batch CLI tool.

**Version rationale:** 0.25.x is the current stable line. It includes WebP encoding support (added in 0.24). Do not use 0.24 or earlier as WebP encoding was incomplete.

## Scalability Considerations

| Concern | At 10 images | At 1,000 images | At 10,000 images |
|---------|-------------|-----------------|------------------|
| Memory | Negligible; one image in memory at a time | Same; sequential processing | Same pattern holds |
| CPU | Instant | Noticeable (conversion adds ~50-200ms/image) | Minutes; consider future parallel option |
| Disk I/O | Trivial | Buffered writes sufficient | Still fine; images are small |

The current sequential single-threaded design is appropriate for the batch sizes this tool handles (documents contain tens to hundreds of images, not millions). The `rayon` feature in the `image` crate helps with per-image decode/encode parallelism without any code changes.

## Build Order (Dependencies Between Components)

The recommended implementation sequence follows the dependency graph:

```
1. convert.rs (standalone, no dependencies on other project modules)
     |
     v
2. common.rs changes (resolve_output_dir -- standalone helper)
     |
     v
3. main.rs CLI args (--convert, --gif-only, --gif-output, conflicts_with)
     |
     v
4. docx.rs integration (call convert + routing in write loop)
     |
     v
5. epub.rs integration (call convert + routing in write loop, more complex due to cover path)
```

**Why this order:**
- `convert.rs` can be built and tested in complete isolation with unit tests (give it bytes, check output bytes).
- `common.rs::resolve_output_dir` is a pure function testable with unit tests.
- CLI args must exist before processors can receive them.
- DOCX integration is simpler (single write loop) so do it first as a proving ground.
- EPUB integration is more complex (multiple write paths: `extract_all_images`, `extract_cover_only`, `find_cover_by_filename`) so do it last.

Each step produces a testable, shippable increment.

## Sources

- [image crate docs (docs.rs)](https://docs.rs/image/latest/image/) -- HIGH confidence, official documentation
- [ImageFormat enum](https://docs.rs/image/latest/image/enum.ImageFormat.html) -- HIGH confidence, verified encoding/decoding support matrix
- [DynamicImage API](https://docs.rs/image/latest/image/enum.DynamicImage.html) -- HIGH confidence, `write_to` and `save_with_format` signatures
- [ImageReader API](https://docs.rs/image/latest/image/struct.ImageReader.html) -- HIGH confidence, `with_guessed_format` and `decode` methods
- [image-rs/image GitHub](https://github.com/image-rs/image) -- HIGH confidence, default features and format support
- [gif crate](https://crates.io/crates/gif) -- MEDIUM confidence, animated GIF frame handling limitations
- [Converting PNG/JPEG to WebP discussion](https://users.rust-lang.org/t/converting-png-jpeg-image-to-webp/71080) -- MEDIUM confidence, community patterns

---

*Architecture analysis: 2026-04-01*
