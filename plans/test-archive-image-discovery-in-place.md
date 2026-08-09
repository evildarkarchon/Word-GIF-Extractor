# Test Archive image discovery where the decision is made

Spec for candidate 2 of the `codex/architecture` review. Settled by grilling; every
numbered decision below was chosen deliberately and the rejected alternative is recorded
so a later reader does not re-open it by accident.

## Subject

`Archive image discovery` (`src/image_write_pipeline/discovery.rs`, 344 lines, no tests) and
`Image write purpose` (`src/image_write_pipeline/purpose.rs`, 209 lines, no tests) are exercised
only through `src/image_write_pipeline/tests.rs` (1296 lines, 30 tests). Both terms are already
defined in `CONTEXT.md` and neither changes meaning here — this is a test-placement change, not a
domain-model change, so `CONTEXT.md` is untouched.

## Why, corrected

The review's stated motive — "roughly half the pipeline tests pay for a temp directory" — is
wrong about cost and understates the real problem.

`test_support::temp_test_dir` (`src/test_support.rs:44`) only *computes* a path; it never creates
the directory. So the ten tests that call it and then assert `!temp_dir.exists()` create nothing
and pay one `stat`. They are not slow.

They are, however, **vacuous**. `assert!(!temp_dir.exists())` at `tests.rs:276, 315, 352, 388, 553,
585, 723, 972, 998, 1036` passes whether the source was rejected, filtered, or failed to acquire —
and passes if the pipeline never ran at all. `ArchiveImageDiscoveryOutcome` distinguishes exactly
what that assertion cannot. The motive for this change is **assertion strength and locality**, not
speed.

The review's "~1296 lines drop to roughly half" does not survive either. The 11 tests that
genuinely need the pipeline are 428 body lines, and ~214 lines of header and helpers mostly stay —
a ~640-line floor before any remainder. Expect `tests.rs` to land near **850–900**, with ~350–450
new lines across two new files. **Net crate-wide test lines are roughly flat.** Anyone reviewing
this for volume reduction is measuring the wrong thing.

## Decisions

1. **Test subject: split by subject, not interface-only.** `format_from_magic`, `format_from_mime`,
   `is_svg` and `is_emf` are module-private *recognizers* with their own documented contracts, and
   they get direct tests. `discover_image` gets tests for what only it does: evidence precedence,
   the 1,027-byte boundary, the extension-fallback warning, purpose delegation, and the two-phase
   read. *Rejected:* interface-only (constructing a source, a purpose and an allow-set to assert
   "BMP is recognized" re-creates the problem at smaller scale); internals-only (freezes the
   recognizer decomposition).

2. **Relocation deletes.** A decision is tested in one place. Where a pipeline test's only on-disk
   assertion is the vacuous `!temp_dir.exists()`, it is deleted, not kept green. Where it asserts
   real bytes at a real filename, one representative case stays and the rest moves down.
   *Rejected:* duplicating below while leaving the original — that is the "same fact spelled twice"
   shape this review exists to remove.

3. **`purpose/tests.rs` covers path safety only.** `is_safe_archive_path` has four rejection rules
   and is a traversal/ADS guard — a missing branch there is a real defect. The ten
   `ImageWritePurpose` methods return unconditional literals; asserting them is a source mirror.
   They are exercised where they are consumed. *Rejected:* full purpose coverage.

4. **Port plus the boundary cases the doc comments already promise.** `format_from_magic` documents
   behaviour for "short, incomplete, or unknown evidence" that nothing currently tests, because at
   the pipeline seam each case cost a fixture and a directory. *Rejected:* port-only (leaves the
   strongest argument for the change unused); exhaustive (walking the 14-arm MIME table asserts a
   lookup table equals itself).

5. **Three derives, no test-only mirror.** `#[derive(Debug, PartialEq)]` on
   `ArchiveImageDiscoveryOutcome` and `DiscoveredImage`; add `PartialEq` to `AcceptedImage`, which
   already derives `Debug`. A derive is not a visibility widening — the types stay `pub(super)`, so
   ADR-0003 is untouched — and `ImageWriteWarning` already derives `Debug, Clone, PartialEq, Eq`
   for exactly this reason. `purpose/tests.rs` needs **zero** derives: under decision 3 its subject
   returns `bool`, and `SourceEligibility` is asserted with `matches!`. *Rejected:* a test-local
   enum mirroring `ArchiveImageDiscoveryOutcome` — adding a fourth outcome would mean editing two
   enums, which is the defect candidate 1 exists to remove.

6. **Three thin pipeline residues survive.** Discovery cannot produce the pipeline's *disposition*.
   Two arms matter enough to pin: `Completed -> RequiredCoverWriteOutcome::Completed` and
   `AcquisitionFailed -> ::Retry`, which is the contract EPUB cover extraction's retry loop is built
   on (ADR-0005). Plus one normal-side residue for `Completed -> counts.extracted == 0`, because
   after this change nothing else asserts it — test 12 proves emission-failure-to-zero, not
   discovery-completed-to-zero.

7. **`magic_format_cases()` moves into `discovery/tests.rs`; the `expected_extension` column dies.**
   `src/image_format/tests.rs:42` (`exposes_the_canonical_extension_for_every_format`) already
   asserts `format.extension()` for all twelve formats, so that column duplicates an existing test.
   *Rejected:* promoting the table to `test_support.rs` (one consumer after this change —
   speculative sharing); duplicating byte literals (including the hand-built EMF row that places
   `" EMF"` at offset 40).

   **Accepted cost, stated plainly.** Today one test proves end-to-end that a WEBP payload lands at
   `sample.webp`. After this change that becomes a composition of three separately-tested facts:
   `format_from_magic` identifies WEBP (discovery), `Webp::extension() == "webp"`
   (`image_format/tests.rs`), and the emitted filename is built from `format.extension()` (one
   representative in `tests.rs`). The composition is sound, but no single test walks it for eleven of
   the twelve formats. This is a deliberate reduction in end-to-end coverage, not a free win.

8. **`temp_test_dir` is documented, not fixed.** Add a doc comment stating it does not create the
   directory and that `!path.exists()` is therefore not evidence of non-emission. The created-dir
   fixture belongs to candidate 6. *Rejected:* leaving the trap undocumented after deleting ten
   instances of it; fixing it here (blurs two specs).

9. **The two required-cover conversion tests stay in `tests.rs`.** `discover_image` calls only three
   of the five purpose hooks — `source_eligibility`, `unidentified_format`, `filtered_format`.
   `unsupported_conversion` and `failed_conversion` are called from `prepare_image_for_write`
   (`image_write_pipeline.rs:609-678`), after discovery has returned `Accepted`. Tests 4 and 5 are
   therefore unreachable from `discovery/tests.rs` and excluded from `purpose/tests.rs` by decision
   3. They keep their bodies minus the vacuous assertion. What makes them worth keeping is the
   *contrast* — a cover completes without emission where a normal image preserves the original — and
   that contrast only exists where both paths run. Consequence: test 5's `"Failed to decode image"`
   remains coupled to a real `ConversionPolicy::convert` run. That is accepted; it is the only place
   proving a real conversion error reaches the warning text.

10. **No ADR.** Of the three criteria, only "result of a real trade-off" holds. It is reversible in
    one commit, and `emission.rs` + `emission/tests.rs` already establishes the pattern so
    `discovery/tests.rs` reads as convention. ADR-0005 already carries the governing argument —
    it refused to write ZIPs to observe a decision that never touches the filesystem, and paid a
    generic for it; this change applies that reasoning to `discover_image`, which needs no generic.
    *Rejected:* amending ADR-0005, whose subject is the cover-extraction generic. Instead, the
    module header of `discovery/tests.rs` explains why the recognizers are tested directly, so the
    next reader does not "fix" it by routing everything through `discover_image`.

## Constraints confirmed against the code

- **No visibility change anywhere.** A `mod tests` inside `discovery` or `purpose` is a *descendant*
  of `image_write_pipeline`, and private items are visible to the defining module and all its
  descendants. That includes `AcceptedImage`'s private `data`/`format` fields. ADR-0003's rule —
  never widen the interface for a test's benefit — is satisfied, not bent.
- **File layout follows existing convention.** `emission.rs` already carries
  `#[cfg(test)] mod tests;` resolving to `emission/tests.rs`. `discovery/` and `purpose/` directories
  are created the same way. Matches AGENTS.md (`src/<module>/tests.rs`).
- **`ImageFormat` derives `Eq`/`Hash`** (it is a `HashSet` key), so the new derives compose.

## Commits

Four, each green, with the deletion isolated because it is where coverage can silently vanish.

### 1. `refactor(pipeline): derive comparison for discovery outcomes`

- `discovery.rs:103` `DiscoveredImage` -> `#[derive(Debug, PartialEq)]`
- `discovery.rs:109` `ArchiveImageDiscoveryOutcome` -> `#[derive(Debug, PartialEq)]`
- `image_write_pipeline.rs:208` `AcceptedImage` -> add `PartialEq` to the existing `Debug`

No behaviour change. Nothing else in the commit.

### 2. `test(discovery): assert format evidence as values`

Create `src/image_write_pipeline/discovery/tests.rs`; add `#[cfg(test)] mod tests;` to the end of
`discovery.rs`. Module header explains decision 1. Move `magic_format_cases()` in, minus
`expected_extension`. Move `FailAfterReader` to `test_support.rs` (see *Open detail* below).

**Recognizer tests, direct:**

| Subject | Cases |
|---|---|
| `format_from_magic` | 12 positives, one per table row |
| `format_from_magic` | empty slice; 1 byte; PNG magic truncated to 7 bytes -> `None` |
| `format_from_magic` | `RIFF` with fewer than 12 bytes; `RIFF` with 12+ bytes whose `[8..12]` is not `WEBP` -> `None` |
| `is_emf` | `\x01\x00\x00\x00` prefix with data shorter than 44 bytes; prefix present but offset 40 is not `" EMF"` -> `false` |
| `is_svg` | `<svgfoo` -> `false` (the delimiter check); `<svg `, `<svg>`, `<svg/`, `<svg` at EOF -> `true`; `<SVG` case-insensitive -> `true` |
| `is_svg` | marker ending at byte 1024 after a 3-byte BOM -> `true`; marker one byte past -> `false` |
| `format_from_mime` | `IMAGE/PNG` (case); `image/svg+xml; charset=utf-8` (parameters); `text/plain` -> `None` |

Representative MIME cases only — the full 14-arm table walk is decision 4's rejected option.

**`discover_image` tests, interface:**

| Fact | Source |
|---|---|
| Magic outranks a conflicting extension and MIME; no `ExtensionFallback` emitted | pure part of test 8 |
| Eligible extension outranks MIME and emits `ExtensionFallback { source_name, format }` | pure part of test 20 |
| MIME is used only after magic and extension fail, and emits no warning | pure part of test 29 |
| Unidentified normal source completes silently, consuming exactly 1027 bytes | test 10 |
| Filtered format consumes exactly 1027 bytes even though identified at byte 0 | test 11 |
| Accepted source retains the prefix and appends the tail: position 4096, `AcceptedImage.data == original` | pure part of test 9 |
| Ineligible source leaves the reader at position 0 | test 22 |
| BOM-prefixed SVG at the end of the window is `Accepted` | pure part of test 14 |
| BOM-prefixed SVG beyond the window is `Completed`, position 1027 | test 15 |
| `RequiredCover` unidentified -> continues as JPEG with `CoverDefaultToJpeg { mime }` | pure part of test 1 |
| `RequiredCover` filtered -> `Completed` with `UnsupportedCoverFormat { format }` | pure part of test 2 |
| `RequiredCover` tail failure -> `AcquisitionFailed`, warnings ordered `[CoverDefaultToJpeg, ArchiveImageAcquisitionFailed]` | pure part of test 3 |

Assert boundaries against the literal `1027`, not `FORMAT_EVIDENCE_LIMIT` — comparing the constant
to itself pins nothing; the literal pins the documented contract.

`tests.rs` is untouched in this commit, so the crate is over-covered and green.

### 3. `test(purpose): assert archive path safety directly`

Create `src/image_write_pipeline/purpose/tests.rs`; add `#[cfg(test)] mod tests;` to `purpose.rs`.

- `is_safe_archive_path` rejects: `..` anywhere, leading `/`, leading `\`, `:` anywhere, `\0` anywhere
- `is_safe_archive_path` accepts: `word/media/image.png`, `OEBPS/images/cover.jpg`
- `is_normal_source_safe` is `false` for a source built with `ArchiveImageSource::required_cover`,
  which carries no path evidence
- `NormalImages::source_eligibility` -> `Reject` for each rejection case (`matches!`)
- `RequiredCover::source_eligibility` -> `Inspect` unconditionally (`matches!`)

### 4. `test(pipeline): thin decision tests to the interface`

The deletion commit. Also carries the decision-8 doc comment on `test_support::temp_test_dir`.

| Disposition | Tests | Count |
|---|---|---|
| Deleted; content now in `discovery/tests.rs` | 10, 15, 22 | 3 |
| Deleted; content in `discovery`/`purpose`, replaced by one normal-side residue asserting a non-emitting outcome increments nothing | 11, 21 | 2 |
| Thinned to the cover disposition arm only (`Completed` / `Retry`); pure content moved down | 2, 3 | 2 |
| Kept, minus the vacuous assertion (decision 9) | 4, 5 | 2 |
| Untouched — drives `ArchiveImageVisitor::unreadable`, not `discover_image` | 23 | 1 |
| Pure part shed; thinned on-disk residue kept | 1, 8, 9, 14, 20, 24, 25, 29 | 8 |
| Twelve fixtures collapse to one (decision 7) | 7 | 1 |
| Untouched | 6, 12, 13, 16, 17, 18, 19, 26, 27, 28, 30 | 11 |

30 in, 26 out (three deleted, two deleted, one added).

Every remaining `assert!(!temp_dir.exists())` is removed. Where a test still needs to prove
non-emission, it asserts the counts and outcome it already has. Note `!output_dir.exists()` in the
GIF-routing tests (6, 28) is a *different* and meaningful assertion — the directory genuinely could
have been created there — and stays.

## Open detail — decided, not grilled

`FailAfterReader` (`tests.rs:102-132`) is used by test 3, which moves to `discovery/tests.rs`, and
test 13, which stays. Two consumers in two modules meets the crate's own "one adapter is a
hypothetical seam, two are a real one" rule (ADR-0005), and a `Read` adapter that fails after N
bytes is generic infrastructure, so it moves to `src/test_support.rs` in commit 2. This is the one
choice made without being put to the reviewer; the alternative is duplicating ~30 lines.

`AssertOutputBeforeTailReader` stays in `tests.rs` — its subject (test 17) is not moving.

## Verification

`cargo test` green after each commit. After commit 4, confirm by inspection that no
`assert!(!<path>.exists())` remains where `<path>` came from `temp_test_dir` without an intervening
`create_dir_all`. Run `graphify update .` at the end.
