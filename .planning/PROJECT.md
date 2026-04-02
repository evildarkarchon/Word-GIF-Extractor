# Word/EPUB Image Extractor — Conversion & GIF Features

## What This Is

A Rust CLI tool that extracts images from Microsoft Word (.docx) and EPUB documents. The tool treats these files as ZIP archives, scans for image entries, and writes them to disk with intelligent naming. This milestone adds image format conversion and GIF-specific extraction workflows.

## Core Value

Extracted images are consistently in the user's desired format — no manual conversion step after extraction.

## Requirements

### Validated

- ✓ Extract images from DOCX files via ZIP traversal — existing
- ✓ Extract images from EPUB files with metadata-based naming — existing
- ✓ Recursive directory processing with `-r` flag — existing
- ✓ Format filtering with `-f` flag (e.g., `-f png,gif`) — existing
- ✓ EPUB cover-only extraction with `-c` and `--cover-fallback` — existing
- ✓ EPUB metadata filtering by `--title` and `--author` — existing
- ✓ EPUB deduplication by (author, title) pair — existing
- ✓ Path traversal protection and filename sanitization — existing
- ✓ Progress bars and spinners via indicatif — existing

### Active

- [ ] Convert extracted images to a single target format (jpg, png, or webp) via `--convert` flag
- [ ] GIF-only extraction mode via `--gif-only` flag
- [ ] Separate GIF output directory via `--gif-output <path>` flag
- [ ] `--convert` and `--gif-only` are mutually exclusive (error if both specified)
- [ ] Conversion outputs only the converted file (no original kept)

### Out of Scope

- Multiple target formats per run (e.g., `--convert png,webp`) — unnecessary complexity, run twice instead
- Keeping both original and converted files — user chose converted-only for cleaner output
- Video/animation extraction — out of scope for this tool
- Lossy-to-lossless quality settings — can add later if needed

## Context

- Brownfield: existing ~1,200-line Rust CLI with 4 source files
- Two format processors (DOCX, EPUB) behind a simple match dispatch — no trait abstraction
- The `image` crate will be added for format conversion (supports jpg, png, webp natively)
- Current architecture: extract raw bytes from archive → write to disk. Conversion inserts a decode→encode step before writing.
- GIF separation is a routing concern (which output directory), not a conversion concern

## Constraints

- **Rust edition**: 2024 (requires Rust >= 1.85)
- **Dependency**: `image` crate for conversion — must handle the formats users actually encounter in DOCX/EPUB (jpg, png, gif, bmp, tiff, webp)
- **Performance**: Conversion adds CPU cost per image; acceptable for a CLI batch tool
- **Compatibility**: Some archive images may be WMF/EMF (Windows metafiles) — `image` crate does not support these; conversion should skip unsupported formats with a warning

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| One target format per `--convert` run | Simpler CLI, user can run twice for two formats | — Pending |
| `image` crate for conversion | Standard Rust image processing, supports all target formats | Validated in Phase 1 |
| `--convert` and `--gif-only` mutually exclusive | They conflict logically — one converts away from formats, the other filters to GIF | Validated in Phase 3 |
| Converted-only output (no originals) | Cleaner output directory, matches user intent | — Pending |
| `webp` crate for lossy WebP | `image` crate's built-in WebP encoder is lossless-only; `webp` wraps libwebp for lossy | Validated in Phase 1 |
| JPEG quality 85 default | Higher than `image` crate default (75), better visual quality for typical photos | Validated in Phase 1 |
| Alpha compositing on white | JPEG has no alpha; transparent pixels composite against white (not black) | Validated in Phase 1 |
| `--gif-output <path>` for GIF separation | Explicit user-specified path gives full control | Validated in Phase 3 |
| Skip unsupported formats during conversion | WMF/EMF/SVG can't be decoded by `image` crate; warn and extract as-is | Validated in Phase 2 |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd:transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd:complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-04-02 after Phase 3 completion*
