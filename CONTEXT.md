# Word Image Extractor

This context names the concepts involved in extracting images from document archives and writing them to disk.

## Language

**Extraction run**:
The project workflow for turning requested input paths into processed documents and a final outcome, including document discovery, EPUB filtering, duplicate handling, and per-document output placement.
_Avoid_: CLI orchestration, processing loop

**Image write pipeline**:
The project policy for turning extracted image bytes into files on disk, including output naming, conversion outcomes, and special GIF routing.
_Avoid_: Save helper, output utility
