## Why

The current image extraction logic relies solely on file extensions to determine image formats within .docx archives. This is unreliable because embedded images may have incorrect, missing, or generic extensions (e.g., `.bin`). Adding binary magic value detection allows us to identify the true format by inspecting the file header bytes, improving robustness and accuracy.

## What Changes

- Add a binary magic value detection module that inspects the first bytes of extracted files to determine their true image format.
- Support magic value detection for all existing formats: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico.
- Update the extraction pipeline to use magic value detection as the primary format identification method, with file extension as a fallback.
- Ensure zero breaking changes to the CLI interface or output naming conventions.

## Capabilities

### New Capabilities
- `binary-magic-detection`: Detect image formats by inspecting binary file headers (magic values) rather than relying solely on file extensions.

### Modified Capabilities
- None. No existing spec-level behavior changes.

## Impact

- Affects the image extraction pipeline in `src/main.rs` (or a new module if refactored).
- Introduces a new internal module for magic value detection with no new external dependencies.
- CLI arguments, output directory structure, and naming conventions remain unchanged.
