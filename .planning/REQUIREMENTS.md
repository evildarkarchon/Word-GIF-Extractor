# Requirements: Word/EPUB Image Extractor — Conversion & GIF Features

**Defined:** 2026-04-02
**Core Value:** Extracted images are consistently in the user's desired format — no manual conversion step after extraction.

## v1 Requirements

Requirements for this milestone. Each maps to roadmap phases.

### Conversion

- [ ] **CONV-01**: User can convert all extracted images to a single target format via `--convert <jpg|png|webp>`
- [x] **CONV-02**: JPEG conversion composites alpha channels against a white background (transparent regions must not render as black)
- [ ] **CONV-03**: Unsupported source formats (SVG, WMF, EMF) are skipped during conversion with a warning to stderr, and extracted as raw bytes with their original extension
- [ ] **CONV-04**: Converted output strips the original file extension and uses only the target format extension (e.g., `document_1.bmp` becomes `document_1.png`, not `document_1.bmp.png`)
- [x] **CONV-05**: JPEG conversion uses quality 85 by default
- [ ] **CONV-06**: User can override JPEG quality via `--quality <1-100>`
- [ ] **CONV-07**: `--quality` is only valid with `--convert jpg` (produces an error if used with png or webp)

### GIF Extraction

- [ ] **GIF-01**: User can extract only GIF files via `--gif-only` flag (all non-GIF image formats are skipped)
- [ ] **GIF-02**: User can route extracted GIF files to a separate directory via `--gif-output <path>`
- [ ] **GIF-03**: `--gif-output` works independently of `--gif-only` — GIFs are routed to the GIF output directory even when extracting all formats
- [ ] **GIF-04**: `--convert` and `--gif-only` are mutually exclusive (error if both specified)

## v2 Requirements

Deferred to future milestones. Tracked but not in current roadmap.

### Conversion Enhancements

- **CONV-08**: Conversion summary statistics displayed after batch run (X converted, Y skipped, Z failed)
- **CONV-09**: Dry-run mode (`--dry-run`) to preview what would be extracted and converted without writing files
- **CONV-10**: WebP lossy encoding with quality parameter (requires `webp` crate or `libwebp-sys`)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Multiple target formats per run (`--convert png,webp`) | Unnecessary complexity — run twice instead |
| Keep both original and converted files | User chose converted-only for cleaner output |
| Image resizing during conversion | Different workflow — use ImageMagick or sic |
| Animated GIF frame extraction | Specialized workflow requiring frame compositing |
| AVIF output format | Requires C dependencies (dav1d, rav1e) — revisit when pure-Rust AVIF matures |
| SVG-to-raster conversion | Requires rendering engine (resvg/tiny-skia) — heavy dependency for niche case |
| Interactive format selection (TUI) | Batch CLI tool — all configuration via flags |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| CONV-01 | Phase 5 | Pending |
| CONV-02 | Phase 1 | Complete |
| CONV-03 | Phase 2 | Pending |
| CONV-04 | Phase 2 | Pending |
| CONV-05 | Phase 1 | Complete |
| CONV-06 | Phase 3 | Pending |
| CONV-07 | Phase 3 | Pending |
| GIF-01 | Phase 4 | Pending |
| GIF-02 | Phase 4 | Pending |
| GIF-03 | Phase 4 | Pending |
| GIF-04 | Phase 3 | Pending |

**Coverage:**
- v1 requirements: 11 total
- Mapped to phases: 11
- Unmapped: 0

---
*Requirements defined: 2026-04-02*
*Last updated: 2026-04-02 after roadmap creation*
