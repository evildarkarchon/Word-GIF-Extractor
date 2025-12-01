# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Rust CLI tool that extracts GIF files from Microsoft Word (.docx) documents by treating them as ZIP archives.

## Build Commands

```bash
# Build release binary
cargo build --release

# Run directly via cargo
cargo run -- "path/to/document.docx"
cargo run -- --input "path/to/document.docx" --output "output/folder"

# Run tests (if any)
cargo test
```

The release binary is output to `target/release/word-gif-extractor.exe`.

## Architecture

Single-file application (`src/main.rs`) with straightforward flow:
1. Parse CLI arguments with `clap` (supports positional or `--input` flag)
2. Open .docx file as a ZIP archive
3. Scan for files ending in `.gif`
4. Extract GIFs to output directory, renamed as `{docname}_{n}.gif`

## Dependencies

- **zip**: Archive traversal for .docx files
- **clap**: CLI argument parsing with derive macros
- **anyhow**: Error handling with context
