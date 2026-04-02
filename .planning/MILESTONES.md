# Milestones

## v1.0 Conversion & GIF Features (Shipped: 2026-04-02)

**Phases completed:** 6 phases, 8 plans, 14 tasks

**Key accomplishments:**

- ConversionResult enum, OutputFormat::extension(), and try_convert() wrapping can_convert + convert_image into a single-call API for Phases 5-6 integration
- Five new CLI flags (--convert, --quality, --lossless, --gif-only, --gif-output) with declarative and manual validation, ValueEnum-derived OutputFormat parsing, and 18 unit tests
- Lossless WebP encoding via image crate WebPEncoder::new_lossless, convert_image/try_convert expanded to 5-parameter signatures, ExtractionCounts gains converted/skipped fields
- End-to-end DOCX conversion via try_convert() in extraction loop with GIF routing priority, conversion-aware finish messages, and 6 new unit tests

---
