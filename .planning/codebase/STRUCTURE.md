# Codebase Structure

**Analysis Date:** 2026-04-01

## Directory Layout

```
word-image-extractor/
├── src/
│   ├── main.rs         # CLI entry point, argument parsing, orchestration
│   ├── common.rs       # Shared utilities (path safety, file I/O, extensions)
│   ├── docx.rs         # DOCX image extraction processor
│   └── epub.rs         # EPUB image extraction processor
├── plans/              # Planning documents (not code)
├── .planning/
│   └── codebase/       # GSD codebase analysis documents
├── .claude/            # Claude Code configuration
├── Cargo.toml          # Rust package manifest and dependencies
├── Cargo.lock          # Pinned dependency versions
├── .gitignore          # Ignores /target and workspace file
├── CLAUDE.md           # Claude Code project instructions
├── GEMINI.md           # Gemini AI project instructions
├── README.md           # Project documentation
└── LICENSE             # License file
```

## Directory Purposes

**`src/`:**
- Purpose: All Rust source code
- Contains: 4 files -- entry point + 3 modules
- Key files: `main.rs` (448 lines), `epub.rs` (439 lines), `common.rs` (230 lines), `docx.rs` (85 lines)

**`plans/`:**
- Purpose: Planning documents
- Contains: Non-code planning files
- Generated: No
- Committed: Yes

**`.planning/codebase/`:**
- Purpose: GSD codebase analysis documents (auto-generated)
- Contains: Architecture, structure, conventions analysis files
- Generated: Yes (by GSD mapping)
- Committed: Yes

## Module Map

**`src/main.rs` (448 lines) -- Entry point and orchestration:**
- `Args` struct: CLI argument definitions via `clap::Parser` derive
- `DocumentType` enum: `Docx` | `Epub` for format identification
- `get_document_type(path) -> Option<DocumentType>`: Maps file extension to document type
- `is_supported_document(path) -> bool`: Checks if path has a supported extension
- `is_epub(path) -> bool`: Checks if path is an EPUB file
- `create_progress_style() -> ProgressStyle`: Standard bar style for known-length phases
- `create_spinner_style() -> ProgressStyle`: Spinner style for unknown-length phases
- `collect_document_files(inputs, recursive) -> Vec<PathBuf>`: Walks inputs, collects matching files
- `filter_epub_files_with_progress(files, filter) -> Vec<PathBuf>`: Filters EPUBs by metadata
- `deduplicate_by_metadata(files) -> Vec<PathBuf>`: Deduplicates EPUBs by (author, title)
- `process_file(...)  -> Result<usize>`: Dispatches to format-specific processor
- `fn main() -> Result<()>`: Full pipeline orchestration

**`src/common.rs` (230 lines) -- Shared utilities:**
- `get_supported_extensions() -> HashSet<&'static str>`: Returns all 12 supported image extensions
- `normalize_format(fmt) -> Vec<&'static str>`: Maps user format string to actual extensions (handles aliases like jpg/jpeg)
- `is_safe_archive_path(name) -> bool`: Validates archive entry paths against traversal attacks
- `sanitize_filename(name) -> String`: Replaces unsafe characters with underscores
- `ImageToExtract` struct: `{ index: usize, extension: String }` -- pending extraction descriptor
- `get_unique_output_path(dir, base, seq, total, ext) -> Result<PathBuf>`: Generates collision-free output path
- `write_image_to_file(path, data) -> Result<()>`: Writes bytes to file with BufWriter
- Unit tests for `sanitize_filename`, `normalize_format`, `get_supported_extensions`, `is_safe_archive_path`

**`src/docx.rs` (85 lines) -- DOCX processor:**
- `process_file(input_path, output_base_dir, allowed_extensions) -> Result<usize>`: Opens DOCX as ZIP, scans for image entries, extracts matching ones
- Uses `zip::ZipArchive` for archive traversal
- Uses `common::ImageToExtract` for two-pass scan-then-extract pattern

**`src/epub.rs` (439 lines) -- EPUB processor:**
- `EpubFilter` struct: `{ title: Option<String>, author: Option<String> }` -- filter criteria
  - `is_empty() -> bool`: Checks if any filter is set
  - `description() -> String`: Human-readable filter description for progress messages
- `check_filter_match(path, filter) -> Result<bool>`: Checks if EPUB matches filter without extracting
- `get_metadata(path) -> Result<(Option<String>, Option<String>)>`: Returns (author, title) tuple
- `get_base_name(path) -> Result<String>`: Returns sanitized "Author - Title" display name
- `process_file(input_path, output_base_dir, allowed_extensions, cover_only, cover_fallback, filter) -> Result<usize>`: Main EPUB processing entry point
- `extract_all_images(doc, ...) -> Result<usize>`: Extracts all images from EPUB resources (private)
- `extract_cover_only(doc, ...) -> Result<usize>`: Extracts cover image with fallback chain (private)
- `find_cover_by_filename(doc) -> Option<(Vec<u8>, String)>`: Heuristic cover detection by filename (private)
- `mime_to_extension(mime) -> Option<String>`: Converts MIME type to file extension (private)
- `matches_filter(title, author, filter) -> bool`: Case-insensitive substring match (private)
- `format_epub_base_name(author, title, fallback) -> String`: Builds output filename from metadata (private)
- `JPEG_EXTENSIONS` constant: `["jpg", "jpeg", "jpe", "jfif"]` for cover filename detection
- `EpubImage` struct (private): `{ id: String, extension: String }` -- resource descriptor
- Unit tests for `format_epub_base_name`, `mime_to_extension`, `JPEG_EXTENSIONS`

## Entry Points

**CLI Entry:**
- `src/main.rs` `fn main() -> Result<()>` (line 299)
- Invoked as: `word-image-extractor [inputs...] [options]`
- Binary name defined in `Cargo.toml`: `word-image-extractor`

**CLI Flow:**
1. `Args::parse()` -- clap processes argv
2. Merge positional and `--input` args; default to CWD
3. Resolve output directory (default: `.`)
4. Build extension filter set
5. Build `EpubFilter` from `--title`/`--author` args
6. `collect_document_files()` -- gather all .docx/.epub files
7. `filter_epub_files_with_progress()` -- apply metadata filter (if set)
8. `deduplicate_by_metadata()` -- remove duplicate EPUBs
9. Loop over files: `process_file()` dispatches to `docx::process_file()` or `epub::process_file()`
10. Print summary via progress bar

## Configuration Files

**`Cargo.toml`:**
- Package: `word-image-extractor` v0.3.0, Rust edition 2024
- Dependencies: `zip` 2.1.0, `clap` 4.5.4 (derive feature), `anyhow` 1.0.82, `walkdir` 2.5.0, `epub` 2.1.4, `indicatif` 0.18.3
- Release profile: `opt-level = 3`, `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"`

**`Cargo.lock`:**
- Pinned dependency versions (committed to repo)

**`.gitignore`:**
- Ignores `/target` (build artifacts) and `Word-GIF-Extractor.code-workspace`

## Naming Conventions

**Files:**
- Rust source files: lowercase `snake_case.rs` (e.g., `common.rs`, `docx.rs`, `epub.rs`)
- No nested module directories; flat `src/` layout

**Modules:**
- Module names match filenames: `mod common;`, `mod docx;`, `mod epub;`
- Public items use `pub fn` / `pub struct`; private helpers are unqualified

**Functions:**
- `snake_case` throughout (Rust convention)
- Public API functions: `process_file`, `check_filter_match`, `get_metadata`, `get_base_name`
- Private helpers: `extract_all_images`, `extract_cover_only`, `find_cover_by_filename`, `matches_filter`

## Where to Add New Code

**New Document Format (e.g., PDF):**
1. Create `src/{format}.rs` with a `pub fn process_file(input_path, output_base_dir, allowed_extensions, ...) -> Result<usize>`
2. Add `mod {format};` declaration in `src/main.rs` (line 8 area)
3. Add variant to `DocumentType` enum in `src/main.rs` (line 61)
4. Add extension mapping in `get_document_type()` in `src/main.rs` (line 68)
5. Add dispatch arm in `process_file()` in `src/main.rs` (line 270)
6. Add any new dependencies to `Cargo.toml`

**New Shared Utility Function:**
- Add to `src/common.rs`
- Export with `pub fn`
- Import in consumers via `use crate::common::{function_name};`

**New CLI Flag:**
- Add field to `Args` struct in `src/main.rs` (lines 21-58)
- Use `#[arg(...)]` attribute for clap configuration
- Wire into processing pipeline in `fn main()`

**New Image Format Support:**
- Add extension string(s) to `get_supported_extensions()` in `src/common.rs` (line 10)
- Add normalization case to `normalize_format()` in `src/common.rs` (line 17)
- If EPUB: add MIME mapping to `mime_to_extension()` in `src/epub.rs` (line 359)

**New Tests:**
- Unit tests go in `#[cfg(test)] mod tests {}` blocks at the bottom of each source file
- No separate `tests/` directory exists; integration tests would go in a new `tests/` directory at the project root

## Special Directories

**`target/`:**
- Purpose: Cargo build artifacts (debug and release binaries, dependency compilations)
- Generated: Yes (by `cargo build`)
- Committed: No (in `.gitignore`)
- Release binary location: `target/release/word-image-extractor.exe`

**`.claude/`:**
- Purpose: Claude Code configuration
- Generated: Partially (worktrees are auto-managed)
- Committed: Yes (`CLAUDE.md` is committed)

---

*Structure analysis: 2026-04-01*
