---
phase: 4
slug: gif-extraction-and-routing
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` / `cargo test` |
| **Config file** | none — standard Cargo test harness |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 04-01-01 | 01 | 1 | GIF-01 | unit | `cargo test gif_only` | ❌ W0 | ⬜ pending |
| 04-01-02 | 01 | 1 | GIF-02 | unit | `cargo test gif_output` | ❌ W0 | ⬜ pending |
| 04-01-03 | 01 | 1 | GIF-03 | unit | `cargo test gif_routing` | ❌ W0 | ⬜ pending |
| 04-01-04 | 01 | 1 | GIF-01,02,03 | integration | `cargo test extraction_counts` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Test stubs for GIF-01 (gif-only filtering)
- [ ] Test stubs for GIF-02 (gif-output routing)
- [ ] Test stubs for GIF-03 (gif-output independent of gif-only)
- [ ] Test stubs for ExtractionCounts struct

*Existing infrastructure covers framework needs — only test stubs needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Split count summary message | GIF-02 | Output formatting to stderr | Run with `--gif-output /tmp/gifs` on a DOCX with mixed images, verify summary shows split counts |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
