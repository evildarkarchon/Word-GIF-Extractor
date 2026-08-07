# AGENTS.md

Guidance for coding agents working in this repository.

## Project

Rust CLI (`word-image-extractor`) that extracts images from `.docx` and `.epub` files by treating them as ZIP archives. Supports format filtering, conversion, and EPUB cover-only extraction. `README.md` has the user-facing flags and output-naming rules.

## Commands

```bash
cargo build --release   # binary at target/release/word-image-extractor.exe
cargo test              # in-crate unit tests + tests/ CLI integration tests
cargo run -- "path/to/document.docx"
```

## Architecture

- `src/lib.rs` owns the whole module tree, including terminal presentation, and exposes exactly four items: `Args`, `run_cli`, `TerminalOutput`, `Capture`. Read its module docs before widening that boundary.
- `src/main.rs` parses arguments and calls `run_cli`; it holds no other logic.
- Unit tests stay in-crate beside their subject (`src/<module>/tests.rs`); `tests/` covers the binary and CLI-visible behavior only.
- Domain vocabulary is defined in `CONTEXT.md` — use those terms and avoid the synonyms it lists. Decisions live in `docs/adr/`; if a change contradicts one, say so explicitly instead of silently overriding it.

## graphify

This project has a knowledge graph at `graphify-out/`.

- For codebase questions, run `graphify query "<question>"` first. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than `GRAPH_REPORT.md` or raw grep output.
- Use `graphify-out/wiki/index.md` for broad navigation instead of raw source browsing; read `GRAPH_REPORT.md` only for broad architecture review or when query/path/explain do not surface enough context.
- Dirty `graphify-out/` files are expected after hooks or incremental updates and are not a reason to skip graphify. Only skip it if the task is about stale or incorrect graph output, or the user says not to.
- After modifying code, run `graphify update .` (AST-only, no API cost).

## Agent policy

- **Issues** live in GitHub Issues on `evildarkarchon/Word-GIF-Extractor`, driven by the `gh` CLI. External PRs are **not** a triage surface. See `docs/agents/issue-tracker.md`.
- **Triage labels**: the five canonical strings are used verbatim (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.
- **Domain docs**: single-context (`CONTEXT.md` and `docs/adr/` at the repo root). See `docs/agents/domain.md`.
