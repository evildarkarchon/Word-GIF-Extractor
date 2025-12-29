# Word Image Extractor

A fast CLI tool that extracts images from Microsoft Word (.docx) and EPUB files.

## Features

- Extract images from `.docx` and `.epub` files
- Process single files or entire directories
- Recursive directory scanning with `-r`
- Filter by specific image formats with `-f`
- **EPUB support**: Uses book metadata (author/title) for smart output naming
- Supports: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico

## Installation

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

# Process all .docx and .epub files in a directory
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
| `-i, --input <PATH>`  | Input .docx/.epub file or directory (also accepts positional arg) |
| `-o, --output <DIR>`  | Output directory (defaults to current directory)                  |
| `-r, --recursive`     | Recursively search directories for .docx/.epub files              |
| `-f, --formats <FMT>` | Comma-separated list of formats to extract                        |

## Output Naming

### Word Documents (.docx)
Extracted images are named based on the source document filename:
- Single image: `document.png`
- Multiple images: `document_1.png`, `document_2.jpg`, etc.

### EPUB Files (.epub)
Extracted images use the book's metadata for naming in the format "Author - Title":
- With metadata: `Stephen King - The Shining_1.png`, `Stephen King - The Shining_2.jpg`
- Title only: `The Shining_1.png`
- Author only: `Stephen King_1.png`
- No metadata: Falls back to filename like `.docx` files

Invalid filename characters in metadata are automatically replaced with underscores.

## License

[GPL-3.0 License](https://opensource.org/licenses/GPL-3.0)
