# Testing Patterns

**Analysis Date:** 2026-04-01

## Test Framework

**Runner:**
- Built-in Rust test framework (`#[cfg(test)]` / `#[test]`)
- No external test runner or harness
- Config: None (default `cargo test` behavior)

**Assertion Library:**
- Standard library `assert!`, `assert_eq!`, `assert_ne!` macros
- No additional assertion crates

**Run Commands:**
```bash
cargo test                    # Run all 18 tests
cargo test -- --list          # List all available tests
cargo test common::           # Run tests in common module only
cargo test epub::             # Run tests in epub module only
cargo test -- --nocapture     # Show println output during tests
```

## Test File Organization

**Location:**
- Co-located with source code using `#[cfg(test)] mod tests` blocks at the bottom of each module file
- No separate `tests/` directory for integration tests

**Naming:**
- Test function names use `test_` prefix followed by function-under-test and scenario:
  - `test_sanitize_filename`
  - `test_normalize_format`
  - `test_is_safe_archive_path_valid`
  - `test_is_safe_archive_path_traversal`
  - `test_format_epub_base_name_both`
  - `test_format_epub_base_name_empty_strings`

**Structure:**
```
src/
├── common.rs    # 9 unit tests (sanitize, normalize, extensions, path safety)
├── docx.rs      # 0 tests
├── epub.rs      # 9 unit tests (base name formatting, mime conversion, constants)
└── main.rs      # 0 tests
```

## Test Structure

**Suite Organization:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name_scenario() {
        // Arrange: set up inputs
        // Act: call the function
        // Assert: verify output
        assert_eq!(function_under_test(input), expected);
    }
}
```

**Patterns:**
- Tests are simple, single-assertion functions (most tests have 1-4 assertions)
- No setup/teardown (`before_each`/`after_each`) -- each test is self-contained
- No test fixtures or shared state
- Tests use `use super::*;` to access all items from the parent module

**Typical test structure (from `src/common.rs`):**
```rust
#[test]
fn test_sanitize_filename() {
    assert_eq!(sanitize_filename("Normal Name"), "Normal Name");
    assert_eq!(
        sanitize_filename("File/With\\Bad:Chars"),
        "File_With_Bad_Chars"
    );
    assert_eq!(sanitize_filename("Test*?\"<>|"), "Test______");
    assert_eq!(sanitize_filename("  Trimmed  "), "Trimmed");
}
```

**Grouped scenario tests (from `src/common.rs`):**
```rust
#[test]
fn test_is_safe_archive_path_valid() {
    assert!(is_safe_archive_path("word/media/image1.png"));
    assert!(is_safe_archive_path("image.jpg"));
    assert!(is_safe_archive_path("nested/folder/file.gif"));
}

#[test]
fn test_is_safe_archive_path_traversal() {
    assert!(!is_safe_archive_path("../etc/passwd"));
    assert!(!is_safe_archive_path("foo/../bar"));
    assert!(!is_safe_archive_path(".."));
}
```

## Mocking

**Framework:** None

**Patterns:**
- No mocking is used anywhere in the test suite
- Tests only cover pure functions that don't require mocking (string manipulation, format conversion, validation)
- Functions requiring file I/O or external crates (`EpubDoc`, `ZipArchive`) are NOT tested

**What is NOT mocked (and therefore NOT tested):**
- `zip::ZipArchive` operations in `src/docx.rs`
- `epub::doc::EpubDoc` operations in `src/epub.rs`
- File system operations (`fs::File::open`, `fs::create_dir_all`, file writing)
- Progress bar behavior (`indicatif`)

## Fixtures and Factories

**Test Data:**
- All test data is inline string literals and expected values
- No test fixture files (no `.docx` or `.epub` test files)
- No factory functions or builders

**Example (from `src/epub.rs`):**
```rust
#[test]
fn test_format_epub_base_name_both() {
    let result = format_epub_base_name(Some("Stephen King"), Some("The Shining"), "fallback");
    assert_eq!(result, "Stephen King - The Shining");
}
```

**Location:**
- No fixture directory exists
- If adding test fixtures, create `tests/fixtures/` at the project root

## Coverage

**Requirements:** None enforced. No coverage thresholds configured.

**Current Coverage by Module:**

| Module | Functions Tested | Functions Untested | Approximate Coverage |
|--------|------------------|--------------------|---------------------|
| `src/common.rs` | `sanitize_filename`, `normalize_format`, `get_supported_extensions`, `is_safe_archive_path` | `get_unique_output_path`, `write_image_to_file` | ~60% of public functions |
| `src/epub.rs` | `format_epub_base_name`, `mime_to_extension`, `JPEG_EXTENSIONS` constant | `process_file`, `extract_all_images`, `extract_cover_only`, `find_cover_by_filename`, `check_filter_match`, `get_metadata`, `get_base_name`, `matches_filter` | ~25% of functions |
| `src/docx.rs` | (none) | `process_file` | 0% |
| `src/main.rs` | (none) | `get_document_type`, `is_supported_document`, `is_epub`, `collect_document_files`, `filter_epub_files_with_progress`, `deduplicate_by_metadata`, `process_file`, `main` | 0% |

**View Coverage:**
```bash
# Requires cargo-tarpaulin or cargo-llvm-cov
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

## Test Types

**Unit Tests:**
- 18 unit tests total, all in `#[cfg(test)]` modules
- Test pure functions only (no side effects, no I/O)
- All tests pass and run in < 1 second
- Grouped by concern within each module's test block

**Integration Tests:**
- None. No `tests/` directory exists at the project root.
- No end-to-end tests that exercise the full extraction pipeline with real `.docx` or `.epub` files.

**E2E Tests:**
- Not present. CLI behavior is not tested programmatically.
- No `assert_cmd` or similar CLI testing framework.

## Common Patterns

**Pure Function Testing:**
```rust
#[test]
fn test_normalize_format() {
    assert_eq!(normalize_format("jpg"), vec!["jpg", "jpeg"]);
    assert_eq!(normalize_format("JPEG"), vec!["jpg", "jpeg"]);
    assert_eq!(normalize_format("png"), vec!["png"]);
    assert_eq!(normalize_format("unknown").len(), 0);
}
```

**Boundary/Edge Case Testing:**
```rust
#[test]
fn test_format_epub_base_name_empty_strings() {
    // Whitespace-only and empty strings should fall through to fallback
    let result = format_epub_base_name(Some("  "), Some(""), "fallback");
    assert_eq!(result, "fallback");
}
```

**Security Validation Testing (multiple scenarios per concern):**
```rust
#[test]
fn test_is_safe_archive_path_traversal() {
    assert!(!is_safe_archive_path("../etc/passwd"));
    assert!(!is_safe_archive_path("foo/../bar"));
    assert!(!is_safe_archive_path(".."));
}

#[test]
fn test_is_safe_archive_path_windows_drive() {
    assert!(!is_safe_archive_path("C:\\Windows\\System32\\calc.exe"));
    assert!(!is_safe_archive_path("D:file.txt"));
}
```

**Constant/Configuration Validation:**
```rust
#[test]
fn test_jpeg_extensions_contains_common_extensions() {
    assert!(JPEG_EXTENSIONS.contains(&"jpg"));
    assert!(JPEG_EXTENSIONS.contains(&"jpeg"));
    assert!(JPEG_EXTENSIONS.contains(&"jpe"));
    assert!(JPEG_EXTENSIONS.contains(&"jfif"));
}
```

## Adding New Tests

**When adding a new pure function:**
1. Add tests in the `#[cfg(test)] mod tests` block of the same file
2. Name: `test_{function_name}_{scenario}`
3. Test happy path, edge cases, and error cases
4. Use inline test data (no fixtures needed for pure functions)

**When adding integration tests:**
1. Create `tests/` directory at project root
2. Add test `.docx`/`.epub` files to `tests/fixtures/`
3. Use `assert_cmd` crate for CLI testing
4. Use `tempfile` crate for temporary output directories

**When adding I/O tests (e.g., for `get_unique_output_path`):**
1. Use `tempfile::tempdir()` for isolated filesystem operations
2. Create test files to verify collision handling
3. Clean up automatically via `TempDir` drop

---

*Testing analysis: 2026-04-01*
