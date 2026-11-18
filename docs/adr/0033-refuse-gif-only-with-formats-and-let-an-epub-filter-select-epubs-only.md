# Refuse `--gif-only` with `--formats`, and let an EPUB filter select EPUBs only

The 2026-11-14 audit (L6) found two flag combinations that were accepted and then did something other than what they say.

- `-g -f png` parsed. `--gif-only` replaced the format list with GIF alone, without a word. The run extracted GIFs only and, in the audit's case, wrote nothing.
- `--author nobody ./docs` extracted every DOCX in the folder. `--title` and `--author` filtered EPUBs only, and other documents went straight through.

## Decision

**`--gif-only` and `--formats` conflict at parse time.** Passing both is a `clap` argument-conflict error, exit status 2, before intake runs. ADR-0027 handles an unrecognized format name the same way. That holds whatever the list holds, `-f gif` included. Accepting only a list that agrees with `-g` would make the rule depend on the list's contents. `-f gif` already means what `-g` means, so refusing the pair blocks no run anyone needs. The other option was to intersect the two, so that `-g -f gif,png` extracts GIFs. It was rejected because it still quietly drops part of what was typed. `--gif-only` already conflicts with `--convert` in the same way.

**A non-empty EPUB filter makes only EPUB documents eligible.** ADR-0011's reasoning for `--cover-only` applies unchanged. A title or author filter asks for particular books. An Office or OpenDocument file can never match one, so it is not work the run was asked to do. Everything ADR-0011 decided about the skip itself carries over:
- Document selection decides it.
- It runs between discovery and EPUB filtering, so discovery still counts what it found.
- A requested non-EPUB input gets one `SkippedNonEpubInput` diagnostic, worded "this run extracts EPUB documents only".
- A non-EPUB found by a directory search is dropped without a line, the same way a filtered-out EPUB is.
- A run left with nothing eligible ends `NoDocuments` and exits 0.

Selection derives this restriction itself, from the `EpubFilter` it is already handed. It does not have intake or the run fold it into `epub_only`. ADR-0011 has selection told *that* eligibility is restricted and never *why*, because the cover reason belongs to Document extraction. Filtering belongs to selection, so here no outside reason needs hiding. Folding the filter into `epub_only` would spread one selection rule across two modules. `DocumentSelectionOptions::epub_only` now means a restriction for a reason outside selection.

The other option was to keep the behaviour and document it in the README and `--help`. It was rejected because it documents a run that serves no one. Nobody filters by author in order to extract their spreadsheets.

## Consequences

This amends ADR-0011, which is among the ADRs up for re-evaluation. Its rule now covers a second reason, and its decision about where and how the skip happens is kept. A run that combined a filter with Office inputs extracts less than it did before, and each named Office input is reported. The README's options table gains `--title`, `--author` and `--gif-only` rows, and its *EPUB Filters* section states the rule. The `--help` text for both filters says that non-EPUB inputs are skipped. The glossary's Document selection entry names both restrictions. Nothing here becomes public, and ADR-0003 still governs the library interface.
