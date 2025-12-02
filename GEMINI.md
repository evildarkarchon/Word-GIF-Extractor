# Word-GIF-Extractor (word-image-extractor)

A Rust CLI utility designed to extract image files (including GIFs) directly from Microsoft Word (`.docx`) documents. It operates by treating the `.docx` file as a ZIP archive, allowing for efficient extraction without needing Microsoft Office installed.

## Project Overview

*   **Language:** Rust
*   **Main File:** `src/main.rs`
*   **Key Dependencies:**
    *   `zip`: For reading the internal structure of `.docx` files.
    *   `clap`: For command-line argument parsing.
    *   `walkdir`: For recursive directory traversal.
    *   `anyhow`: For robust error handling.

## Getting Started

### Prerequisites

*   **Rust Toolchain:** Ensure you have Rust and Cargo installed.

### Building the Project

To build the project for release (optimized binary):

```bash
cargo build --release
```

The resulting binary will be located at `target/release/word-image-extractor.exe`.

### Running the Application

You can run the tool directly using `cargo run`:

**Basic Usage (Single File):**
```bash
cargo run -- "path/to/document.docx"
```

**Specify Output Directory:**
```bash
cargo run -- "document.docx" --output "extracted_images/"
```

**Process a Directory of Files:**
```bash
cargo run -- "path/to/folder"
```

**Recursive Directory Scan:**
```bash
cargo run -- "path/to/folder" --recursive
```

**Filter by Image Format:**
Only extract specific formats (e.g., only PNGs and GIFs):
```bash
cargo run -- "document.docx" --formats png,gif
```

### Testing

Run the standard Rust test suite:

```bash
cargo test
```

## Development Conventions

*   **Argument Parsing:** The project uses `clap` with the `derive` feature. All CLI arguments are defined in the `Args` struct in `src/main.rs`.
*   **Error Handling:** `anyhow` is used for error propagation. Functions that interact with the filesystem or ZIP archives return `Result<()>`.
*   **File Processing:** The core logic resides in `process_file`, which handles opening the ZIP, iterating through entries, checking extensions against a whitelist, and writing the output.
*   **Extensions:** Supported extensions are defined in `get_supported_extensions()` and include: `jpg`, `jpeg`, `png`, `gif`, `bmp`, `tiff`, `tif`, `svg`, `wmf`, `emf`, `webp`, `ico`.
