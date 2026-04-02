---
phase: 5
slug: docx-pipeline-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` with `cargo test` |
| **Config file** | None -- standard Rust test harness |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 05-01-01 | 01 | 1 | CONV-01 | unit | `cargo test encode_webp_lossless` | Wave 0 | pending |
| 05-01-02 | 01 | 1 | CONV-01 | unit | `cargo test` (existing tests updated for lossless param) | Existing -- needs update | pending |
| 05-01-03 | 01 | 1 | CONV-01 | unit | `cargo test extraction_counts` | Wave 0 | pending |
| 05-01-04 | 01 | 1 | CONV-01 | unit | `cargo test docx_convert` | Wave 0 | pending |
| 05-01-05 | 01 | 1 | CONV-01 | unit | `cargo test gif_routing_with_convert` | Wave 0 | pending |
| 05-01-06 | 01 | 1 | CONV-01 | unit | `cargo test conversion_error_continues` | Wave 0 | pending |
| 05-01-07 | 01 | 1 | CONV-01 | unit | `cargo test summary_message` | Wave 0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `src/convert.rs` -- new tests for `encode_webp_lossless()`, `convert_image()` with `lossless: true`, `try_convert()` with `lossless` param
- [ ] `src/convert.rs` -- existing tests updated to pass `lossless: false` parameter
- [ ] `src/common.rs` -- test for `ExtractionCounts` with `converted` and `skipped` fields

*Existing infrastructure covers framework needs; only test stubs needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| End-to-end DOCX extraction with `--convert png` | CONV-01 | Requires real DOCX fixture files not in repo | 1. Create/find a DOCX with mixed images 2. Run `cargo run -- doc.docx --convert png -o out/` 3. Verify PNG outputs and warnings for unsupported formats |
| GIF routing with conversion on real DOCX | CONV-01 | Requires real DOCX with GIF images | 1. Find DOCX with GIF images 2. Run `cargo run -- doc.docx --convert png --gif-output gifs/` 3. Verify GIFs in gifs/ dir, PNGs in default dir |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
