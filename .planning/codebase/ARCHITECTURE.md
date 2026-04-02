# Architecture

**Analysis Date:** 2026-04-01

## Pattern Overview

**Overall:** Modular CLI application with format-specific processors behind a common interface

**Key Characteristics:**
- Single-binary CLI tool with `clap` derive-based argument parsing
- Format-specific processing modules (`docx`, `epub`) dispatched by file extension
- Shared utility layer (`common`) for cross-cutting file I/O, path safety, and naming
- No trait-based abstraction; dispatch uses a simple `match` on `DocumentType` enum
- All processing is synchronous and single-threaded

## Layers

**CLI / Entry Point:**
- Purpose: Parse arguments, collect input files, orchestrate processing pipeline, display progress
- Location: `src/main.rs`
- Contains: `Args` struct (clap), `DocumentType` enum, file collection, metadata filtering, deduplication, main processing loop, progress bar management
- Depends on: `common`, `docx`, `epub`, `clap`, `walkdir`, `indicatif`
- Used by: End user via CLI

**Format Processors:**
- Purpose: Handle format-specific archive traversal and image extraction
- Location: `src/docx.rs`, `src/epub.rs`
- Contains: `process_file()` entry points per format, format-specific logic (ZIP scanning for DOCX, resource iteration for EPUB), cover detection (EPUB only), metadata extraction (EPUB only)
- Depends on: `common`, `zip` crate (DOCX), `epub` crate (EPUB)
- Used by: `src/main.rs` via `docx::process_file()` and `epub::process_file()`

**Common Utilities:**
- Purpose: Shared helpers for path safety, filename generation, file writing, and extension management
- Location: `src/common.rs`
- Contains: `ImageToExtract` struct, `get_supported_extensions()`, `normalize_format()`, `is_safe_archive_path()`, `sanitize_filename()`, `get_unique_output_path()`, `write_image_to_file()`
- Depends on: `anyhow`, `std::fs`, `std::io`
- Used by: `src/main.rs`, `src/docx.rs`, `src/epub.rs`

## Data Flow

**Main Pipeline (file processing):**

1. CLI args parsed into `Args` struct (`src/main.rs` lines 21-58)
2. Input paths resolved: positional + `--input` merged, default to CWD if empty (`src/main.rs` lines 302-313)
3. Allowed extensions computed from `--formats` flag or defaults (`src/main.rs` lines 317-329)
4. File collection: `collect_document_files()` walks inputs, filters by `.docx`/`.epub` extension (`src/main.rs` lines 105-157)
5. EPUB metadata filtering (optional): `filter_epub_files_with_progress()` opens each EPUB to check title/author match, partitions EPUBs from other types (`src/main.rs` lines 161-194)
6. EPUB deduplication: `deduplicate_by_metadata()` removes duplicate EPUBs by (author, title) pair (`src/main.rs` lines 200-267)
7. Per-file dispatch: `process_file()` matches on `DocumentType` and delegates to `docx::process_file()` or `epub::process_file()` (`src/main.rs` lines 270-297)
8. Results aggregated: total images and documents counted, summary displayed via progress bar

**DOCX Processing (`src/docx.rs`):**

1. Open file as `ZipArchive`
2. Scan all ZIP entries; filter by extension against allowed set, validate path safety
3. Collect matching entries as `Vec<ImageToExtract>` (index + extension)
4. Create output directory
5. For each image: read data from archive by index, compute unique output path (`{docname}_{n}.{ext}`), write to disk

**EPUB Processing (`src/epub.rs`):**

1. Open file with `EpubDoc::new()`
2. Extract metadata (title from `"title"`, author from `"creator"`)
3. Check filter match if filter is set; skip if no match
4. Compute base name from metadata: `"Author - Title"` pattern, sanitized, with fallback to filename
5. If `cover_only`: attempt `doc.get_cover()`, then filename-based cover detection (`find_cover_by_filename()`), then optional fallback to all images
6. If extracting all: iterate `doc.resources`, filter by MIME type prefix `"image/"` and allowed extensions, extract each via `doc.get_resource()`

**State Management:**
- No persistent state; each run is stateless
- In-memory collections (`Vec<PathBuf>`, `HashMap` for dedup) used within a single invocation
- `EpubDoc` holds a `BufReader<File>` for the duration of processing a single file

## Key Abstractions

**DocumentType Enum:**
- Purpose: Type-safe document format identification
- Defined in: `src/main.rs` (line 61)
- Variants: `Docx`, `Epub`
- Pattern: Used for dispatch in `process_file()` and helper functions `is_supported_document()`, `is_epub()`

**ImageToExtract Struct:**
- Purpose: Represents a pending image extraction from a ZIP archive (DOCX only)
- Defined in: `src/common.rs` (line 83)
- Fields: `index: usize` (archive entry index), `extension: String`
- Pattern: Two-pass approach -- first collect matching entries, then extract them

**EpubFilter Struct:**
- Purpose: Encapsulates EPUB metadata filtering criteria
- Defined in: `src/epub.rs` (line 17)
- Fields: `title: Option<String>`, `author: Option<String>`
- Pattern: Passed through the pipeline; checked at filter phase and again at process phase

**EpubImage Struct (private):**
- Purpose: Holds resource ID and extension for an image found in EPUB resources
- Defined in: `src/epub.rs` (line 116)
- Fields: `id: String`, `extension: String`

## Entry Points

**Binary Entry Point:**
- Location: `src/main.rs` `fn main()`
- Triggers: CLI invocation (`word-image-extractor` or `cargo run`)
- Responsibilities: Full orchestration from args to output

**Module Entry Points (called from main):**
- `docx::process_file()` at `src/docx.rs` line 16 -- processes one DOCX file
- `epub::process_file()` at `src/epub.rs` line 127 -- processes one EPUB file
- `epub::check_filter_match()` at `src/epub.rs` line 58 -- checks if an EPUB matches filter criteria
- `epub::get_metadata()` at `src/epub.rs` line 71 -- reads EPUB metadata for deduplication
- `epub::get_base_name()` at `src/epub.rs` line 84 -- gets display name for progress bar

## Error Handling

**Strategy:** `anyhow::Result` throughout with contextual error messages via `.with_context()` / `.context()`

**Patterns:**
- All public functions return `Result<T>` (from `anyhow`)
- Errors in the per-file processing loop are caught and printed to stderr; processing continues with remaining files (`src/main.rs` lines 398-419)
- Invalid/missing input paths produce warnings but do not abort (`src/main.rs` lines 339-345)
- Archive path safety violations are silently skipped (defense-in-depth, not user-facing errors)
- The `epub` crate returns `String` errors which are converted to `anyhow::Error` via `anyhow::anyhow!()` (`src/epub.rs` line 60)
- Unique filename generation has a hard limit of 1000 attempts before bailing (`src/common.rs` line 119)

## Cross-Cutting Concerns

**Logging:** Direct `eprintln!()` for warnings and errors. No structured logging framework. Progress indication via `indicatif` progress bars and spinners (`src/main.rs`).

**Validation:**
- Archive path traversal protection in `is_safe_archive_path()` (`src/common.rs` lines 41-66) -- checks for null bytes, `..`, absolute paths, Windows drive letters, alternate data streams
- Filename sanitization in `sanitize_filename()` (`src/common.rs` lines 70-80) -- replaces `/\:*?"<>|` and control characters with `_`
- Format normalization handles aliases (e.g., `jpg`/`jpeg`, `tiff`/`tif`) in `normalize_format()` (`src/common.rs` lines 16-33)

**Authentication:** Not applicable (local CLI tool).

**File I/O:** Centralized in `write_image_to_file()` (`src/common.rs` lines 143-159) using `BufWriter` with explicit flush and context-rich error messages.

## Key Design Decisions

**Two-pass extraction for DOCX:** Scan all archive entries first to collect matches, then extract. This allows computing `total_images` upfront for filename numbering (`{name}_{n}.{ext}` vs `{name}.{ext}` for single images).

**Metadata-based naming for EPUB:** Output filenames derive from EPUB metadata (`"Author - Title"`) rather than the source filename, which is often an opaque hash or ID. Falls back to filename when metadata is absent.

**No trait-based polymorphism:** DOCX and EPUB processors have different signatures (`epub::process_file` takes additional `cover_only`, `cover_fallback`, `filter` parameters). Dispatch is a simple `match` rather than a trait object, which keeps complexity low but means the caller must pass EPUB-specific args through even for DOCX files.

**Partition-based pipeline for EPUB features:** Metadata filtering and deduplication operate only on EPUB files by partitioning the file list into `(epub_files, other_files)` and recombining after. This avoids applying EPUB-specific logic to DOCX files without needing separate pipelines.

**Cover detection cascade:** Three-level fallback for EPUB covers: (1) `EpubDoc::get_cover()` (metadata-based), (2) filename heuristic (`find_cover_by_filename` looks for files named `cover.*`), (3) optional fallback to extracting all images (`--cover-fallback` flag).

---

*Architecture analysis: 2026-04-01*
