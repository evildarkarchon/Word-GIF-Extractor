## 1. Magic Detection Module

- [x] 1.1 Create a new internal module or function for binary magic value detection in `src/main.rs` (or `src/magic.rs` if refactored)
- [x] 1.2 Define magic byte constants for all supported formats: png, jpg, gif, bmp, tiff, webp, ico, wmf, emf
- [x] 1.3 Implement SVG text-based detection by checking for `<?xml` or `<svg` prefixes
- [x] 1.4 Implement the core detection function that reads the first 12 bytes and returns the detected format string

## 2. Integration with Extraction Pipeline

- [x] 2.1 Update the per-file extraction loop to read file content from the ZIP before format filtering
- [x] 2.2 Call the magic detection function on each file's byte content
- [x] 2.3 Use the detected format as the primary input for the format filter (`-f`)
- [x] 2.4 Implement extension fallback when magic detection returns `None`, with a warning log
- [x] 2.5 Ensure the output file extension matches the detected format, not the original archive extension

## 3. Testing and Verification

- [x] 3.1 Add unit tests for each magic value detection scenario (png, jpg, gif, bmp, tiff, webp, ico, wmf, emf)
- [x] 3.2 Add unit tests for extension fallback behavior
- [x] 3.3 Add unit tests for SVG text-based detection
- [x] 3.4 Add integration test verifying that a mislabeled `.bin` PNG file is correctly extracted when `-f png` is used
- [x] 3.5 Run `cargo test` and fix any failures

## 4. Build and Validation

- [x] 4.1 Run `cargo build --release` to verify no compilation errors
- [x] 4.2 Run `cargo clippy` and resolve any warnings
- [x] 4.3 Manually test against a sample .docx with renamed/mislabeled image extensions
- [x] 4.4 Update `AGENTS.md` if any build commands or architecture details changed
