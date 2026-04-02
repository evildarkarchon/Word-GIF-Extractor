# Phase 1: Conversion Module Core - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 01-conversion-module-core
**Areas discussed:** WebP encoding strategy, Conversion API design, Alpha compositing, Error vs skip behavior

---

## WebP Encoding Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| webp crate (lossy) | Smaller output files, quality control. Adds C dependency (libwebp). | |
| image crate (lossless only) | Pure Rust, no C deps. But lossless WebP is often larger than PNG. | |
| Both with flag | Default lossy via webp crate, add --lossless flag for lossless. | Y |

**User's choice:** Both with flag — default lossy, `--lossless` for lossless WebP
**Notes:** Follow-up confirmed `--lossless` applies only to WebP output (not JPEG or PNG).

---

## Conversion API Design

### Return type

| Option | Description | Selected |
|--------|-------------|----------|
| Converted bytes | Returns Vec<u8>. Caller writes to disk. Clean separation. | Y |
| Converted bytes + metadata | Returns (Vec<u8>, new_extension). Caller gets both. | |
| You decide | Claude picks. | |

**User's choice:** Converted bytes only

### Format parameter type

| Option | Description | Selected |
|--------|-------------|----------|
| Enum | OutputFormat::Jpg, OutputFormat::Png, OutputFormat::Webp. Type-safe. | Y |
| String | "jpg", "png", "webp". Simpler, matches CLI input. | |
| You decide | Claude picks. | |

**User's choice:** Enum (OutputFormat)

### can_convert check

| Option | Description | Selected |
|--------|-------------|----------|
| Yes | Expose can_convert(ext) -> bool for pre-check. | Y |
| No | Just try and handle error. | |

**User's choice:** Yes, expose can_convert

---

## Alpha Compositing

### Background color

| Option | Description | Selected |
|--------|-------------|----------|
| White | Industry standard. Natural for document images. | Y |
| User-configurable | --background flag. More flexible but adds scope. | |
| You decide | Claude picks. | |

**User's choice:** White background

### Alpha in PNG/WebP output

| Option | Description | Selected |
|--------|-------------|----------|
| Preserve alpha | Keep transparency intact. No data loss. | Y |
| Always flatten | Composite against white for all formats. Simpler. | |

**User's choice:** Preserve alpha for PNG and WebP

---

## Error vs Skip Behavior

| Option | Description | Selected |
|--------|-------------|----------|
| Ok(None) for unsupported, Err for corrupt | Distinguishes "can't convert" from "broken data". | Y |
| Always Err with typed variants | ConvertError::Unsupported vs DecodeFailed. More precise but heavier. | |
| Always Ok(None) | Any failure returns None. Simplest but no distinction. | |

**User's choice:** Ok(None) for unsupported, Err for corrupt

---

## Claude's Discretion

- Internal module structure and helper organization
- Decode approach (load_from_memory vs ImageReader)
- webp crate integration details
- Unit test strategy and test image generation

## Deferred Ideas

None — discussion stayed within phase scope
