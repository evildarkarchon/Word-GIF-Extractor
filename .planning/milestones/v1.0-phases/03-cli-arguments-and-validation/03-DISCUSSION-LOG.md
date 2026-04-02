# Phase 3: CLI Arguments and Validation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md -- this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 03-cli-arguments-and-validation
**Areas discussed:** Short flag assignments, --quality scope, Validation approach, --lossless flag

---

## Short Flag Assignments

| Option | Description | Selected |
|--------|-------------|----------|
| All four get short flags | -C/--convert, -q/--quality, -g/--gif-only, -G/--gif-output. Maximizes CLI convenience. | ✓ |
| Only common ones | -C/--convert and -q/--quality get short flags. --gif-only and --gif-output long-only. | |
| None -- long-only | All new flags are long-only. Avoids short flag clutter. | |

**User's choice:** All four get short flags
**Notes:** Capital letters for conversion-related flags. Letter assignments confirmed: -C, -q, -g, -G.

### Follow-up: Letter Assignments

| Option | Description | Selected |
|--------|-------------|----------|
| -C, -q, -g, -G are fine | Use these assignments as proposed. | ✓ |
| Different letters | User specifies different short flag letters. | |

**User's choice:** -C, -q, -g, -G are fine

---

## --quality Scope

| Option | Description | Selected |
|--------|-------------|----------|
| JPEG and WebP both | --quality works with --convert jpg and --convert webp. More consistent. | ✓ |
| JPEG only (per CONV-07) | --quality only valid with --convert jpg. WebP always uses default 85. | |
| You decide | Claude picks the approach. | |

**User's choice:** JPEG and WebP both
**Notes:** Updates CONV-07 -- --quality invalid only with --convert png (lossless format).

---

## Validation Approach

| Option | Description | Selected |
|--------|-------------|----------|
| Clap attributes (Recommended) | Use clap's conflicts_with, requires, value_parser. Consistent with existing pattern. | ✓ |
| Manual in main() | Check flag combinations manually after parsing. Full control over messages. | |
| Hybrid | Clap for simple conflicts, manual for conditional rules. | |

**User's choice:** Clap attributes (Recommended)
**Notes:** Acknowledged that --quality with --convert png check requires a small manual validation since clap can't validate against another flag's value.

---

## --lossless Flag

| Option | Description | Selected |
|--------|-------------|----------|
| Add it now | Add --lossless in this phase alongside other flags. Only valid with --convert webp. | ✓ |
| Defer it | Skip for now -- not in REQUIREMENTS.md. | |
| You decide | Claude picks. | |

**User's choice:** Add it now

### Follow-up: --lossless Short Flag

| Option | Description | Selected |
|--------|-------------|----------|
| -L for --lossless | Capital L, consistent with conversion-related flags. | ✓ |
| Long-only | No short flag for --lossless. | |
| You decide | Claude picks. | |

**User's choice:** -L for --lossless

---

## Claude's Discretion

- Exact error message wording for manual validation checks
- Whether to derive ValueEnum directly on OutputFormat or use a wrapper
- Test strategy for argument validation
- Argument ordering and grouping in --help output

## Deferred Ideas

None -- discussion stayed within phase scope
