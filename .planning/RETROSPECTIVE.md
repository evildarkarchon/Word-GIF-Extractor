# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — Conversion & GIF Features

**Shipped:** 2026-04-02
**Phases:** 6 | **Plans:** 8 | **Tasks:** 14

### What Was Built
- Image format conversion module (convert.rs) with JPEG/PNG/WebP encoding, alpha compositing on white, and quality controls
- 5 CLI flags (--convert, --quality, --lossless, --gif-only, --gif-output) with declarative validation and mutual exclusion rules
- GIF routing and extraction across both DOCX and EPUB processors with separate output directories
- End-to-end conversion pipeline for DOCX and EPUB files including cover-only mode with skip-on-failure
- ExtractionConfig struct consolidating 4 conversion parameters, eliminating clippy::too_many_arguments
- 77 unit tests covering conversion, CLI validation, data structures, and copy semantics

### What Worked
- Bottom-up phase ordering: building convert.rs first (highest risk) provided a stable foundation for all later phases
- Two-pass integration (DOCX then EPUB) proved the pattern before applying to the more complex processor
- ExtractionConfig refactoring in Phase 6 kept parameter count manageable as the API grew
- Detailed plan interfaces (exact function signatures, line numbers) reduced executor guesswork

### What Was Inefficient
- ROADMAP.md progress table fell out of sync with disk state (Phases 2, 3, 5 showed "Not started" despite being complete)
- Some SUMMARY.md files lacked `requirements_completed` frontmatter, making 3-source cross-reference incomplete
- Phase 1 SUMMARY.md had no `one_liner` in frontmatter, reducing accomplishment extraction quality

### Patterns Established
- ExtractionConfig pattern for threading conversion parameters through dispatch chain
- Consistent error handling: all-images mode writes raw on failure, cover-only mode skips entirely
- GIF routing priority: routed GIFs bypass conversion regardless of --convert flag
- Two-branch cover detection (metadata + filename fallback) both get identical conversion treatment

### Key Lessons
1. Plan interfaces with exact current signatures and line numbers dramatically improve executor accuracy
2. The DOCX-first integration strategy (Phase 5) proves patterns that Phase 6 can follow directly
3. Parameter count growth signals the need for config structs -- catching this at Phase 6 was right on time

### Cost Observations
- Model mix: 100% opus (executor, verifier, integration-checker agents)
- Notable: Plans with detailed interfaces and exact code references completed in 4-5 minutes each

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Phases | Plans | Key Change |
|-----------|--------|-------|------------|
| v1.0 | 6 | 8 | First milestone -- established phase ordering, plan interface patterns, wave-based execution |

### Cumulative Quality

| Milestone | Tests | Key Metric |
|-----------|-------|------------|
| v1.0 | 77 | 48/48 must-haves verified, 11/11 requirements satisfied, 5/5 E2E flows complete |

### Top Lessons (Verified Across Milestones)

1. Bottom-up phase ordering (foundational modules first) reduces integration surprises
2. Detailed plan interfaces with exact signatures eliminate executor ambiguity
