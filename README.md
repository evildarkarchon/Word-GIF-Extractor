# Word Image Extractor

A fast CLI tool that extracts images from Microsoft Word (.docx) documents.

## Features

- Extract images from `.docx` files (treats them as ZIP archives)
- Process single files or entire directories
- Recursive directory scanning with `-r`
- Filter by specific image formats with `-f`
- Supports: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/word-image-extractor.exe`.

## Usage

```bash
# Extract all images from a document
word-image-extractor document.docx

# Specify output directory
word-image-extractor document.docx -o ./images

# Process all .docx files in a directory
word-image-extractor ./documents

# Recursive directory processing
word-image-extractor ./documents -r

# Extract only specific formats
word-image-extractor document.docx -f png,gif,jpg
```

### Options

| Option | Description |
|--------|-------------|
| `-i, --input <PATH>` | Input .docx file or directory (also accepts positional arg) |
| `-o, --output <DIR>` | Output directory (defaults to current directory) |
| `-r, --recursive` | Recursively search directories for .docx files |
| `-f, --formats <FMT>` | Comma-separated list of formats to extract |

## Output Naming

Extracted images are named based on the source document:
- Single image: `document.png`
- Multiple images: `document_1.png`, `document_2.jpg`, etc.

## License

[GPL-3.0 License](https://opensource.org/licenses/GPL-3.0)
