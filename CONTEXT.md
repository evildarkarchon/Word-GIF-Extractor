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

**Applicable outcome facts**:
Which optional fact groups the eventual Extraction run outcome may carry — the applicable conversion and GIF-routing facts — together with the GIF destination when routing applies. Document extraction reports it from Image write policy at the moment an Extraction run needs it. It excludes counts, terminal wording, and outcome classification. Cover intent is classification input rather than an applicable fact group: whether a run sought covers reaches the Extraction run outcome at the moment that outcome is classified, never through this value.
_Avoid_: Conversion requested flag, run summary facts, presentation intent

**Extraction run observation**:
One structured, ordered fact emitted while an Extraction run progresses, spanning Document selection progress and diagnostics, per-document extraction status, and the terminal Extraction run outcome. It is the run's single observation vocabulary: every emitter reports through it, so no part of the run transports facts belonging to a second one. It excludes terminal wording, user-interface commands, and run policy.
_Avoid_: Run event, progress callback, UI command

**Extraction run presentation**:
The project responsibility for turning Extraction run observations into terminal output, including progress display, diagnostic and warning rendering, terminal wording, and the terminal summary. It owns all user-facing wording and excludes outcome classification and observation ordering.
_Avoid_: Terminal adapter, observer, renderer, UI

**Document extraction**:
The per-document responsibility within an Extraction run for turning one selected document into an extraction outcome, including document-kind handling, cover behavior, warnings, and output classification. It excludes Document selection, cross-document sequencing, and run-level presentation.
_Avoid_: Per-document execution, document processor

**Document extraction outcome**:
The terminal result of Document extraction: either completed or failed with a document-local error, while retaining any image-write facts produced before completion or failure. A failed outcome does not end the Extraction run or imply that no files were emitted.
_Avoid_: Per-file result, extraction return value

**Document extraction facts**:
The opaque facts retained by a Document extraction outcome, including emitted-image totals, output-purpose classification, conversion and GIF-routing totals, and ordered Document extraction warnings. Its emitted-image totals are one value in which the converted, conversion-skipped and GIF-routed counts together never exceed the emitted count, because the Image write pipeline places each emitted image in exactly one of those roles; folding the facts of several documents therefore cannot produce an Extraction run outcome with inconsistent totals. Its output-purpose classification is a closed three-way value — covers only, included normal images, or nothing emitted — rather than a normal-image boolean.
_Avoid_: Image write result, extraction summary, raw counters

**Document extraction warning**:
A non-fatal, user-observable fact produced by Document extraction, exposed with stable wording while its Image write pipeline classification remains internal.
_Avoid_: Image write warning, warning string

**Document extraction error**:
The document-local failure attached to a failed Document extraction outcome. It preserves its underlying cause without exposing document-adapter or Image write pipeline error types across the Document extraction seam.
_Avoid_: Image write failure, raw anyhow error

**EPUB cover policy**:
The per-run choice to extract a required EPUB cover instead of normal document images, and whether an EPUB without a usable cover falls back to normal images. Its absence is what asks for normal images, so a run seeking no covers carries no cover policy at all and cover behaviour reaches only the document kind that has covers. It excludes Image formats, conversion, GIF routing, and Image file emission choices, which belong to Image write policy.
_Avoid_: Document extraction policy, cover flags, extraction booleans

**Document selection**:
The part of an Extraction run that decides which discovered documents are eligible to process and what document-level facts are known before extraction, including EPUB filtering, duplicate handling, display identity, and per-document output placement. A run may restrict eligibility to EPUB documents alone; selection is told that eligibility is restricted, never why, and reports each document it skips for that reason as a Document selection diagnostic. A selected document's display identity is stable across EPUB cover policies: EPUB declarations supply it when available, otherwise selection uses its path identity.
_Avoid_: File collection, scan results, work item builder

**Document discovery**:
The part of Document selection that inspects requested files and directories through the Document search surface, reports non-fatal inspection failures, and yields supported document candidates in encounter order. It excludes EPUB filtering, deduplication, identity, and output placement.
_Avoid_: File collection, directory scan, input traversal, source discovery

**Document search surface**:
What Document discovery can observe about the world it searches — what one path is, with and without following links; what one directory directly contains; and what a recursive traversal of one directory yields, in encounter order. Every observation may instead report a failure, and a failure to observe a genuinely absent path is distinguishable from every other failure. A traversal failure knows its position in the traversal even when it does not know the path it belongs to. It excludes document-kind classification, EPUB declarations, and archive payload reads.
_Avoid_: Filesystem, file system adapter, VFS, path provider

**Selected document**:
The immutable handoff produced by Document selection for one eligible document, containing its source identity, document kind, output placement, display identity, and any retained EPUB declarations. Its document kind is authoritative, Document extraction consumes it exactly once, and later declaration acquisition cannot revise its identity or placement.
_Avoid_: Extraction work item, selected file, document task

**Source identity**:
The path a Selected document was found at, fixed by Document selection and unchanged by anything Document extraction later learns. It is distinct from output placement even when output placement is derived from it.
_Avoid_: Input path, file path, source file

**Output placement**:
Where one selected document's emitted files go and what they are named — the output directory together with the output filename stem, decided by Document selection before extraction begins. Deriving the directory from the source identity does not make them one fact. It excludes collision handling and per-image naming, which belong to Image file emission.
_Avoid_: Output dir, output path, base name

**Document selection progress**:
The live, user-observable status of Document selection while it discovers documents, filters EPUBs, and removes duplicates. Each phase reports a running status and exactly one finished status, both as Extraction run observations. Discovery has no denominator until it has finished finding, so it reports only a growing count, while filtering and deduplication run against a known candidate set every member of which must be accounted for before their finished status is meaningful. It excludes per-document extraction status and terminal presentation details.
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

**Emitted image role**:
The closed four-way role one image takes as the Image write pipeline emits it: GIF-routed, converted, conversion-skipped, or preserved. Exactly one role applies to each emitted image, which is what keeps the converted, conversion-skipped and GIF-routed counts from together exceeding the emitted count. A GIF-routed role carries the destination that routing sends it to, so a routed image and its destination cannot be decided apart. Preserved covers both an unrequested conversion and a conversion that kept a matching source, because neither is counted and the applicable warning fact carries the difference. It excludes Image write purpose, output naming, and the counts it is folded into.
_Avoid_: Conversion flags, emission booleans, write mode

**EPUB cover extraction**:
The EPUB-only responsibility for identifying and ordering cover candidates and turning them into one required-cover outcome, including acquisition retry, avoiding repeated attempts at the same Archive resource identity, cover-specific Image format and Conversion policy, and optional fallback to normal images. It excludes EPUB declaration acquisition, archive resource reading mechanics, and Image file emission.
_Avoid_: Cover pipeline, cover selection, cover helper

**Cover candidate**:
One declared manifest resource considered for cover use by EPUB cover extraction, carrying the declared facts that ordering and exclusion are decided from. Several distinct cover candidates can share a single Archive resource identity, so a candidate is a reference under consideration rather than the payload it resolves to. It excludes archive payload acquisition and Image write decisions.
_Avoid_: Cover match, cover entry, candidate path

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
