# Word Image Extractor

This context names the concepts involved in extracting images from document archives and writing them to disk.

## Language

**Extraction run**:
The project workflow for turning requested input paths into processed documents and a final outcome, including document discovery, EPUB filtering, duplicate handling, and per-document output placement.
_Avoid_: CLI orchestration, processing loop

**Extraction run intake**:
The project policy for turning parsed user options into one ready-to-run extraction request, including input fallback, image format selection, GIF-only behavior, conversion defaults, EPUB filters, and summary facts needed after the run completes.
_Avoid_: Argument normalization, options builder

**Image write pipeline**:
The project policy for turning extracted image bytes into files on disk, including output naming, conversion outcomes, and special GIF routing.
_Avoid_: Save helper, output utility

**Archive image discovery**:
The project policy for deciding which image-bearing archive resources are accepted for extraction before the Image write pipeline runs, including archive source safety, Image format identification, fallback warning facts, and requested format filtering.
_Avoid_: Candidate normalization, resource filter

**Image format**:
The project-normalized identity of image bytes used to decide whether an image is extractable, writable, and convertible, independent of how a document labels it.
_Avoid_: Raw extension, MIME type, detected kind
