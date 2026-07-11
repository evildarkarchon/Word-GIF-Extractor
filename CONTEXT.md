# Word Image Extractor

This context names the concepts involved in extracting images from document archives and writing them to disk.

## Language

**Extraction run**:
The project workflow for turning requested input paths into processed documents and a final outcome, including Document selection and per-document extraction.
_Avoid_: CLI orchestration, processing loop

**Document selection**:
The part of an Extraction run that decides which discovered documents are eligible to process and what document-level facts are known before extraction, including EPUB filtering, duplicate handling, display identity, and per-document output placement.
_Avoid_: File collection, scan results, work item builder

**Document selection progress**:
The live, user-observable status of Document selection while it scans inputs, filters EPUBs, and removes duplicates. It excludes per-document extraction status and terminal presentation details.
_Avoid_: Run events, selection UI events, progress callbacks

**Document selection diagnostic**:
A non-fatal fact explaining why Document selection skipped an input or could not use document metadata. It excludes per-document extraction warnings and terminal wording.
_Avoid_: Warning string, selection error, progress message

**Extraction run intake**:
The project policy for turning parsed user options into one ready-to-run extraction request, including input fallback, image format selection, GIF-only behavior, conversion defaults, EPUB filters, and summary facts needed after the run completes.
_Avoid_: Argument normalization, options builder

**Image write pipeline**:
The project policy for turning buffered archive image sources into files on disk, including Archive image discovery, output naming, conversion outcomes, warning facts, counts, and special GIF routing.
_Avoid_: Save helper, output utility

**Image file emission**:
The per-image responsibility within the Image write pipeline for claiming a collision-free output name and completing one file without overwriting existing output. It excludes Archive image discovery, conversion, and extraction counts.
_Avoid_: Atomic image emission, output writer

**Image write policy**:
The valid per-run choices that govern how the Image write pipeline accepts and emits images, including requested Image formats, an optional Conversion policy, and GIF destination.
_Avoid_: Extraction config, writer options

**Image write purpose**:
The role of one source set in the Image write pipeline: normal batch images or a required EPUB cover. It distinguishes cover-specific outcomes from per-run Image write policy.
_Avoid_: Write mode, archive image purpose, extraction kind

**EPUB cover extraction**:
The EPUB-only responsibility for turning ordered cover candidates into one required-cover outcome, including acquisition fallback, cover-specific Image format and Conversion policy, and optional fallback to normal images. It excludes archive resource reading mechanics and Image file emission.
_Avoid_: Cover pipeline, cover selection, cover helper

**Archive image discovery**:
The per-resource policy within the Image write pipeline for acquiring archive sources and deciding which may be emitted, including source safety, Image format identification, requested format filtering, and non-fatal acquisition or fallback warning facts.
_Avoid_: Candidate normalization, resource filter

**Image format**:
The project-normalized identity of image bytes used to decide whether an image is extractable, writable, and convertible, independent of how a document labels it.
_Avoid_: Raw extension, MIME type, detected kind

**Conversion policy**:
The valid target encoding requested for extracted images, including only the settings meaningful to that target and whether matching source bytes should be preserved. It excludes GIF routing and batch-versus-cover fallback behavior.
_Avoid_: Conversion flags, conversion options
