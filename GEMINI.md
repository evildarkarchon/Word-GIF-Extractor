# Word-GIF-Extractor

## Project Overview
This project is a Rust-based Command Line Interface (CLI) tool designed to extract `.gif` images from Microsoft Word (`.docx`) documents. It treats the `.docx` file as a ZIP archive, iterates through its contents, and extracts any files ending in `.gif`.

## Key Features
- **Extraction:** Isolates `.gif` files from the document structure.
- **Renaming:** Renames extracted files based on the document name (e.g., `MyDoc_1.gif`, `MyDoc_2.gif`) to avoid conflicts and maintain context.
- **Flexible Input:** Accepts the input file path as either a positional argument or a named flag (`--input`). Supports both single files and directories.
- **Recursive Scan:** Optionally searches directories recursively for `.docx` files.
- **Standalone:** Compiled as a static binary for easy distribution without runtime dependencies.

## Tech Stack
- **Language:** Rust (Edition 2021)
- **Dependencies:**
    - `clap`: For robust command-line argument parsing.
    - `zip`: For reading and traversing the `.docx` archive structure.
    - `anyhow`: For flexible error handling.
    - `walkdir`: For recursive directory traversal.

## Building and Running

### Prerequisites
- Rust toolchain installed (`cargo`, `rustc`).

### Build Command
To build the release binary (optimized):
```bash
cargo build --release
```
The executable will be located at `target/release/word-gif-extractor.exe`.

### Usage
The tool can be run directly via `cargo` or by executing the compiled binary.

**Syntax:**
```bash
word-gif-extractor.exe [OPTIONS] [INPUT_POS]
```

**Examples:**
1. **Positional Input (File):**
   ```bash
   word-gif-extractor.exe "path/to/document.docx"
   ```

2. **Positional Input (Directory):**
   ```bash
   word-gif-extractor.exe "path/to/documents_folder"
   ```

3. **Recursive Directory Scan:**
   ```bash
   word-gif-extractor.exe "path/to/documents_folder" --recursive
   ```

4. **Named Input Flag:**
   ```bash
   word-gif-extractor.exe --input "path/to/document.docx"
   ```

5. **Specifying Output Directory:**
   ```bash
   word-gif-extractor.exe "path/to/document.docx" --output "path/to/output/folder"
   ```

## Development Conventions
- **Error Handling:** Uses `anyhow::Result` for main function return types to provide clean error output.
- **Argument Parsing:** Uses `clap` with struct derivation (`#[derive(Parser)]`).
- **File Operations:** Standard library `std::fs` and `std::path` are used for file system interactions.