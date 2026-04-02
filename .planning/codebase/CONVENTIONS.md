# Coding Conventions

**Analysis Date:** 2026-04-01

## Naming Patterns

**Files:**
- Use lowercase `snake_case.rs` for all source files
- Module names match filenames: `common.rs`, `docx.rs`, `epub.rs`
- Single entry point: `main.rs`

**Functions:**
- Use `snake_case` for all functions: `get_document_type`, `is_supported_document`, `extract_cover_only`
- Prefix boolean-returning functions with `is_` or `matches_`: `is_epub()`, `is_safe_archive_path()`, `matches_filter()`
- Prefix getter functions with `get_`: `get_metadata()`, `get_base_name()`, `get_supported_extensions()`
- Use verb-noun pattern for action functions: `process_file()`, `collect_document_files()`, `write_image_to_file()`

**Variables:**
- Use `snake_case` for all variables and parameters
- Descriptive names preferred: `allowed_extensions`, `output_base_dir`, `cover_fallback`
- Short names acceptable for iterators: `i`, `e`, `c`

**Types/Structs:**
- Use `PascalCase` for structs and enums: `Args`, `DocumentType`, `EpubFilter`, `ImageToExtract`, `EpubImage`
- Derive macros on structs: `#[derive(Debug, Clone)]` for data structs, `#[derive(Parser, Debug)]` for CLI

**Constants:**
- Use `SCREAMING_SNAKE_CASE`: `JPEG_EXTENSIONS`, `MAX_ATTEMPTS`
- Constants defined with `const` at module level

## Code Style

**Formatting:**
- Default `rustfmt` formatting (no `.rustfmt.toml` present)
- 4-space indentation
- Trailing commas in multi-line expressions
- Opening brace on same line as declaration
- Run `cargo fmt` before committing

**Linting:**
- Default `clippy` rules (no `clippy.toml` present)
- Run `cargo clippy` to check
- No `#[allow(...)]` annotations present in codebase -- all clippy warnings are resolved

**Rust Edition:**
- Rust 2024 edition (`edition = "2024"` in `Cargo.toml`)
- Uses 2024-edition features like `let chains` in if-let expressions (see `src/epub.rs` line 271-274)

## Import Organization

**Order:**
1. External crate imports (`anyhow`, `clap`, `epub`, `indicatif`, `std`, `walkdir`, `zip`)
2. Standard library imports (`std::collections`, `std::fs`, `std::io`, `std::path`)
3. Local crate imports (`crate::common::...`, `common::...`, `epub::...`)

**Style:**
- Group related imports with `use` blocks separated by blank lines
- Use braced imports for multiple items from same module: `use crate::common::{get_unique_output_path, is_safe_archive_path, sanitize_filename, write_image_to_file};`
- Prefer explicit imports over glob imports (no `use x::*` anywhere)

**Path Aliases:**
- None configured. All imports use full paths relative to crate root.

## Error Handling

**Framework:** `anyhow` crate for application-level error handling

**Patterns:**
- All fallible public functions return `anyhow::Result<T>`
- Use `.context()` / `.with_context()` for adding context to errors:
  ```rust
  fs::File::open(input_path)
      .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
  ```
- Use `anyhow::bail!()` macro for explicit error returns:
  ```rust
  anyhow::bail!("Unsupported file type: {}", input_path.display());
  ```
- Use `.map_err()` to convert library errors that don't implement `std::error::Error`:
  ```rust
  EpubDoc::new(input_path).map_err(|e| anyhow::anyhow!("Failed to open EPUB file: {}", e))?;
  ```
- Non-fatal errors use `eprintln!("Warning: ...")` and continue processing (see `src/main.rs` lines 178-183, `src/common.rs` line 31)
- The `?` operator is used consistently for propagation

**Error Context Convention:**
- Always include the file path in error context messages
- Use format: `"Failed to {action}: {path}"` (e.g., "Failed to create output file: /path/to/file")

## Logging

**Framework:** Direct `eprintln!` for warnings, `println!` for informational output, `indicatif` for progress bars

**Patterns:**
- Warnings to stderr: `eprintln!("Warning: ...")`
- Progress indication via `indicatif` crate with two styles:
  - `ProgressBar` with bar template for known-count operations (see `create_progress_style()` in `src/main.rs`)
  - `ProgressBar::new_spinner()` for unknown-count operations (see `create_spinner_style()` in `src/main.rs`)
- Use `pb.suspend(|| { eprintln!(...) })` to print warnings without corrupting progress bar output
- No structured logging or log levels -- this is a CLI tool

## Documentation Comments

**Module-level docs:**
- Every module has a `//!` doc comment at the top:
  - `src/main.rs`: `//! Word Image Extractor - A CLI tool for extracting images from DOCX and EPUB files.`
  - `src/common.rs`: `//! Common utilities shared between document processors`
  - `src/docx.rs`: `//! DOCX file processing module`
  - `src/epub.rs`: `//! EPUB file processing module`

**Function-level docs:**
- All public functions have `///` doc comments explaining purpose and return value
- Private functions with non-obvious behavior also have `///` doc comments
- Comments describe WHAT the function does, not HOW:
  ```rust
  /// Processes a single .docx file, extracting images matching the allowed extensions.
  /// Returns the number of images extracted.
  pub fn process_file(...) -> Result<usize>
  ```

**Inline comments:**
- Used sparingly for non-obvious decisions or crate-specific API notes:
  ```rust
  // 'creator' is the Dublin Core element for author
  // create_dir_all is idempotent - succeeds if directory exists
  // Defense-in-depth: skip entries with path traversal patterns
  ```
- Comments explain WHY, not WHAT (the code explains what)

**Struct field docs:**
- Public struct fields have `///` doc comments (see `ImageToExtract` in `src/common.rs`)
- CLI arg fields use `///` comments that become help text via clap derive (see `Args` in `src/main.rs`)

## Function Design

**Size:** Functions are generally short (under 50 lines). Larger orchestration functions like `main()` and `extract_cover_only()` are up to ~80 lines.

**Parameters:**
- Pass `&Path` for filesystem paths (not `PathBuf` or `&str`)
- Pass `&HashSet<&str>` for extension filters (borrowed, not owned)
- Pass `&EpubFilter` for filter criteria (borrowed reference)
- Mutable references only when needed: `&mut EpubDoc<...>`

**Return Values:**
- Use `Result<usize>` for extraction functions (count of items extracted)
- Use `Result<bool>` for filter/match functions
- Use `Option<T>` for lookups that may not find anything
- Return `Ok(0)` for "nothing to do" cases (not an error)

## Module Design

**Exports:**
- Public functions and types exported explicitly with `pub`
- Internal helpers are private (no `pub`)
- No barrel files or re-exports

**Module boundaries:**
- `common` provides shared utilities used by both `docx` and `epub`
- `docx` and `epub` are independent of each other
- `main` orchestrates everything and depends on all three modules

**Cross-module dependencies:**
```
main.rs --> common.rs (get_supported_extensions, normalize_format)
main.rs --> epub.rs (EpubFilter, check_filter_match, get_metadata, get_base_name)
main.rs --> docx.rs (process_file)
docx.rs --> common.rs (ImageToExtract, get_unique_output_path, is_safe_archive_path, write_image_to_file)
epub.rs --> common.rs (get_unique_output_path, is_safe_archive_path, sanitize_filename, write_image_to_file)
```

## CLI Argument Patterns

**Framework:** `clap` with derive macros

**Conventions:**
- Use `#[derive(Parser)]` on the `Args` struct in `src/main.rs`
- Short flags are single letters: `-i`, `-o`, `-r`, `-f`, `-c`
- Long flags use kebab-case: `--input`, `--output`, `--recursive`, `--formats`, `--cover-only`, `--cover-fallback`
- Positional arguments come first, named args after
- Use `#[arg(requires = "...")]` for dependent flags (e.g., `--cover-fallback` requires `--cover-only`)
- Default values handled in `main()` logic, not in clap attributes

---

*Convention analysis: 2026-04-01*
