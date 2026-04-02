---
phase: 1
slug: conversion-module-core
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` / `cargo test` |
| **Config file** | none — standard Cargo test harness |
| **Quick run command** | `cargo test --lib convert` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib convert`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01 | 1 | CONV-02 | unit | `cargo test convert::tests::test_alpha_compositing` | ❌ W0 | ⬜ pending |
| 01-01-02 | 01 | 1 | CONV-05 | unit | `cargo test convert::tests::test_jpeg_quality_85` | ❌ W0 | ⬜ pending |
| 01-01-03 | 01 | 1 | CONV-02 | unit | `cargo test convert::tests::test_format_conversions` | ❌ W0 | ⬜ pending |
| 01-01-04 | 01 | 1 | CONV-05 | unit | `cargo test convert::tests::test_webp_lossy` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/convert.rs` — module with `#[cfg(test)] mod tests` block
- [ ] Test helper functions for generating test images programmatically (1x1 or small pixel images)
- [ ] `Cargo.toml` updated with `image` and `webp` crate dependencies

*Existing `cargo test` infrastructure covers the test harness — no new framework needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| WebP lossy output is visually acceptable | CONV-05 | Visual quality assessment | Convert a photographic image to WebP at quality 85, compare file size with lossless |

*Most behaviors have automated verification via unit tests.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
