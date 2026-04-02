# Technology Stack

**Analysis Date:** 2026-04-01

## Languages

**Primary:**
- Rust (Edition 2024) - All application code

**Secondary:**
- None

## Runtime

**Environment:**
- Rust 1.94.0 (nightly/stable; edition 2024 requires Rust >= 1.85)
- No `rust-toolchain.toml` present; relies on the system-installed toolchain

**Package Manager:**
- Cargo 1.94.0
- Lockfile: `Cargo.lock` present and committed (lockfile version 4)

## Frameworks

**Core:**
- No web/application framework; this is a standalone CLI binary

**Testing:**
- Built-in `#[cfg(test)]` / `cargo test` with standard Rust test harness
- No external test framework (no `proptest`, `quickcheck`, or `rstest`)

**Build/Dev:**
- Cargo (standard Rust build system)
- No custom build scripts (`build.rs` not present)
- No procedural macros beyond `clap`'s derive macros

## Key Dependencies

**Critical (direct dependencies in `Cargo.toml`):**

| Crate | Version (spec) | Resolved | Purpose |
|-------|----------------|----------|---------|
| `zip` | `2.1.0` | `2.4.2` | Opens DOCX files as ZIP archives for image extraction |
| `clap` | `4.5.4` (features: `derive`) | `4.5.53` | CLI argument parsing with derive macros |
| `anyhow` | `1.0.82` | `1.0.100` | Ergonomic error handling with context chaining |
| `walkdir` | `2.5.0` | `2.5.0` | Recursive directory traversal for batch processing |
| `epub` | `2.1.4` | `2.1.5` | EPUB document parsing, metadata extraction, and resource access |
| `indicatif` | `0.18.3` | `0.18.3` | Terminal progress bars and spinners for user feedback |

**Notable transitive dependencies:**
- `epub` crate internally uses `zip 3.0.0` (different major version from the direct `zip 2.x` dependency)
- `epub` depends on `xml-rs`, `regex`, `percent-encoding`, `thiserror`
- `zip 2.x` pulls in encryption support (`aes`, `hmac`, `sha1`, `pbkdf2`) and compression (`flate2`, `bzip2`, `lzma-rs`, `deflate64`)

## Configuration

**Environment:**
- No environment variables required
- No `.env` files present
- Pure CLI-driven configuration via `clap` arguments

**Build:**
- `Cargo.toml`: Single package, no workspace
- Release profile (`[profile.release]`):
  - `opt-level = 3` - Maximum optimization
  - `lto = true` - Link-time optimization for smaller binary
  - `codegen-units = 1` - Single codegen unit for better optimization
  - `strip = true` - Strip debug symbols from binary
  - `panic = "abort"` - Abort on panic (no unwinding overhead)

**CLI Arguments (defined in `src/main.rs` `Args` struct):**
- `inputs` (positional) - Input file/directory paths
- `-i / --input` - Named input paths (alternative to positional)
- `-o / --output` - Output directory (defaults to `.`)
- `-r / --recursive` - Recursive directory scanning
- `-f / --formats` - Comma-delimited format filter (e.g., `png,jpg`)
- `-c / --cover-only` - Extract only EPUB cover images
- `--cover-fallback` - Fall back to all images if no cover found (requires `-c`)
- `--title` - Filter EPUBs by title substring (case-insensitive)
- `--author` - Filter EPUBs by author substring (case-insensitive)

## Platform Requirements

**Development:**
- Rust toolchain >= 1.85 (required for edition 2024; code uses `let` chains which stabilized in edition 2024)
- Cargo with lockfile v4 support
- No OS-specific build dependencies

**Production:**
- Cross-platform CLI binary (Windows, Linux, macOS)
- No runtime dependencies beyond the OS standard library
- Binary output: `target/release/word-image-extractor.exe` (Windows) / `target/release/word-image-extractor` (Unix)
- No network access required at runtime

## Source File Layout

| File | Lines | Purpose |
|------|-------|---------|
| `src/main.rs` | ~448 | Entry point, CLI parsing, orchestration, progress bars |
| `src/common.rs` | ~230 | Shared utilities: path safety, filename sanitization, image I/O |
| `src/docx.rs` | ~85 | DOCX processing: ZIP traversal and image extraction |
| `src/epub.rs` | ~439 | EPUB processing: metadata, cover detection, image extraction |

---

*Stack analysis: 2026-04-01*
