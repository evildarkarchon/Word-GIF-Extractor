# Roadmap: Conversion & GIF Features

## Overview

This milestone adds image format conversion and GIF-specific extraction to an existing Rust CLI tool. The work progresses from a standalone conversion module (highest technical risk, best testability) through CLI arguments and GIF routing, then integrates through both DOCX and EPUB processors. Each phase delivers a verifiable capability that builds toward the end goal: extracted images consistently in the user's desired format with no manual conversion step.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Conversion Module Core** - Build convert.rs with image decoding, format conversion, alpha compositing, and WebP lossy encoding (completed 2026-04-02)
- [ ] **Phase 2: Format Handling and Output Naming** - Skip unsupported vector formats with warning, correct extension replacement on converted files
- [ ] **Phase 3: CLI Arguments and Validation** - Add --convert, --gif-only, --gif-output, --quality flags with conflict rules
- [ ] **Phase 4: GIF Extraction and Routing** - Implement GIF-only filtering and separate GIF output directory routing
- [ ] **Phase 5: DOCX Pipeline Integration** - Thread conversion and GIF features through DOCX processor for end-to-end operation
- [ ] **Phase 6: EPUB Pipeline Integration** - Thread conversion and GIF features through EPUB processor including cover-only mode

## Phase Details

### Phase 1: Conversion Module Core
**Goal**: Image format conversion works correctly at the module level -- decoding source formats, encoding to target formats, and handling transparency
**Depends on**: Nothing (first phase)
**Requirements**: CONV-02, CONV-05
**Success Criteria** (what must be TRUE):
  1. A PNG image with transparency converts to JPEG with a white background (not black) when processed through the conversion module
  2. A BMP image converts to PNG, JPEG, or WebP with correct pixel data when processed through the conversion module
  3. WebP output uses lossy encoding (converted file is smaller than lossless equivalent for photographic content)
  4. JPEG output uses quality 85 by default (not the image crate's default of 75)
  5. Unit tests pass for all supported source-to-target format combinations (JPEG, PNG, GIF, BMP, TIFF, WebP as sources; JPEG, PNG, WebP as targets)
**Plans:** 1/1 plans complete
Plans:
- [x] 01-01-PLAN.md -- Build convert.rs module with OutputFormat enum, can_convert(), convert_image(), alpha compositing, and unit tests

### Phase 2: Format Handling and Output Naming
**Goal**: Conversion handles edge cases gracefully -- unsupported formats are skipped with clear feedback, and converted files have correct extensions
**Depends on**: Phase 1
**Requirements**: CONV-03, CONV-04
**Success Criteria** (what must be TRUE):
  1. When conversion encounters an SVG, WMF, or EMF file, a warning is printed to stderr and the file is extracted with its original extension and raw bytes
  2. A converted file uses only the target format extension (e.g., an original `document_1.bmp` becomes `document_1.png`, not `document_1.bmp.png`)
  3. Unconverted files (skipped due to unsupported format) retain their original filename and extension
**Plans**: TBD

### Phase 3: CLI Arguments and Validation
**Goal**: Users can configure conversion and GIF features via well-validated command-line flags
**Depends on**: Phase 1
**Requirements**: CONV-06, CONV-07, GIF-04
**Success Criteria** (what must be TRUE):
  1. Running with `--convert jpg --quality 90` produces JPEG output at quality 90
  2. Running with `--quality 90 --convert png` produces an error message explaining that --quality is only valid with --convert jpg
  3. Running with `--convert jpg --gif-only` produces an error message explaining the flags are mutually exclusive
  4. Running with `--help` shows all new flags (--convert, --quality, --gif-only, --gif-output) with descriptions
**Plans**: TBD

### Phase 4: GIF Extraction and Routing
**Goal**: Users can extract only GIFs and route GIF files to a dedicated output directory
**Depends on**: Phase 3
**Requirements**: GIF-01, GIF-02, GIF-03
**Success Criteria** (what must be TRUE):
  1. Running with `--gif-only` extracts only GIF files and skips all other image formats
  2. Running with `--gif-output /path/to/gifs` writes GIF files to the specified directory while other formats go to the default output directory
  3. `--gif-output` routes GIFs to the separate directory even without `--gif-only` (when extracting all formats, GIFs go to the GIF directory and non-GIFs go to the default directory)
**Plans**: TBD

### Phase 5: DOCX Pipeline Integration
**Goal**: Users can convert images extracted from DOCX files end-to-end
**Depends on**: Phase 2, Phase 4
**Requirements**: CONV-01
**Success Criteria** (what must be TRUE):
  1. Running `word-image-extractor document.docx --convert png` extracts all images from the DOCX and writes them as PNG files in the output directory
  2. A DOCX containing mixed formats (PNG, JPEG, GIF, BMP, WMF) produces converted files for supported formats and raw extractions for unsupported formats, with warnings
  3. GIF routing works for DOCX extraction -- `--gif-output` correctly separates GIF files from converted non-GIF files
  4. Conversion errors on individual images do not abort the batch -- remaining images are still processed
**Plans**: TBD

### Phase 6: EPUB Pipeline Integration
**Goal**: Users can convert images extracted from EPUB files end-to-end, including cover-only mode
**Depends on**: Phase 5
**Requirements**: (extends CONV-01 to EPUB -- CONV-01 is fully delivered when both DOCX and EPUB work)
**Success Criteria** (what must be TRUE):
  1. Running `word-image-extractor book.epub --convert webp` extracts all images from the EPUB and writes them as WebP files
  2. Cover-only mode (`-c`) combined with `--convert png` extracts and converts only the cover image
  3. GIF routing works for EPUB extraction -- `--gif-output` correctly separates GIF files
  4. All EPUB extraction modes (all images, cover only, metadata-filtered) work correctly with conversion enabled
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Conversion Module Core | 1/1 | Complete   | 2026-04-02 |
| 2. Format Handling and Output Naming | 0/0 | Not started | - |
| 3. CLI Arguments and Validation | 0/0 | Not started | - |
| 4. GIF Extraction and Routing | 0/0 | Not started | - |
| 5. DOCX Pipeline Integration | 0/0 | Not started | - |
| 6. EPUB Pipeline Integration | 0/0 | Not started | - |
