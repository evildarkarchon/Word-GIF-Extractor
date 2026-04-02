# Phase 4: GIF Extraction and Routing - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md -- this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 04-gif-extraction-and-routing
**Areas discussed:** GIF routing + conversion, Summary reporting, Processor API shape

---

## GIF Routing + Conversion

### Q1: When --gif-output and --convert are both used, what happens to GIF files?

| Option | Description | Selected |
|--------|-------------|----------|
| GIFs stay as-is (Recommended) | GIFs are routed to the GIF directory unconverted. Non-GIF images are converted to the target format in the default output dir. Clean separation of concerns. | ✓ |
| GIFs get converted too | GIFs are converted to the target format AND written to the GIF output directory. The GIF dir would contain non-GIF files. | |
| GIFs go to both dirs | Original GIF goes to --gif-output, converted copy goes to default output dir. User gets both versions. | |

**User's choice:** GIFs stay as-is (Recommended)
**Notes:** Confirms the STATE.md research gap tentative answer. Aligns with PROJECT.md principle that GIF separation is a routing concern, not conversion.

### Q2: When --gif-only is used (without --gif-output), where do GIFs go?

| Option | Description | Selected |
|--------|-------------|----------|
| Default output dir (Recommended) | GIFs go to -o/--output (or current dir). --gif-only is just a filter, not a routing flag. | ✓ |
| Require --gif-output | Make --gif-only require --gif-output so there's always an explicit GIF destination. | |

**User's choice:** Default output dir (Recommended)
**Notes:** Establishes that --gif-only and --gif-output are orthogonal flags.

---

## Summary Reporting

### Q3: How should the finish message report GIF routing?

| Option | Description | Selected |
|--------|-------------|----------|
| Split counts (Recommended) | Show separate counts: "Extracted 9 image(s), routed 3 GIF(s) to /path/to/gifs from 4 document(s)". | |
| Total only | Keep existing single count. Simpler but user doesn't know GIF routing breakdown. | |
| Only when routed | Show split counts only when --gif-output is used. Normal total otherwise. | ✓ |

**User's choice:** Only when routed
**Notes:** Split counts are contextual -- only shown when --gif-output is active and routing actually matters. When --gif-only alone, all images are GIFs so total count suffices.

### Q4: Should the per-file progress bar reflect GIF routing?

| Option | Description | Selected |
|--------|-------------|----------|
| Final summary only (Recommended) | Per-file progress bar stays as-is. GIF routing info only in finish message. | ✓ |
| Per-file GIF count | Progress bar shows GIF routing count during extraction. More detailed but cluttered. | |

**User's choice:** Final summary only (Recommended)
**Notes:** Keeps progress bar clean; routing details belong in the summary.

---

## Processor API Shape

### Q5: How should GIF routing be threaded through the processor functions?

| Option | Description | Selected |
|--------|-------------|----------|
| Extra params (Recommended) | Add gif_only: bool and gif_output: Option<&Path> to process_file signatures. Minimal change. | ✓ |
| Config struct | Bundle all extraction options into ExtractionConfig struct. Cleaner but bigger refactor. | |
| Route in main.rs | Processors return extracted images; main.rs handles routing. Biggest refactor. | |

**User's choice:** Extra params (Recommended)
**Notes:** Follows existing pattern. Config struct deferred (STATE.md notes EPUB parameter growth concern for Phase 6).

### Q6: Should process_file return split counts or single total?

| Option | Description | Selected |
|--------|-------------|----------|
| Split counts (Recommended) | Return struct or tuple (total_extracted, gifs_routed) for summary message. | ✓ |
| Single total | Keep Result<usize>. Main.rs tracks GIF counts separately. | |

**User's choice:** Split counts (Recommended)

### Q7: Tuple or struct for the split counts return type?

| Option | Description | Selected |
|--------|-------------|----------|
| Tuple (usize, usize) | Simpler, lighter. Less self-documenting at call sites. | |
| ExtractionCounts struct (Recommended) | Named fields, readable, extensible for Phase 5/6 conversion counts. | ✓ |

**User's choice:** ExtractionCounts struct (Recommended)
**Notes:** Named fields improve readability and allow adding conversion counts in later phases without signature changes.

---

## Claude's Discretion

- Where --gif-only filtering is applied (main.rs vs inside processors)
- Directory creation timing for --gif-output
- ExtractionCounts trait derives
- Test strategy

## Deferred Ideas

None -- discussion stayed within phase scope
