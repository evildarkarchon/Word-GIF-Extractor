# Word Image Extractor

A fast CLI tool that extracts images from Microsoft Office, OpenDocument and EPUB files.

## Features

- Extract images from Word, PowerPoint, Excel and Visio files, OpenDocument files, and EPUBs (see [Supported Documents](#supported-documents))
- Process single files or entire directories
- Recursive directory scanning with `-r`
- Filter by specific image formats with `-f`
- **EPUB support**: Uses book metadata (author/title) for smart output naming
- Supports: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico

## Installation

Building needs Rust 1.88 or newer.

```bash
cargo build --release
```

The binary will be at `target/release/word-image-extractor.exe`.

## Usage

```bash
# Extract all images from a Word document
word-image-extractor document.docx

# Extract all images from an EPUB file
word-image-extractor book.epub

# Specify output directory
word-image-extractor document.docx -o ./images
word-image-extractor book.epub -o ./images

# Extract the images from a presentation, a spreadsheet or an OpenDocument file
word-image-extractor slides.pptx
word-image-extractor budget.xlsx
word-image-extractor notes.odt

# Process every supported document in a directory
word-image-extractor ./documents

# Recursive directory processing
word-image-extractor ./documents -r

# Extract only specific formats
word-image-extractor document.docx -f png,gif,jpg
word-image-extractor book.epub -f png,jpg
```

### Options

| Option                | Description                                                       |
| --------------------- | ----------------------------------------------------------------- |
| `-i, --input <PATH>`  | Input document or directory (also accepts positional arg)         |
| `-o, --output <DIR>`  | Output directory (defaults to each input file's directory)      |
| `-r, --recursive`     | Recursively search directories for supported documents            |
| `-f, --formats <FMT>` | Comma-separated list of formats to extract: `jpg` (or `jpeg`), `png`, `gif`, `bmp`, `tiff` (or `tif`), `svg`, `wmf`, `emf`, `webp`, `ico`. An unrecognized name is an error and nothing runs |
| `--cover-only`        | Extract EPUB cover images only; non-EPUB inputs are skipped       |
| `--cover-fallback`    | With `--cover-only`, extract an EPUB's images when no cover is written, including when `--formats` filters it out |
| `--keep-duplicates`   | Extract every EPUB, even when several declare the same book       |
| `--max-image-size <SIZE>` | Skip, with a warning, any image larger than `SIZE` (default `256M`). See [Image Size Limit](#image-size-limit) |

## Supported Documents

| Kind | Extensions | Images are read from |
| ---- | ---------- | -------------------- |
| Word | `.docx`, `.docm`, `.dotx`, `.dotm` | `word/media/` |
| PowerPoint | `.pptx`, `.pptm`, `.potx`, `.potm`, `.ppsx`, `.ppsm` | `ppt/media/` |
| Excel | `.xlsx`, `.xlsm`, `.xltx`, `.xltm`, `.xlsb` | `xl/media/` |
| Visio | `.vsdx`, `.vsdm`, `.vstx`, `.vstm`, `.vssx`, `.vssm` | `visio/media/` |
| OpenDocument | `.odt`, `.ott`, `.odp`, `.otp`, `.ods`, `.ots`, `.odg`, `.otg` | `Pictures/` |
| EPUB | `.epub` | the resources the book declares |

Extensions are matched without regard to case. Office and OpenDocument files are ZIP archives, and only the images in the media folder above are extracted. Everything else in the archive is left alone, notably the preview thumbnail a file manager shows (`docProps/thumbnail.*` or `Thumbnails/thumbnail.png`), which is not one of the document's images. A video or sound clip in a presentation's media folder is not an image either and is passed over.

The older binary formats (`.doc`, `.ppt`, `.xls`) and flat OpenDocument XML files (`.fodt` and the like) are not ZIP archives and are not supported.

## EPUB Covers

`--cover-only` extracts one cover image per EPUB. It applies to EPUB files alone: Office and OpenDocument inputs are not eligible for a cover run and are skipped before extraction begins. One you name on the command line is reported as skipped; one found while searching a directory is dropped without a message, the same way an EPUB that fails a title or author filter is.

A cover is looked for in two places, in this order: the cover the EPUB declares, then a JPEG file named `cover` (such as `cover.jpg`). The first one written is the cover, and the search stops there. A candidate that writes nothing, because its format is filtered out by `--formats` or because it cannot be read, is passed over with a warning and the next one is tried.

`--cover-fallback` applies per EPUB, not per run. An EPUB with no usable cover falls back to extracting its images; it does not bring Office or OpenDocument files back into a cover run. A usable cover is a cover file that was written, so a cover filtered out by `--formats` counts as no usable cover, and the fallback writes the EPUB's images that pass the filter. The fallback leaves out every cover already tried, so a passed-over cover is not written again as a normal image. Without `--cover-fallback`, an EPUB with no usable cover writes nothing, and its warnings say why.

## Duplicate EPUBs

When several EPUBs declare the same book, only the first is extracted, and each one skipped is reported with a warning naming it and the EPUB it duplicates. Two EPUBs are the same book when they declare the same package identifier (the `<dc:identifier>` the package names as unique), creator and title, ignoring case and surrounding spaces. An EPUB with no identifier is compared on its creator and title only when it declares both. An EPUB that declares neither, or that cannot be read, is never treated as a duplicate, so two files are never merged because they share a file name.

The first is the first one found. Inputs named on the command line are taken in the order given, and a searched directory is read in file-name order, ignoring case, on every operating system. The same order decides which of two documents that write to the same output name gets the unnumbered names.

The issues of a periodical share a creator and title but not an identifier, so each issue is extracted. Pass `--keep-duplicates` to extract every EPUB regardless. Skipped duplicates do not change the exit status.

## Conversion

Use `--convert jpg`, `--convert png`, or `--convert webp` to encode extracted images in a target format. JPEG and lossy WebP default to quality 85; `--quality <1-100>` requests a specific quality, and `--lossless` requests lossless WebP.

When an image already matches the conversion target, its original bytes are preserved unless an encoding setting was explicit. JPEG and WebP are re-encoded when `--quality` is supplied, WebP is re-encoded when `--lossless` is supplied, and matching PNG files are always preserved.

An image that cannot be converted, because its format is not supported for conversion (such as SVG) or its data cannot be decoded, is kept in its original format, with a warning naming the image. The same applies to an EPUB cover with `--cover-only`: a cover that cannot be converted is kept in its original format, still counts as the cover, and is counted among the summary's skipped conversions.

## Image Size Limit

Every image is held to a size limit, 256 MiB by default, so one malformed archive
entry cannot make a run write or hold an arbitrarily large file. An image larger than
the limit is skipped with a warning naming it, and the document's other images are
still extracted and numbered as if it were not there; an EPUB cover larger than the
limit counts as no cover, so the next candidate is tried. `--max-image-size` sets the
limit as a byte count or with a `K`, `M` or `G` suffix in binary units (`1M` is
1,048,576 bytes; `KiB`, `MiB` and `GiB` work too). Zero and decimal units such as `MB`
are rejected.

The limit is checked against the bytes actually read, not the size the archive
declares. An image written in its original bytes is copied to its file as it is read,
so it takes almost no memory however large it is, apart from a document's first
image, which is held until the next one shows whether to number it. An image being
converted is read whole first, because it has to be decoded.

## Output Naming

### Office and OpenDocument Files
Extracted images are named based on the source document filename:
- Single image: `document.png`
- Multiple images: `document_1.png`, `document_2.jpg`, etc.

### EPUB Files (.epub)
Extracted images use the book's metadata for naming in the format "Author - Title":
- With metadata: `Stephen King - The Shining_1.png`, `Stephen King - The Shining_2.jpg`
- Title only: `The Shining_1.png`
- Author only: `Stephen King_1.png`
- No metadata: Falls back to filename like Office files

Invalid filename characters in metadata are automatically replaced with underscores.

For both document kinds, the number is the image's position in the document, counted
across every image it emits. GIFs routed to a separate directory with `--gif-output`
keep their place in that count, so a document with one PNG and one GIF writes
`document_1.png` in the output directory and `document_2.gif` in the GIF directory.

## Output

A run ends with one summary line on standard output, such as
`Extracted 3 image(s) from 2 document(s)`, `No images found` or
`No documents found to process.`. The summary is printed whether or not the output
goes to a terminal, so a script or a redirect can read it from the last line of
standard output. Some earlier steps also print a line there, ahead of the summary:
`--recursive` prints how many documents it found, `--title` or `--author` prints how
many EPUBs matched, and skipping duplicate EPUBs prints how many were removed.
Warnings and errors go to standard error. A named path that does not exist, or that
is not a [supported document](#supported-documents) or a directory, is reported as an `Error:` line;
an unsupported file found inside a directory is passed over silently.

The progress bar is drawn on standard error and only when that is a terminal. It is
cleared before each line above is printed, so it never stays on screen under the
result.

## Exit Status

- `0`: the run completed, no document failed and every named input was usable.
  This includes runs that found no documents or no images, such as a named
  directory holding none, and runs whose only problems were printed as warnings.
- `1`: at least one document failed to extract (printed as an
  `Error processing ...` line), at least one named input does not exist or is not
  a supported document or a directory (printed as an `Error:` line), or the
  options were valid to parse but could not be combined, such as `--quality` with
  `--convert png`. A failed document or rejected input does not stop the run: the
  other inputs are still processed and the summary is still printed.
- `2`: the command line could not be parsed.

## License

[GPL-3.0 License](https://opensource.org/licenses/GPL-3.0)
