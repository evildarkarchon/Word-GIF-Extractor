# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project Overview

A Rust CLI tool that extracts image files from Microsoft Word (.docx) documents by treating them as ZIP archives. Supports multiple image formats: jpg, jpeg, png, gif, bmp, tiff, svg, wmf, emf, webp, ico.

## Build Commands

```bash
# Build release binary
cargo build --release

# Run directly via cargo
cargo run -- "path/to/document.docx"
cargo run -- --input "path/to/document.docx" --output "output/folder"

# Process a directory of .docx files
cargo run -- "path/to/folder"

# Recursive directory processing
cargo run -- "path/to/folder" -r

# Filter by format (comma-separated)
cargo run -- "document.docx" -f png,gif

# Extract only cover images from EPUB files
cargo run -- "book.epub" -c
cargo run -- "path/to/epubs" -r --cover-only

# Run tests
cargo test
```

The release binary is output to `target/release/word-image-extractor.exe`.

## Architecture

Rust CLI application with document-specific processing modules and straightforward flow:
1. Parse CLI arguments with `clap` (supports positional or `--input` flag)
2. Determine target image extensions (all supported or user-filtered via `-f`)
3. If input is a file: process it directly; if directory: iterate over .docx files (optionally recursive with `-r`)
4. Open each .docx as a ZIP archive, read safe entries, and detect image formats by magic bytes with extension fallback
5. Extract images to the output directory (explicit `-o`, or each source file's directory by default) using the detected format extension, renamed as `{docname}_{n}.{ext}` (or `{docname}.{ext}` if only one)

## Dependencies

- **zip**: Archive traversal for .docx files
- **clap**: CLI argument parsing with derive macros
- **anyhow**: Error handling with context
- **walkdir**: Recursive directory traversal

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
