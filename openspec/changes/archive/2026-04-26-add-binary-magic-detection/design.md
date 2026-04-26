## Context

The current image extraction pipeline in `src/main.rs` identifies image formats solely by inspecting the file extension of entries within the .docx ZIP archive. This is fragile because:

- Word documents may embed images with generic or incorrect extensions (e.g., `image1.bin`, `image2`)
- Users may request extraction of formats that are mislabeled in the archive
- The tool currently skips files that do not match the expected extension list

Binary magic value detection (inspecting the file header bytes) is a standard, reliable method for format identification used by tools like `file` and `libmagic`.

## Goals / Non-Goals

**Goals:**
- Detect image formats by reading the first N bytes of each extracted file
- Support all currently supported formats: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico
- Use magic value detection as the primary format identifier, with file extension as fallback
- Maintain zero breaking changes to the CLI interface and output file naming conventions
- Keep the implementation lightweight with no new external crates

**Non-Goals:**
- Full MIME type detection or integration with `libmagic`
- Deep content inspection beyond header magic bytes
- Changing the output directory structure or naming scheme
- Modifying archive traversal or ZIP handling logic

## Decisions

**Decision 1: Pure-Rust magic detection table instead of external crate**
- **Rationale**: The supported formats are fixed and well-documented. A hardcoded match table of magic byte sequences avoids adding a dependency like `magic` or `infer`, keeping the binary small and build times fast.
- **Alternative considered**: Using the `infer` crate. Rejected because it adds an external dependency for a bounded, static requirement.

**Decision 2: Read only the first 12 bytes of each file**
- **Rationale**: All supported formats have magic values within the first 12 bytes (e.g., PNG: first 8 bytes, JPEG: first 3 bytes, WebP: first 12 bytes). This minimizes I/O overhead when scanning ZIP entries.
- **Alternative considered**: Reading full files into memory. Rejected because it is unnecessary and slow for large archives.

**Decision 3: Integrate detection into the existing per-file extraction loop**
- **Rationale**: The detection should run after reading the file content from the ZIP but before deciding whether to write it to disk. This allows the extension filter (`-f`) to operate on the true detected format.
- **Alternative considered**: A separate pre-scan pass. Rejected because it would require reading each file twice.

**Decision 4: Extension fallback with a logged warning**
- **Rationale**: If magic detection fails (e.g., for corrupted files or formats without reliable magic bytes like SVG), the existing extension-based logic preserves backward compatibility. A warning informs the user that the true format could not be verified.

## Risks / Trade-offs

- **[Risk]** SVG files do not have a reliable binary magic value (they are text-based). → **Mitigation**: SVG detection falls back to extension or a simple check for the `<?xml` or `<svg` prefix in the first few bytes.
- **[Risk]** WMF and EMF formats have overlapping or ambiguous headers. → **Mitigation**: Use the specific WMF magic value (`0x01 0x00 0x09 0x00` or `0xD7 0xCD 0xC6 0x9A`) and EMF magic value (`0x01 0x00 0x00 0x00` at offset 40, but check first 4 bytes for `EMF` placeholder if available). Document known limitations.
- **[Risk]** TIFF files may be big-endian or little-endian. → **Mitigation**: Check for both `II*` (little-endian) and `MM\0*` (big-endian) magic sequences.
