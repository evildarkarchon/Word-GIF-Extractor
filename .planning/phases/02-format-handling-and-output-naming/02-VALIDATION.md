---
phase: 2
slug: format-handling-and-output-naming
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` with standard test harness |
| **Config file** | None -- uses `cargo test` defaults |
| **Quick run command** | `cargo test convert::tests` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test convert::tests`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 02-01-01 | 01 | 1 | CONV-03 | unit | `cargo test convert::tests::test_try_convert_unsupported_extension` | ❌ W0 | ⬜ pending |
| 02-01-02 | 01 | 1 | CONV-03 | unit | `cargo test convert::tests::test_try_convert_unsupported_at_decode` | ❌ W0 | ⬜ pending |
| 02-01-03 | 01 | 1 | CONV-04 | unit | `cargo test convert::tests::test_try_convert_correct_extension` | ❌ W0 | ⬜ pending |
| 02-01-04 | 01 | 1 | CONV-04 | unit | `cargo test convert::tests::test_output_format_extension` | ❌ W0 | ⬜ pending |
| 02-01-05 | 01 | 1 | -- | unit | `cargo test convert::tests::test_try_convert_supported` | ❌ W0 | ⬜ pending |
| 02-01-06 | 01 | 1 | -- | unit | `cargo test convert::tests::test_try_convert_corrupt_data` | ❌ W0 | ⬜ pending |
| 02-01-07 | 01 | 1 | -- | unit | `cargo test convert::tests::test_try_convert_case_normalization` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] All 7 test functions listed above -- added as part of Phase 2 implementation tasks
- [ ] No new test fixtures needed -- existing helpers (`create_test_rgba_png`, `create_test_rgb_jpeg`, etc.) in `convert::tests` provide test data
- [ ] No framework install needed -- standard Rust test harness already configured

*Existing infrastructure covers framework requirements. New tests are the only Wave 0 deliverable.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
