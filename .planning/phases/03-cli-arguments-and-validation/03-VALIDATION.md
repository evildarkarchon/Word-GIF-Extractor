---
phase: 3
slug: cli-arguments-and-validation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust standard test harness (`#[cfg(test)]` / `cargo test`) |
| **Config file** | None (standard Cargo setup) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~5 seconds |

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
| 03-01-01 | 01 | 1 | CONV-06 | unit | `cargo test test_quality_with_convert_jpg -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-02 | 01 | 1 | CONV-06 | unit | `cargo test test_quality_range_validation -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-03 | 01 | 1 | CONV-07 | unit | `cargo test test_quality_with_png_error -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-04 | 01 | 1 | CONV-07 | unit | `cargo test test_quality_with_convert_webp -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-05 | 01 | 1 | GIF-04 | unit | `cargo test test_convert_and_gif_only_conflict -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-06 | 01 | 1 | — | unit | `cargo test test_convert_parses_all_formats -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-07 | 01 | 1 | — | unit | `cargo test test_quality_requires_convert -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-08 | 01 | 1 | — | unit | `cargo test test_lossless_requires_convert -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-09 | 01 | 1 | — | unit | `cargo test test_lossless_conflicts_with_quality -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-10 | 01 | 1 | — | unit | `cargo test test_lossless_with_jpg_error -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-11 | 01 | 1 | — | unit | `cargo test test_lossless_with_png_error -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-12 | 01 | 1 | — | unit | `cargo test test_short_flags -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-13 | 01 | 1 | — | unit | `cargo test test_gif_output_independent -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-14 | 01 | 1 | — | unit | `cargo test test_existing_flags_unchanged -- --exact` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/main.rs` — add `#[cfg(test)] mod tests { ... }` test module (none exists currently)
- [ ] All test functions listed in verification map above
- [ ] No additional framework or fixture setup needed — `Args::try_parse_from` available from `Parser` derive

*Existing infrastructure covers framework needs. Wave 0 is test stubs only.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `--help` shows all new flags with descriptions | Success Criteria #4 | Help text formatting is best verified visually | Run `cargo run -- --help` and verify --convert, --quality, --gif-only, --gif-output, --lossless appear with descriptions |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
