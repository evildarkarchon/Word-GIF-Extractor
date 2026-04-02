# Codebase Concerns

**Analysis Date:** 2026-04-01

## Tech Debt

**Repeated EPUB file opening:**
- Issue: The same EPUB file is opened with `EpubDoc::new()` up to 3 times during a single processing pipeline: once in `check_filter_match()` (`src/epub.rs:60`), once in `get_metadata()` for deduplication (`src/epub.rs:73`), and once in `process_file()` for extraction (`src/epub.rs:142`). Additionally, `get_base_name()` (`src/epub.rs:90`) calls `get_metadata()` again for progress display in `src/main.rs:386-389`, making it 4 opens per file in the worst case (cover-only mode with filter + dedup).
- Files: `src/epub.rs:58-66`, `src/epub.rs:71-78`, `src/epub.rs:141-146`, `src/main.rs:386`
- Impact: Unnecessary I/O overhead, especially when processing large collections of EPUB files. Each open parses the ZIP central directory and XML metadata.
- Fix approach: Cache metadata during the filter pass. Return metadata alongside the match result from `check_filter_match()`, or introduce a metadata cache (`HashMap<PathBuf, (Option<String>, Option<String>)>`) populated during filtering/dedup and passed into the extraction phase.

**cover_only and cover_fallback passed but ignored for DOCX:**
- Issue: The `process_file()` dispatcher in `src/main.rs:270-297` accepts `cover_only` and `cover_fallback` parameters but silently ignores them when processing DOCX files (line 280 only passes `input_path`, `output_base_dir`, `allowed_extensions`). Users passing `--cover-only` with DOCX files get all images extracted with no warning.
- Files: `src/main.rs:270-297`, `src/docx.rs:16-20`
- Impact: User confusion -- the `--cover-only` flag silently does nothing for DOCX files. The summary message tries to compensate (`src/main.rs:426-428` with `has_docx_images` tracking) but this is a workaround, not a solution.
- Fix approach: Either (a) emit a warning when `--cover-only` is used with DOCX files, (b) skip DOCX files entirely in cover-only mode, or (c) implement cover detection for DOCX (the first image in `word/media/` is typically the cover).

**Duplicated cover extraction logic:**
- Issue: `extract_cover_only()` in `src/epub.rs:284-356` has two nearly identical blocks for writing the cover image -- one for the metadata-based cover (lines 296-317) and one for the filename-based fallback (lines 321-339). Both perform the same allowed-extension check, `create_dir_all`, `get_unique_output_path`, and `write_image_to_file` sequence.
- Files: `src/epub.rs:296-317`, `src/epub.rs:321-339`
- Impact: Maintenance burden -- any change to the write logic must be made in two places.
- Fix approach: Extract a helper function like `write_cover_image(data, mime, output_base_dir, base_name, allowed_extensions) -> Result<usize>` and call it from both branches.

**EPUB filter checked redundantly in process_file:**
- Issue: When filters are active, `filter_epub_files_with_progress()` in `src/main.rs:161-194` already filters out non-matching EPUBs. But `epub::process_file()` (`src/epub.rs:149`) checks the filter again and silently returns `Ok(0)` for non-matching files. This second check is dead code when the upstream filter has already run.
- Files: `src/main.rs:352-354`, `src/epub.rs:148-150`
- Impact: Minor -- the redundant check adds negligible overhead but is confusing for maintainers. It suggests the filter might not be applied upstream.
- Fix approach: Either remove the filter check from `epub::process_file()` (relying on the upstream filter) or document it as a defensive guard. If keeping it, add a comment explaining it is intentional defense-in-depth.

## Known Bugs

**No known bugs detected from static analysis.** The codebase handles errors consistently with `anyhow::Result` and provides context on failures.

## Security Considerations

**Archive path traversal -- well-handled:**
- Risk: Zip-slip attacks via malicious archive entries with `../` paths.
- Files: `src/common.rs:41-66`
- Current mitigation: `is_safe_archive_path()` checks for null bytes, `..` traversal, absolute Unix/Windows paths, Windows drive letters, and alternate data streams. This is thorough.
- Recommendations: The check is string-based. Consider also canonicalizing the resolved output path and verifying it is still within `output_base_dir` as a belt-and-suspenders approach. This would guard against edge cases where OS-specific path normalization differs from string checks.

**No file size limit on extracted images:**
- Risk: A malicious or corrupted archive could contain an image entry claiming to be very large. `read_to_end()` in `src/docx.rs:77` and `get_resource()` in `src/epub.rs:233` will allocate memory proportional to the entry's uncompressed size, potentially causing OOM on decompression bombs.
- Files: `src/docx.rs:77`, `src/epub.rs:233`
- Current mitigation: None.
- Recommendations: Add a configurable maximum file size (e.g., 100 MB) and skip entries that exceed it. For DOCX, check `file.size()` before calling `read_to_end()`. For EPUB, this depends on the `epub` crate's API.

**No zip bomb protection:**
- Risk: A crafted DOCX with extreme compression ratios could expand to gigabytes of data. The `zip` crate does not limit decompression ratios by default.
- Files: `src/docx.rs:29`, `src/docx.rs:77`
- Current mitigation: None.
- Recommendations: Track cumulative bytes extracted and abort if a threshold is exceeded. Alternatively, check compression ratio per entry (`compressed_size` vs `size`).

**Filename sanitization -- mostly safe but allows very long names:**
- Risk: EPUB metadata (author + title) can be arbitrarily long, producing filenames that exceed OS limits (260 chars on Windows by default, 255 bytes on most Linux filesystems).
- Files: `src/common.rs:70-80`, `src/epub.rs:101-113`
- Current mitigation: `sanitize_filename()` replaces dangerous characters but does not truncate.
- Recommendations: Truncate sanitized filenames to a safe maximum (e.g., 200 characters) to leave room for sequence numbers and extensions.

## Performance Bottlenecks

**All images read into memory before writing:**
- Problem: In `src/docx.rs:77`, each image is fully loaded into a `Vec<u8>` via `read_to_end()` before being written to disk. For the EPUB path, the `epub` crate's `get_resource()` similarly returns `Vec<u8>`.
- Files: `src/docx.rs:74-80`, `src/epub.rs:232-235`
- Cause: The zip crate supports streaming reads, but the code buffers entirely into memory. The epub crate does not expose streaming access.
- Improvement path: For DOCX, use `std::io::copy()` to stream directly from the zip entry reader to a `BufWriter<File>` instead of buffering. This would eliminate the per-image memory allocation for large files. For EPUB, this is constrained by the `epub` crate API.

**Sequential file processing:**
- Problem: All files are processed sequentially in a single thread (`src/main.rs:380-422`).
- Files: `src/main.rs:380-422`
- Cause: Simple loop design; no parallelism implemented.
- Improvement path: Use `rayon` for parallel file processing. Each file is independent (reads from its own archive, writes to uniquely-named outputs). The `get_unique_output_path()` function would need a mutex or atomic counter to avoid race conditions on filename collision checks.

**EPUB files opened multiple times (see Tech Debt above):**
- Problem: Same EPUB parsed 2-4 times during filter + dedup + display + extraction pipeline.
- Files: `src/epub.rs:58-66`, `src/epub.rs:71-78`, `src/epub.rs:141-146`
- Cause: Each public function independently opens the file.
- Improvement path: Introduce a metadata cache or restructure the pipeline to pass cached data forward.

## Fragile Areas

**get_unique_output_path counter logic:**
- Files: `src/common.rs:92-140`
- Why fragile: The function appends `_1`, `_2`, etc. to avoid collisions, but the counter is appended to the full stem (which may already contain `_N` from sequence numbering). For example, `Author - Title_3` could collide with `Author - Title` sequence index 3, leading to confusing filenames like `Author - Title_3_1`.
- Safe modification: If changing naming logic, always test with both single-image and multi-image documents, and with pre-existing files in the output directory.
- Test coverage: No tests for `get_unique_output_path()` -- this function is untested.

**WalkDir errors silently swallowed:**
- Files: `src/main.rs:132`, `src/main.rs:142`
- Why fragile: Both `WalkDir::new().into_iter().flatten()` and `entries.flatten()` silently discard directory traversal errors (permission denied, broken symlinks, etc.). The user gets no indication that some paths were skipped.
- Safe modification: Replace `.flatten()` with explicit error handling that logs warnings.
- Test coverage: No tests for `collect_document_files()`.

**Progress bar template expects -- panics on invalid template:**
- Files: `src/main.rs:93`, `src/main.rs:101`
- Why fragile: `.expect("Invalid progress bar template")` will panic if the template string is ever changed to an invalid format. These are compile-time-constant strings so the risk is low, but the pattern is technically a panic path in production code.
- Safe modification: These are acceptable as-is since the strings are hardcoded and validated at development time.
- Test coverage: None, but low risk.

## Scaling Limits

**Memory usage scales with largest single image:**
- Current capacity: Works fine for typical documents (images < 50 MB each).
- Limit: A single image larger than available RAM will cause an OOM crash due to `read_to_end()` buffering.
- Scaling path: Stream-write for DOCX images (see Performance section). For EPUB, this is limited by the `epub` crate.

**Filename collision counter capped at 1000:**
- Current capacity: `MAX_ATTEMPTS = 1000` in `src/common.rs:119`.
- Limit: If more than 1000 files with the same base name exist in the output directory, extraction fails with an error.
- Scaling path: Increase `MAX_ATTEMPTS` or use a different naming strategy (e.g., hash-based suffixes). This is unlikely to be hit in normal use.

## Dependencies at Risk

**`epub` crate (v2.1.4):**
- Risk: The `epub` crate returns non-standard error types (not `std::error::Error`-compatible), requiring `.map_err(|e| anyhow::anyhow!(...))` wrappers instead of the `?` operator with `anyhow::Context`. This suggests the crate may have ergonomic or maintenance concerns.
- Impact: Error handling boilerplate in `src/epub.rs:60`, `src/epub.rs:73`, `src/epub.rs:142`.
- Migration plan: Monitor the crate for updates. If it becomes unmaintained, consider switching to `epub-rs` or implementing direct ZIP+XML parsing using the `zip` and `quick-xml` crates.

**Rust edition 2024:**
- Risk: Using `edition = "2024"` in `Cargo.toml:4`. This is the newest edition and some tooling or CI environments may not yet fully support it.
- Impact: Build failures on older Rust toolchains.
- Migration plan: Document minimum supported Rust version (MSRV) in the project. Consider adding `rust-version` field to `Cargo.toml`.

## Test Coverage Gaps

**No tests for `src/docx.rs`:**
- What's not tested: The entire DOCX processing pipeline -- ZIP archive opening, image discovery, extension filtering, sequential extraction, output naming.
- Files: `src/docx.rs` (0 tests, 84 lines)
- Risk: Regressions in DOCX extraction would go unnoticed. This is the original core functionality of the tool.
- Priority: High

**No tests for `src/main.rs`:**
- What's not tested: CLI argument parsing, input path collection, file dispatching, progress bar integration, deduplication logic, filter-then-process pipeline, the entire `main()` flow.
- Files: `src/main.rs` (0 tests, 447 lines)
- Risk: Integration-level regressions. The `collect_document_files()`, `filter_epub_files_with_progress()`, and `deduplicate_by_metadata()` functions contain non-trivial logic that is entirely untested.
- Priority: High -- especially `deduplicate_by_metadata()` which has edge cases around missing metadata fallback to filename.

**No integration tests with real archives:**
- What's not tested: End-to-end extraction from actual `.docx` and `.epub` files. All existing tests are unit tests on pure helper functions.
- Files: No `tests/` directory exists.
- Risk: The tool's primary purpose (extracting images from real document files) has zero automated test coverage. Format-specific edge cases (encrypted archives, split archives, unusual EPUB structures) are untested.
- Priority: High

**`get_unique_output_path` untested:**
- What's not tested: Filename collision handling, counter increment logic, the MAX_ATTEMPTS boundary.
- Files: `src/common.rs:92-140`
- Risk: Subtle filename generation bugs. The interaction between sequence numbering and collision avoidance is a likely source of edge-case bugs.
- Priority: Medium

**`matches_filter` only tested implicitly:**
- What's not tested: The `matches_filter()` function in `src/epub.rs:41-53` has no direct tests. The EPUB test module tests `format_epub_base_name`, `mime_to_extension`, and `JPEG_EXTENSIONS` but not the filter matching logic.
- Files: `src/epub.rs:41-53`
- Risk: Filter logic regressions (e.g., case sensitivity issues, None-handling edge cases).
- Priority: Medium

## Missing Critical Features

**No logging framework:**
- Problem: All user-facing messages use `println!` / `eprintln!` with no log levels, no way to enable verbose/debug output, and no structured logging.
- Blocks: Debugging issues in production, adding verbose mode (`-v`/`--verbose`).

**No dry-run mode:**
- Problem: No way to preview what would be extracted without actually writing files.
- Blocks: Safe exploration of large document collections before committing to extraction.

**No output directory per-document option:**
- Problem: All images are extracted to a single flat directory. When processing many documents, the output directory becomes cluttered with hundreds of files using the `{docname}_{n}.{ext}` pattern.
- Blocks: Organized batch extraction workflows.

---

*Concerns audit: 2026-04-01*
