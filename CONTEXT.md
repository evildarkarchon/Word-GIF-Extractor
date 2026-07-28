# Word Image Extractor

This context names the concepts involved in extracting images from document archives and writing them to disk.

## Language

**Extraction run**:
The project workflow for turning requested input paths into processed documents and a final outcome, including Document selection and per-document extraction.
_Avoid_: CLI orchestration, processing loop

**Extraction run request**:
The ready-to-execute handoff produced by Extraction run intake, containing the requested inputs, workflow policies, and facts needed to classify the eventual Extraction run outcome. The Extraction run consumes it exactly once; it excludes pre-run intake notices and terminal wording.
_Avoid_: Run options, prepared options, configuration bundle

**Extraction run outcome**:
The terminal result of an Extraction run, distinguishing no selected documents, no produced output, and produced output. Produced output retains its output-purpose classification and only the applicable conversion and GIF-routing facts; the outcome excludes terminal wording and raw cross-module counters.
_Avoid_: Run report, extraction summary, final counters

**Extraction run observation**:
One structured, ordered fact emitted while an Extraction run progresses, spanning Document selection progress and diagnostics, per-document extraction status, and the terminal Extraction run outcome. It excludes terminal wording and user-interface commands.
_Avoid_: Run event, progress callback, UI command

**Document extraction**:
The per-document responsibility within an Extraction run for turning one selected document into an extraction outcome, including document-kind handling, cover behavior, warnings, and output classification. It excludes Document selection, cross-document sequencing, and run-level presentation.
_Avoid_: Per-document execution, document processor

**Document extraction outcome**:
The terminal result of Document extraction: either completed or failed with a document-local error, while retaining any image-write facts produced before completion or failure. A failed outcome does not end the Extraction run or imply that no files were emitted.
_Avoid_: Per-file result, extraction return value

**Document extraction facts**:
The opaque facts retained by a Document extraction outcome, including emitted-image totals, output-purpose classification, conversion and GIF-routing totals, and ordered Document extraction warnings.
_Avoid_: Image write result, extraction summary, raw counters

**Document extraction warning**:
A non-fatal, user-observable fact produced by Document extraction, exposed with stable wording while its Image write pipeline classification remains internal.
_Avoid_: Image write warning, warning string

**Document extraction error**:
The document-local failure attached to a failed Document extraction outcome. It preserves its underlying cause without exposing document-adapter or Image write pipeline error types across the Document extraction seam.
_Avoid_: Image write failure, raw anyhow error

**Document extraction policy**:
The valid per-run choices governing normal document images versus EPUB cover extraction and optional cover fallback. It excludes Image formats, conversion, GIF routing, and Image file emission choices, which belong to Image write policy.
_Avoid_: Cover flags, extraction booleans

**Document selection**:
The part of an Extraction run that decides which discovered documents are eligible to process and what document-level facts are known before extraction, including EPUB filtering, duplicate handling, display identity, and per-document output placement. A selected document's display identity is stable across Document extraction policies: EPUB declarations supply it when available, otherwise selection uses its path identity.
_Avoid_: File collection, scan results, work item builder

**Document discovery**:
The part of Document selection that inspects requested files and directories, reports non-fatal inspection failures, and yields supported document candidates in encounter order. It excludes EPUB filtering, deduplication, identity, and output placement.
_Avoid_: File collection, directory scan, input traversal, source discovery

**Selected document**:
The immutable handoff produced by Document selection for one eligible document, containing its source identity, document kind, output placement, display identity, and any retained EPUB declarations. Its document kind is authoritative, Document extraction consumes it exactly once, and later declaration acquisition cannot revise its identity or placement.
_Avoid_: Extraction work item, selected file, document task

**Document selection progress**:
The live, user-observable status of Document selection while it scans inputs, filters EPUBs, and removes duplicates. It excludes per-document extraction status and terminal presentation details.
_Avoid_: Run events, selection UI events, progress callbacks

**Document selection diagnostic**:
A non-fatal fact explaining why Document selection skipped a requested input, could not inspect part of its document search, or could not use document metadata. It excludes per-document extraction warnings and terminal wording.
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
The EPUB-only responsibility for identifying and ordering cover candidates and turning them into one required-cover outcome, including acquisition retry, avoiding repeated attempts at the same Archive resource identity, cover-specific Image format and Conversion policy, and optional fallback to normal images. It excludes EPUB declaration acquisition, archive resource reading mechanics, and Image file emission.
_Avoid_: Cover pipeline, cover selection, cover helper

**Archive image discovery**:
The per-resource policy within the Image write pipeline for acquiring archive sources and deciding which may be emitted, including source safety, Image format identification, requested format filtering, and non-fatal acquisition or fallback warning facts.
_Avoid_: Candidate normalization, resource filter

**Archive resource identity**:
The stable identity of one archive payload across multiple document references within a single EPUB resource archive, used to recognize repeated attempts and exclusions without treating reference spelling as payload identity. It has no equality meaning across different archives or archive sessions. References that cannot be resolved to a payload remain distinct.
_Avoid_: ZIP index, resource path

**EPUB resource archive**:
The ordered custody of resources declared by an EPUB together with their available archive payloads. It owns deterministic ordering, the distinction between document-facing references and Archive resource identity, and scoped payload acquisition. Failure to establish that custody fails Document extraction, while the unavailability of one declared resource is a non-fatal acquisition fact. It excludes cover-candidate ordering, retry or completion decisions, fallback behavior, Archive image discovery, and Image file emission.
_Avoid_: Direct ZIP adapter, resource list

**EPUB declarations**:
The payload-free facts declared by an EPUB and retained across Document selection and Document extraction, including descriptive metadata, cover identity, and resource declarations. Once retained, they are the authoritative declaration facts for that Extraction run. They exclude archive payload acquisition and Archive resource identity resolution.
_Avoid_: EPUB metadata, manifest snapshot, EPUB document model

**Image format**:
The project-normalized identity of image bytes used to decide whether an image is extractable, writable, and convertible, independent of how a document labels it.
_Avoid_: Raw extension, MIME type, detected kind

**Conversion policy**:
The valid target encoding requested for extracted images, including only the settings meaningful to that target and whether matching source bytes should be preserved. It excludes GIF routing and batch-versus-cover fallback behavior.
_Avoid_: Conversion flags, conversion options
