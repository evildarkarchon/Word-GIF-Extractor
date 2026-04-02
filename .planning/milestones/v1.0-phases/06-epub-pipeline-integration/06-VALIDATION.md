---
phase: 6
slug: epub-pipeline-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 6 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Built-in `#[cfg(test)]` / `cargo test` (Rust 1.94) |
| **Config file** | None -- uses standard Cargo test harness |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test && cargo clippy`
- **Before `/gsd:verify-work`:** Full suite must be green + clippy clean
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 06-01-01 | 01 | 1 | ExtractionConfig struct | unit | `cargo test common::tests` | ❌ W0 | ⬜ pending |
| 06-01-02 | 01 | 1 | DOCX refactor (no regression) | compilation | `cargo test` | ✅ existing | ⬜ pending |
| 06-01-03 | 01 | 1 | EPUB all-images conversion | unit | `cargo test epub::tests` | ❌ W0 | ⬜ pending |
| 06-01-04 | 01 | 1 | EPUB cover skip-on-failure | unit | `cargo test epub::tests` | ❌ W0 | ⬜ pending |
| 06-01-05 | 01 | 1 | GIF routing + conversion | unit | `cargo test epub::tests` | ❌ W0 | ⬜ pending |
| 06-01-06 | 01 | 1 | main.rs dispatch (no #[allow]) | compilation | `cargo clippy` | ✅ existing | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Existing docx.rs tests do not call `process_file()` directly -- no changes needed
- [ ] Existing epub.rs tests (`format_epub_base_name`, `mime_to_extension`) don't touch `process_file()` -- no changes needed
- [ ] Unit tests for `ExtractionConfig` construction (verify fields, Debug derive)
- [ ] Verify `cargo clippy` clean without `#[allow(clippy::too_many_arguments)]`
- [ ] EPUB conversion tests: test wiring at the try_convert pattern level (no real EPUB fixtures needed)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| End-to-end EPUB conversion | CONV-01 EPUB extension | Requires real EPUB files with embedded images | Run `cargo run -- book.epub --convert webp` on a real EPUB file |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
