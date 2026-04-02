# External Integrations

**Analysis Date:** 2026-04-01

## APIs & External Services

**None.** This is a fully offline CLI tool with no network calls, no API keys, no external service dependencies.

## File Format Dependencies

### Input Formats

**DOCX (Microsoft Word):**
- Treated as a ZIP archive containing embedded images
- Library: `zip` crate (v2.x, resolved to 2.4.2)
- Client code: `src/docx.rs` - uses `zip::ZipArchive` to open and iterate entries
- Images located by scanning all ZIP entries for matching file extensions
- No XML parsing of Word content; purely extension-based image detection

**EPUB (Electronic Publication):**
- Parsed using the `epub` crate (v2.1.x, resolved to 2.1.5)
- Client code: `src/epub.rs` - uses `epub::doc::EpubDoc`
- Metadata access: Dublin Core fields via `doc.mdata("title")` and `doc.mdata("creator")`
- Cover detection: `doc.get_cover()` method, with fallback to filename-based detection (`find_cover_by_filename` in `src/epub.rs`)
- Image resources accessed via `doc.resources` HashMap and `doc.get_resource(&id)`
- The `epub` crate internally uses `zip 3.0.0` and `xml-rs` for OPF/container parsing

### Supported Image Formats (output)

Defined in `src/common.rs` `get_supported_extensions()`:

| Format | Extensions | Notes |
|--------|-----------|-------|
| JPEG | `jpg`, `jpeg` | Most common in both DOCX and EPUB |
| PNG | `png` | Common in both formats |
| GIF | `gif` | Animated GIFs preserved as-is |
| BMP | `bmp` | Bitmap images |
| TIFF | `tiff`, `tif` | Both extensions recognized |
| SVG | `svg` | Vector graphics |
| WMF | `wmf` | Windows Metafile (DOCX-specific) |
| EMF | `emf` | Enhanced Metafile (DOCX-specific) |
| WebP | `webp` | Modern web format |
| ICO | `ico` | Icon format |

MIME-to-extension mapping for EPUB resources in `src/epub.rs` `mime_to_extension()`:
- `image/jpeg` -> `jpg`
- `image/png` -> `png`
- `image/gif` -> `gif`
- `image/bmp` -> `bmp`
- `image/webp` -> `webp`
- `image/svg+xml` -> `svg`
- `image/tiff` -> `tiff`
- `image/x-icon` / `image/vnd.microsoft.icon` -> `ico`
- `image/x-emf` / `image/emf` -> `emf`
- `image/x-wmf` / `image/wmf` -> `wmf`

### Output Format

- Raw binary image data extracted as-is (no transcoding or conversion)
- Output filenames: `{base_name}_{n}.{ext}` (multiple images) or `{base_name}.{ext}` (single image)
- EPUB base names derived from metadata: `{Author} - {Title}` (sanitized via `src/common.rs` `sanitize_filename()`)
- DOCX base names derived from the input filename stem
- Collision avoidance: counter suffix appended if file exists (`src/common.rs` `get_unique_output_path()`)

## Data Storage

**Databases:**
- None

**File Storage:**
- Local filesystem only
- Reads: input DOCX/EPUB files (opened read-only)
- Writes: extracted image files to the specified output directory (or current directory)
- Uses `std::fs::create_dir_all` to ensure output directories exist
- Uses `std::io::BufWriter` for efficient file writes (`src/common.rs` `write_image_to_file()`)

**Caching:**
- None. Each run processes files from scratch.

## Authentication & Identity

**Auth Provider:**
- Not applicable. No authentication of any kind.

## Monitoring & Observability

**Error Tracking:**
- None. Errors printed to stderr via `eprintln!` macros.

**Logs:**
- No structured logging framework
- User-facing messages to stdout via `println!`
- Warnings and errors to stderr via `eprintln!`
- Progress indication via `indicatif` progress bars and spinners (stderr)

## CI/CD & Deployment

**Hosting:**
- Not applicable. Distributed as a compiled binary.

**CI Pipeline:**
- No `.github/workflows` or other CI configuration detected
- No automated testing or release pipeline

## Environment Configuration

**Required env vars:**
- None

**Optional env vars:**
- None

**Secrets:**
- None required or used

## Webhooks & Callbacks

**Incoming:**
- None

**Outgoing:**
- None

## Security Considerations

**Archive Path Traversal Protection:**
- `src/common.rs` `is_safe_archive_path()` validates all archive entry paths before extraction
- Rejects: null bytes, `..` path traversal, absolute paths, Windows drive letters, Windows alternate data streams
- Applied in both `src/docx.rs` (ZIP entry names) and `src/epub.rs` (resource paths)

**Filename Sanitization:**
- `src/common.rs` `sanitize_filename()` replaces dangerous characters (`/`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|`, null, control chars) with underscores
- Applied to EPUB metadata-derived filenames before writing to disk

**No Network Surface:**
- Tool makes zero network connections; no attack surface from network-based threats

---

*Integration audit: 2026-04-01*
