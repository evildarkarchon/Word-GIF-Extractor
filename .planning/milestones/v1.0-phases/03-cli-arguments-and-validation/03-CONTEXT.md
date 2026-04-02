# Phase 3: CLI Arguments and Validation - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Add `--convert`, `--gif-only`, `--gif-output`, `--quality`, and `--lossless` flags to the existing clap-based `Args` struct. Implement conflict rules and validation. No changes to DOCX/EPUB processors or conversion logic -- this phase only adds CLI argument definitions and validation.

</domain>

<decisions>
## Implementation Decisions

### Short Flag Assignments
- **D-01:** All new flags get short forms:
  - `-C` / `--convert <jpg|png|webp>` -- target conversion format
  - `-q` / `--quality <1-100>` -- encoding quality override
  - `-g` / `--gif-only` -- extract only GIF files
  - `-G` / `--gif-output <path>` -- separate GIF output directory
  - `-L` / `--lossless` -- use lossless WebP encoding instead of lossy
- **D-02:** Capital letters (`-C`, `-G`, `-L`) differentiate conversion-related flags from existing lowercase flags (`-i`, `-o`, `-r`, `-f`, `-c`).

### --quality Scope
- **D-03:** `--quality` works with both `--convert jpg` and `--convert webp` (both are lossy formats with quality parameters). Updates CONV-07: `--quality` is invalid only with `--convert png` (PNG is lossless).
- **D-04:** Default quality remains 85 for both JPEG and WebP lossy (from Phase 1 D-14/D-15). `--quality` overrides this default.

### Validation Approach
- **D-05:** Use clap's declarative attributes (`conflicts_with`, `requires`, `value_parser`) for all validation that clap can express:
  - `--convert` and `--gif-only` are mutually exclusive (`conflicts_with`)
  - `--quality` requires `--convert` (`requires`)
  - `--lossless` requires `--convert` (`requires`)
- **D-06:** One manual check after parsing: `--quality` with `--convert png` produces an error (clap can't validate against another flag's value). Similarly, `--lossless` with `--convert jpg` or `--convert png` produces an error.
- **D-07:** Consistent with existing pattern -- `--cover-fallback` already uses clap's `requires` attribute.

### --lossless Flag
- **D-08:** `--lossless` (`-L`) added in this phase. Valid only with `--convert webp`. Switches from lossy (default) to lossless WebP encoding via the `image` crate's built-in encoder (Phase 1 D-02).
- **D-09:** `--lossless` and `--quality` are mutually exclusive when used with `--convert webp` (lossless has no quality parameter). Use `conflicts_with` in clap.

### --convert Value Parsing
- **D-10:** Use clap's `ValueEnum` derive on `OutputFormat` to parse `--convert` values directly into the enum. Maps `jpg`/`png`/`webp` strings to `OutputFormat::Jpg`/`Png`/`Webp`.

### Claude's Discretion
- Exact error message wording for the manual `--quality` + `--convert png` check
- Whether to add `ValueEnum` derive directly on the existing `OutputFormat` or create a wrapper
- Test strategy for argument validation (unit tests vs integration tests)
- Argument ordering and grouping in `--help` output

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Context
- `.planning/PROJECT.md` -- Core value, constraints, key decisions
- `.planning/REQUIREMENTS.md` -- CONV-06 (quality override), CONV-07 (quality scope -- updated: jpg and webp), GIF-04 (convert/gif-only conflict)

### Prior Phase Context
- `.planning/phases/01-conversion-module-core/01-CONTEXT.md` -- Phase 1 decisions: OutputFormat enum (D-05), quality defaults (D-14/D-15), --lossless concept (D-02)
- `.planning/phases/02-format-handling-and-output-naming/02-CONTEXT.md` -- Phase 2 decisions: ConversionResult enum (D-05), try_convert API (D-06)

### Source (integration points)
- `src/main.rs` lines 23-60 -- Existing `Args` struct with clap derive, current flag definitions
- `src/convert.rs` lines 16-24 -- `OutputFormat` enum that needs `ValueEnum` derive
- `Cargo.toml` -- May need clap feature flags for `ValueEnum`

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `OutputFormat` enum in `src/convert.rs` -- already has `Jpg`, `Png`, `Webp` variants; add `ValueEnum` derive for clap integration
- `Args` struct in `src/main.rs` -- extend with new fields; follows established patterns

### Established Patterns
- `#[derive(Parser)]` on `Args` struct with `///` doc comments as help text
- Short flags are single letters: `-i`, `-o`, `-r`, `-f`, `-c`
- `#[arg(requires = "...")]` for dependent flags (see `cover_fallback`)
- `#[arg(short, long)]` pattern on all flag definitions
- `Option<T>` for optional arguments, `bool` for boolean flags

### Integration Points
- New fields added to `Args` struct in `src/main.rs`
- `OutputFormat` in `src/convert.rs` gets `ValueEnum` derive
- Manual validation goes in `main()` after `Args::parse()`

</code_context>

<specifics>
## Specific Ideas

- Capital letter short flags for conversion-related features visually group them apart from existing extraction flags
- `--quality` applying to both JPEG and WebP is more consistent -- both are lossy formats with quality parameters, and Phase 1 already set the same default (85) for both
- `--lossless` fitting naturally into this phase avoids a future one-flag micro-phase

</specifics>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 03-cli-arguments-and-validation*
*Context gathered: 2026-04-02*
