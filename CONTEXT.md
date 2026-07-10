# Word Image Extractor

This context names the concepts involved in extracting images from document archives and writing them to disk.

## Language

**Extraction run**:
The project workflow for turning requested input paths into processed documents and a final outcome, including Document selection and per-document extraction.
_Avoid_: CLI orchestration, processing loop

**Document selection**:
The part of an Extraction run that decides which discovered documents are eligible to process and what document-level facts are known before extraction, including EPUB filtering, duplicate handling, display identity, and per-document output placement.
_Avoid_: File collection, scan results, work item builder

**Extraction run intake**:
The project policy for turning parsed user options into one ready-to-run extraction request, including input fallback, image format selection, GIF-only behavior, conversion defaults, EPUB filters, and summary facts needed after the run completes.
_Avoid_: Argument normalization, options builder

**Image write pipeline**:
The project policy for turning buffered archive image sources into files on disk, including Archive image discovery, output naming, conversion outcomes, warning facts, counts, and special GIF routing.
_Avoid_: Save helper, output utility

**Image write policy**:
The valid per-run choices that govern how the Image write pipeline accepts and emits images, including requested Image formats, an optional Conversion policy, and GIF destination.
_Avoid_: Extraction config, writer options

**Image write purpose**:
The role of one source set in the Image write pipeline: normal batch images or a required EPUB cover. It distinguishes cover-specific outcomes from per-run Image write policy.
_Avoid_: Write mode, archive image purpose, extraction kind

**Archive image discovery**:
The acceptance policy within the Image write pipeline for deciding which image-bearing archive resources may be emitted, including archive source safety, Image format identification, fallback warning facts, and requested format filtering.
_Avoid_: Candidate normalization, resource filter

**Image format**:
The project-normalized identity of image bytes used to decide whether an image is extractable, writable, and convertible, independent of how a document labels it.
_Avoid_: Raw extension, MIME type, detected kind

**Conversion policy**:
The valid target encoding requested for extracted images, including only the settings meaningful to that target and whether matching source bytes should be preserved. It excludes GIF routing and batch-versus-cover fallback behavior.
_Avoid_: Conversion flags, conversion options
