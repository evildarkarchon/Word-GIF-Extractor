# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Rust CLI tool that extracts image files from Microsoft Word (.docx) documents by treating them as ZIP archives. Supports multiple image formats: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico.

## Build Commands

```bash
# Build release binary
cargo build --release

# Run directly via cargo
cargo run -- "path/to/document.docx"
cargo run -- --input "path/to/document.docx" --output "output/folder"

# Process a directory of .docx files
cargo run -- "path/to/folder"

# Recursive directory processing
cargo run -- "path/to/folder" -r

# Filter by format (comma-separated)
cargo run -- "document.docx" -f png,gif

# Extract only cover images from EPUB files
cargo run -- "book.epub" -c
cargo run -- "path/to/epubs" -r --cover-only

# Run tests
cargo test
```

The release binary is output to `target/release/word-image-extractor.exe`.

## Architecture

Single-file application (`src/main.rs`) with straightforward flow:
1. Parse CLI arguments with `clap` (supports positional or `--input` flag)
2. Determine target image extensions (all supported or user-filtered via `-f`)
3. If input is a file: process it directly; if directory: iterate over .docx files (optionally recursive with `-r`)
4. Open each .docx as a ZIP archive and scan for matching image extensions
5. Extract images to output directory, renamed as `{docname}_{n}.{ext}` (or `{docname}.{ext}` if only one)

## Dependencies

- **zip**: Archive traversal for .docx files
- **clap**: CLI argument parsing with derive macros
- **anyhow**: Error handling with context
- **walkdir**: Recursive directory traversal

<!-- GSD:project-start source:PROJECT.md -->
## Project

**Word/EPUB Image Extractor — Conversion & GIF Features**

A Rust CLI tool that extracts images from Microsoft Word (.docx) and EPUB documents. The tool treats these files as ZIP archives, scans for image entries, and writes them to disk with intelligent naming. This milestone adds image format conversion and GIF-specific extraction workflows.

**Core Value:** Extracted images are consistently in the user's desired format — no manual conversion step after extraction.

### Constraints

- **Rust edition**: 2024 (requires Rust >= 1.85)
- **Dependency**: `image` crate for conversion — must handle the formats users actually encounter in DOCX/EPUB (jpg, png, gif, bmp, tiff, webp)
- **Performance**: Conversion adds CPU cost per image; acceptable for a CLI batch tool
- **Compatibility**: Some archive images may be WMF/EMF (Windows metafiles) — `image` crate does not support these; conversion should skip unsupported formats with a warning
<!-- GSD:project-end -->

<!-- GSD:stack-start source:codebase/STACK.md -->
## Technology Stack

## Languages
- Rust (Edition 2024) - All application code
- None
## Runtime
- Rust 1.94.0 (nightly/stable; edition 2024 requires Rust >= 1.85)
- No `rust-toolchain.toml` present; relies on the system-installed toolchain
- Cargo 1.94.0
- Lockfile: `Cargo.lock` present and committed (lockfile version 4)
## Frameworks
- No web/application framework; this is a standalone CLI binary
- Built-in `#[cfg(test)]` / `cargo test` with standard Rust test harness
- No external test framework (no `proptest`, `quickcheck`, or `rstest`)
- Cargo (standard Rust build system)
- No custom build scripts (`build.rs` not present)
- No procedural macros beyond `clap`'s derive macros
## Key Dependencies
| Crate | Version (spec) | Resolved | Purpose |
|-------|----------------|----------|---------|
| `zip` | `2.1.0` | `2.4.2` | Opens DOCX files as ZIP archives for image extraction |
| `clap` | `4.5.4` (features: `derive`) | `4.5.53` | CLI argument parsing with derive macros |
| `anyhow` | `1.0.82` | `1.0.100` | Ergonomic error handling with context chaining |
| `walkdir` | `2.5.0` | `2.5.0` | Recursive directory traversal for batch processing |
| `epub` | `2.1.4` | `2.1.5` | EPUB document parsing, metadata extraction, and resource access |
| `indicatif` | `0.18.3` | `0.18.3` | Terminal progress bars and spinners for user feedback |
- `epub` crate internally uses `zip 3.0.0` (different major version from the direct `zip 2.x` dependency)
- `epub` depends on `xml-rs`, `regex`, `percent-encoding`, `thiserror`
- `zip 2.x` pulls in encryption support (`aes`, `hmac`, `sha1`, `pbkdf2`) and compression (`flate2`, `bzip2`, `lzma-rs`, `deflate64`)
## Configuration
- No environment variables required
- No `.env` files present
- Pure CLI-driven configuration via `clap` arguments
- `Cargo.toml`: Single package, no workspace
- Release profile (`[profile.release]`):
- `inputs` (positional) - Input file/directory paths
- `-i / --input` - Named input paths (alternative to positional)
- `-o / --output` - Output directory (defaults to `.`)
- `-r / --recursive` - Recursive directory scanning
- `-f / --formats` - Comma-delimited format filter (e.g., `png,jpg`)
- `-c / --cover-only` - Extract only EPUB cover images
- `--cover-fallback` - Fall back to all images if no cover found (requires `-c`)
- `--title` - Filter EPUBs by title substring (case-insensitive)
- `--author` - Filter EPUBs by author substring (case-insensitive)
## Platform Requirements
- Rust toolchain >= 1.85 (required for edition 2024; code uses `let` chains which stabilized in edition 2024)
- Cargo with lockfile v4 support
- No OS-specific build dependencies
- Cross-platform CLI binary (Windows, Linux, macOS)
- No runtime dependencies beyond the OS standard library
- Binary output: `target/release/word-image-extractor.exe` (Windows) / `target/release/word-image-extractor` (Unix)
- No network access required at runtime
## Source File Layout
| File | Lines | Purpose |
|------|-------|---------|
| `src/main.rs` | ~448 | Entry point, CLI parsing, orchestration, progress bars |
| `src/common.rs` | ~230 | Shared utilities: path safety, filename sanitization, image I/O |
| `src/docx.rs` | ~85 | DOCX processing: ZIP traversal and image extraction |
| `src/epub.rs` | ~439 | EPUB processing: metadata, cover detection, image extraction |
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

## Naming Patterns
- Use lowercase `snake_case.rs` for all source files
- Module names match filenames: `common.rs`, `docx.rs`, `epub.rs`
- Single entry point: `main.rs`
- Use `snake_case` for all functions: `get_document_type`, `is_supported_document`, `extract_cover_only`
- Prefix boolean-returning functions with `is_` or `matches_`: `is_epub()`, `is_safe_archive_path()`, `matches_filter()`
- Prefix getter functions with `get_`: `get_metadata()`, `get_base_name()`, `get_supported_extensions()`
- Use verb-noun pattern for action functions: `process_file()`, `collect_document_files()`, `write_image_to_file()`
- Use `snake_case` for all variables and parameters
- Descriptive names preferred: `allowed_extensions`, `output_base_dir`, `cover_fallback`
- Short names acceptable for iterators: `i`, `e`, `c`
- Use `PascalCase` for structs and enums: `Args`, `DocumentType`, `EpubFilter`, `ImageToExtract`, `EpubImage`
- Derive macros on structs: `#[derive(Debug, Clone)]` for data structs, `#[derive(Parser, Debug)]` for CLI
- Use `SCREAMING_SNAKE_CASE`: `JPEG_EXTENSIONS`, `MAX_ATTEMPTS`
- Constants defined with `const` at module level
## Code Style
- Default `rustfmt` formatting (no `.rustfmt.toml` present)
- 4-space indentation
- Trailing commas in multi-line expressions
- Opening brace on same line as declaration
- Run `cargo fmt` before committing
- Default `clippy` rules (no `clippy.toml` present)
- Run `cargo clippy` to check
- No `#[allow(...)]` annotations present in codebase -- all clippy warnings are resolved
- Rust 2024 edition (`edition = "2024"` in `Cargo.toml`)
- Uses 2024-edition features like `let chains` in if-let expressions (see `src/epub.rs` line 271-274)
## Import Organization
- Group related imports with `use` blocks separated by blank lines
- Use braced imports for multiple items from same module: `use crate::common::{get_unique_output_path, is_safe_archive_path, sanitize_filename, write_image_to_file};`
- Prefer explicit imports over glob imports (no `use x::*` anywhere)
- None configured. All imports use full paths relative to crate root.
## Error Handling
- All fallible public functions return `anyhow::Result<T>`
- Use `.context()` / `.with_context()` for adding context to errors:
- Use `anyhow::bail!()` macro for explicit error returns:
- Use `.map_err()` to convert library errors that don't implement `std::error::Error`:
- Non-fatal errors use `eprintln!("Warning: ...")` and continue processing (see `src/main.rs` lines 178-183, `src/common.rs` line 31)
- The `?` operator is used consistently for propagation
- Always include the file path in error context messages
- Use format: `"Failed to {action}: {path}"` (e.g., "Failed to create output file: /path/to/file")
## Logging
- Warnings to stderr: `eprintln!("Warning: ...")`
- Progress indication via `indicatif` crate with two styles:
- Use `pb.suspend(|| { eprintln!(...) })` to print warnings without corrupting progress bar output
- No structured logging or log levels -- this is a CLI tool
## Documentation Comments
- Every module has a `//!` doc comment at the top:
- All public functions have `///` doc comments explaining purpose and return value
- Private functions with non-obvious behavior also have `///` doc comments
- Comments describe WHAT the function does, not HOW:
- Used sparingly for non-obvious decisions or crate-specific API notes:
- Comments explain WHY, not WHAT (the code explains what)
- Public struct fields have `///` doc comments (see `ImageToExtract` in `src/common.rs`)
- CLI arg fields use `///` comments that become help text via clap derive (see `Args` in `src/main.rs`)
## Function Design
- Pass `&Path` for filesystem paths (not `PathBuf` or `&str`)
- Pass `&HashSet<&str>` for extension filters (borrowed, not owned)
- Pass `&EpubFilter` for filter criteria (borrowed reference)
- Mutable references only when needed: `&mut EpubDoc<...>`
- Use `Result<usize>` for extraction functions (count of items extracted)
- Use `Result<bool>` for filter/match functions
- Use `Option<T>` for lookups that may not find anything
- Return `Ok(0)` for "nothing to do" cases (not an error)
## Module Design
- Public functions and types exported explicitly with `pub`
- Internal helpers are private (no `pub`)
- No barrel files or re-exports
- `common` provides shared utilities used by both `docx` and `epub`
- `docx` and `epub` are independent of each other
- `main` orchestrates everything and depends on all three modules
## CLI Argument Patterns
- Use `#[derive(Parser)]` on the `Args` struct in `src/main.rs`
- Short flags are single letters: `-i`, `-o`, `-r`, `-f`, `-c`
- Long flags use kebab-case: `--input`, `--output`, `--recursive`, `--formats`, `--cover-only`, `--cover-fallback`
- Positional arguments come first, named args after
- Use `#[arg(requires = "...")]` for dependent flags (e.g., `--cover-fallback` requires `--cover-only`)
- Default values handled in `main()` logic, not in clap attributes
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

## Pattern Overview
- Single-binary CLI tool with `clap` derive-based argument parsing
- Format-specific processing modules (`docx`, `epub`) dispatched by file extension
- Shared utility layer (`common`) for cross-cutting file I/O, path safety, and naming
- No trait-based abstraction; dispatch uses a simple `match` on `DocumentType` enum
- All processing is synchronous and single-threaded
## Layers
- Purpose: Parse arguments, collect input files, orchestrate processing pipeline, display progress
- Location: `src/main.rs`
- Contains: `Args` struct (clap), `DocumentType` enum, file collection, metadata filtering, deduplication, main processing loop, progress bar management
- Depends on: `common`, `docx`, `epub`, `clap`, `walkdir`, `indicatif`
- Used by: End user via CLI
- Purpose: Handle format-specific archive traversal and image extraction
- Location: `src/docx.rs`, `src/epub.rs`
- Contains: `process_file()` entry points per format, format-specific logic (ZIP scanning for DOCX, resource iteration for EPUB), cover detection (EPUB only), metadata extraction (EPUB only)
- Depends on: `common`, `zip` crate (DOCX), `epub` crate (EPUB)
- Used by: `src/main.rs` via `docx::process_file()` and `epub::process_file()`
- Purpose: Shared helpers for path safety, filename generation, file writing, and extension management
- Location: `src/common.rs`
- Contains: `ImageToExtract` struct, `get_supported_extensions()`, `normalize_format()`, `is_safe_archive_path()`, `sanitize_filename()`, `get_unique_output_path()`, `write_image_to_file()`
- Depends on: `anyhow`, `std::fs`, `std::io`
- Used by: `src/main.rs`, `src/docx.rs`, `src/epub.rs`
## Data Flow
- No persistent state; each run is stateless
- In-memory collections (`Vec<PathBuf>`, `HashMap` for dedup) used within a single invocation
- `EpubDoc` holds a `BufReader<File>` for the duration of processing a single file
## Key Abstractions
- Purpose: Type-safe document format identification
- Defined in: `src/main.rs` (line 61)
- Variants: `Docx`, `Epub`
- Pattern: Used for dispatch in `process_file()` and helper functions `is_supported_document()`, `is_epub()`
- Purpose: Represents a pending image extraction from a ZIP archive (DOCX only)
- Defined in: `src/common.rs` (line 83)
- Fields: `index: usize` (archive entry index), `extension: String`
- Pattern: Two-pass approach -- first collect matching entries, then extract them
- Purpose: Encapsulates EPUB metadata filtering criteria
- Defined in: `src/epub.rs` (line 17)
- Fields: `title: Option<String>`, `author: Option<String>`
- Pattern: Passed through the pipeline; checked at filter phase and again at process phase
- Purpose: Holds resource ID and extension for an image found in EPUB resources
- Defined in: `src/epub.rs` (line 116)
- Fields: `id: String`, `extension: String`
## Entry Points
- Location: `src/main.rs` `fn main()`
- Triggers: CLI invocation (`word-image-extractor` or `cargo run`)
- Responsibilities: Full orchestration from args to output
- `docx::process_file()` at `src/docx.rs` line 16 -- processes one DOCX file
- `epub::process_file()` at `src/epub.rs` line 127 -- processes one EPUB file
- `epub::check_filter_match()` at `src/epub.rs` line 58 -- checks if an EPUB matches filter criteria
- `epub::get_metadata()` at `src/epub.rs` line 71 -- reads EPUB metadata for deduplication
- `epub::get_base_name()` at `src/epub.rs` line 84 -- gets display name for progress bar
## Error Handling
- All public functions return `Result<T>` (from `anyhow`)
- Errors in the per-file processing loop are caught and printed to stderr; processing continues with remaining files (`src/main.rs` lines 398-419)
- Invalid/missing input paths produce warnings but do not abort (`src/main.rs` lines 339-345)
- Archive path safety violations are silently skipped (defense-in-depth, not user-facing errors)
- The `epub` crate returns `String` errors which are converted to `anyhow::Error` via `anyhow::anyhow!()` (`src/epub.rs` line 60)
- Unique filename generation has a hard limit of 1000 attempts before bailing (`src/common.rs` line 119)
## Cross-Cutting Concerns
- Archive path traversal protection in `is_safe_archive_path()` (`src/common.rs` lines 41-66) -- checks for null bytes, `..`, absolute paths, Windows drive letters, alternate data streams
- Filename sanitization in `sanitize_filename()` (`src/common.rs` lines 70-80) -- replaces `/\:*?"<>|` and control characters with `_`
- Format normalization handles aliases (e.g., `jpg`/`jpeg`, `tiff`/`tif`) in `normalize_format()` (`src/common.rs` lines 16-33)
## Key Design Decisions
<!-- GSD:architecture-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd:quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd:debug` for investigation and bug fixing
- `/gsd:execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->

<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd:profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
