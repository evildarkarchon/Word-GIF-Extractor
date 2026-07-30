# Graph Report - Word-GIF-Extractor  (2026-07-30)

## Corpus Check
- 58 files · ~62,997 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1022 nodes · 2478 edges · 52 communities (48 shown, 4 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 27 edges (avg confidence: 0.84)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `6f2c3d9e`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- image_write_pipeline.rs
- epub.rs
- document_selection.rs
- extraction_run.rs
- ImageFormat
- main.rs
- conversion.rs
- document_extraction.rs
- Document Selection
- DocumentSelectionDiagnostic
- .new
- extraction_run_intake/tests.rs
- complete_reserved_file
- document_extraction_warning_cli.rs
- EpubDeclarations
- conversion_policy_cli.rs
- Incremental Update Flow
- Full Graphify Pipeline
- process_file
- document_selection_diagnostics_cli.rs
- Semantic Update Path
- Constrained Query Expansion
- beside_file_output.rs
- Media Transcription Flow
- mislabeled_docx.rs
- Extraction Subagent Contract
- Graph Query Flow
- Graph Build and Analysis
- pre_run_notices_cli.rs
- .new
- RecordingTerm
- IndicatifRunObserver
- Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?
- What You Must Do When Invoked
- resource_archive/tests.rs
- EpubFilter
- DocumentExtractionWarning
- graphify reference: extra exports and benchmark
- .classify
- graphify reference: query, path, explain
- .from
- Q: Does the refreshed graph reflect Document extraction as the warning-wording owner?
- graphify reference: add a URL and watch a folder
- graphify reference: commit hook and native CLAUDE.md integration
- graphify reference: incremental update and cluster-only
- graphify reference: GitHub clone and cross-repo merge
- graphify reference: transcribe video and audio
- CLAUDE.md
- extraction-spec.md
- epub_declarations/tests.rs
- DocumentSelectionLifecycle<'observer>

## God Nodes (most connected - your core abstractions)
1. `ImageFormat` - 38 edges
2. `select_documents()` - 37 edges
3. `temp_test_dir()` - 30 edges
4. `extract()` - 28 edges
5. `ArchiveImageSource` - 26 edges
6. `select_epub()` - 25 edges
7. `temp_test_dir()` - 24 edges
8. `temp_test_dir()` - 23 edges
9. `write_sources()` - 23 edges
10. `convert_image()` - 21 edges

## Surprising Connections (you probably didn't know these)
- `Document Archive Extraction Flow` --semantically_similar_to--> `Extraction Run`  [INFERRED] [semantically similar]
  AGENTS.md → CONTEXT.md
- `Image Conversion Behavior` --semantically_similar_to--> `Conversion Policy`  [INFERRED] [semantically similar]
  README.md → CONTEXT.md
- `Word Image Extractor` --semantically_similar_to--> `Word Image Extractor CLI`  [INFERRED] [semantically similar]
  README.md → AGENTS.md
- `EPUB Declaration Acquisition` --semantically_similar_to--> `EPUB Declarations`  [INFERRED] [semantically similar]
  docs/adr/0001-reopen-epub-for-incremental-resource-reads.md → CONTEXT.md
- `Retained EPUB Declarations` --semantically_similar_to--> `EPUB Declarations`  [INFERRED] [semantically similar]
  docs/adr/0002-retain-epub-declarations-for-the-extraction-run.md → CONTEXT.md

## Import Cycles
- 2-file cycle: `src/extraction_run.rs -> src/extraction_run_intake.rs -> src/extraction_run.rs`
- 2-file cycle: `src/extraction_run.rs -> src/main.rs -> src/extraction_run.rs`
- 2-file cycle: `src/extraction_run_intake.rs -> src/main.rs -> src/extraction_run_intake.rs`
- 2-file cycle: `src/document_selection.rs -> src/document_selection/progress.rs -> src/document_selection.rs`
- 3-file cycle: `src/extraction_run.rs -> src/extraction_run_intake.rs -> src/main.rs -> src/extraction_run.rs`
- 3-file cycle: `src/extraction_run.rs -> src/main.rs -> src/extraction_run_intake.rs -> src/extraction_run.rs`

## Hyperedges (group relationships)
- **Graphify Extraction and Build Flow** — _codex_skills_graphify_skill_parallel_structural_and_semantic_extraction, _codex_skills_graphify_references_extraction_spec_extraction_subagent_contract, _codex_skills_graphify_skill_graph_build_and_analysis, _codex_skills_graphify_skill_graph_health_gate [EXTRACTED 1.00]
- **Graph Maintenance Paths** — _codex_skills_graphify_references_add_watch_folder_watch_mode, _codex_skills_graphify_references_hooks_post_commit_graph_hook, _codex_skills_graphify_references_update_incremental_update_flow [INFERRED 0.85]
- **Query Feedback Cycle** — _codex_skills_graphify_references_query_constrained_query_expansion, _codex_skills_graphify_references_query_graph_grounded_answering, _codex_skills_graphify_references_query_save_result_feedback, _codex_skills_graphify_references_query_work_memory_reflection [EXTRACTED 1.00]
- **Extraction Run Flow** — context_extraction_run_intake, context_extraction_run_request, context_extraction_run, context_document_selection, context_document_extraction, context_extraction_run_outcome, context_extraction_run_observation [EXTRACTED 1.00]
- **Image Write Flow** — context_image_write_policy, context_image_write_pipeline, context_archive_image_discovery, context_image_file_emission, context_image_format, context_conversion_policy, context_image_write_purpose [EXTRACTED 1.00]

## Communities (52 total, 4 thin omitted)

### Community 0 - "image_write_pipeline.rs"
Cohesion: 0.07
Nodes (77): Cursor, accepted_source_reuses_evidence_prefix_and_completes_payload_incrementally(), AcceptedImage, ArchiveImageVisitor, ArchiveImageVisitor<'policy, 'request>, AssertOutputBeforeTailReader, bom_prefixed_svg_at_end_of_evidence_window_is_discovered(), bom_prefixed_svg_beyond_evidence_window_is_not_discovered() (+69 more)

### Community 1 - "epub.rs"
Cohesion: 0.06
Nodes (77): PhantomData, acquisition_failure_sources(), archive_open_failure_after_selection_is_a_fatal_extraction_error(), archive_parse_failure_after_selection_is_a_fatal_extraction_error(), corrupt_stored_payload(), cover_emission_failure_aborts_the_document(), cover_retries_precede_partial_normal_fallback_facts(), cover_retry_warnings_precede_normal_fallback_warning() (+69 more)

### Community 2 - "document_selection.rs"
Cohesion: 0.15
Nodes (38): create_directory_link(), create_file_symlink(), declaration_deduplication_falls_back_to_filename_when_declarations_cannot_be_read(), format_epub_base_name(), remove_directory_link(), remove_file_symlink(), sanitize_filename(), select_documents() (+30 more)

### Community 3 - "extraction_run.rs"
Cohesion: 0.09
Nodes (51): NonZeroUsize, Observer, all_failed_requested_inputs_reach_one_no_documents_terminal_observation(), assert_single_terminal_observation(), ConversionAggregation, ConversionFacts, create_directory_link(), DocumentSelectionObservationAdapter (+43 more)

### Community 4 - "ImageFormat"
Cohesion: 0.06
Nodes (37): Action, EpubImagePlan<'session>, Self, ImageFormat, HashSet, Option, ArchiveImageDiscoveryOutcome, ArchiveImageSource (+29 more)

### Community 5 - "main.rs"
Cohesion: 0.07
Nodes (7): document_warning_line(), document_warning_presentation_adds_one_prefix_and_suspends_extraction_progress(), observer_temp_test_dir(), recursive_discovery_diagnostic_suspends_active_scan_spinner(), Path, PathBuf, write_docx()

### Community 6 - "conversion.rs"
Cohesion: 0.09
Nodes (52): DynamicImage, CodecTarget, composite_on_white(), conversion_policy_converts_supported_source(), ConversionMode, ConversionOutcome, ConversionPolicy, ConversionPolicyError (+44 more)

### Community 7 - "document_extraction.rs"
Cohesion: 0.10
Nodes (35): conversion_policy(), DocumentExtraction, DocumentExtractionError, DocumentExtractionFacts, DocumentExtractionOutcome, DocumentExtractionPolicy, docx_uses_normal_images_when_policy_requests_an_epub_cover(), docx_warning_bodies_keep_source_format_base_name_detail_multiplicity_and_phase_order() (+27 more)

### Community 8 - "Document Selection"
Cohesion: 0.06
Nodes (44): Document Archive Extraction Flow, Graphify Workflow, Word Image Extractor CLI, Repository Guidance, Archive Image Discovery, Archive Resource Identity, Conversion Policy, Document Discovery (+36 more)

### Community 9 - "DocumentSelectionDiagnostic"
Cohesion: 0.09
Nodes (20): SilentDocumentSelectionObserver, DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionPhaseStatus, DocumentSelectionProgress, DocumentSelectionScanScope, EpubDeduplicationCheck, EpubDeduplicationProgress (+12 more)

### Community 10 - ".new"
Cohesion: 0.19
Nodes (19): DocumentCandidate, DocumentSelectionOptions, fallback_base_name(), fallback_display_name(), filename_dedupe_key(), is_epub(), resolve_output_dir(), resolves_output_dir_absolute_input() (+11 more)

### Community 11 - "extraction_run_intake/tests.rs"
Cohesion: 0.10
Nodes (36): ExtractionRunIntakeError, prepare(), PreparedExtractionRun, PreRunNotice, Display, Error, Formatter, HashSet (+28 more)

### Community 12 - "complete_reserved_file"
Cohesion: 0.16
Nodes (18): candidate_path(), complete_reserved_file(), FileCompletionError, FileCompletionStage, ImageFileEmission<'name>, Error, File, FnOnce (+10 more)

### Community 13 - "document_extraction_warning_cli.rs"
Cohesion: 0.53
Nodes (5): renders_document_extraction_warning_with_one_prefix_and_no_document_path(), Path, PathBuf, temp_test_dir(), write_warning_docx()

### Community 14 - "EpubDeclarations"
Cohesion: 0.12
Nodes (15): DocError, EpubDeclarationError, EpubDeclarations, EpubResourceDeclaration, Display, Error, Formatter, Into (+7 more)

### Community 15 - "conversion_policy_cli.rs"
Cohesion: 0.32
Nodes (11): Output, matching_jpeg_is_preserved_when_quality_is_implicit(), matching_jpeg_is_reencoded_when_quality_is_explicit(), Option, Path, PathBuf, Vec, run_jpeg_conversion() (+3 more)

### Community 16 - "Incremental Update Flow"
Cohesion: 0.20
Nodes (10): Supported URL Types, URL Ingestion, Verbatim Source Identity, Changed Subset and Full Corpus, Deletion Pruning, Directed Update Parity, Graph Diff, Incremental Update Flow (+2 more)

### Community 17 - "Full Graphify Pipeline"
Cohesion: 0.20
Nodes (10): Graph Database Exports, MCP Graph Server, Optional Graph Exports, Token Reduction Benchmark, Cross-Repository Graph Merge, GitHub Clone Flow, Monorepo Isolated Extraction, Repository Origin Attribute (+2 more)

### Community 18 - "process_file"
Cohesion: 0.29
Nodes (9): process_file(), ImageWriteOutcome, Path, preserves_zip_order_for_numbered_outputs(), returns_extension_fallback_warning_fact(), Path, PathBuf, temp_test_dir() (+1 more)

### Community 19 - "document_selection_diagnostics_cli.rs"
Cohesion: 0.47
Nodes (9): create_directory_link(), remove_directory_link(), Path, PathBuf, temp_test_dir(), warns_for_broken_requested_link_before_no_documents_summary(), warns_once_for_broken_nested_link_during_non_recursive_discovery(), warns_once_for_broken_nested_link_during_recursive_discovery() (+1 more)

### Community 20 - "Semantic Update Path"
Cohesion: 0.29
Nodes (7): Folder Watch Mode, Semantic Update Flag, Watch Debounce, Code-Only Hook Update, Post-Commit Graph Hook, Code-Only Update Path, Semantic Update Path

### Community 21 - "Constrained Query Expansion"
Cohesion: 0.33
Nodes (7): Breadth-First Traversal, Constrained Query Expansion, Depth-First Traversal, Graph-Grounded Answering, Graph Vocabulary, Save-Result Feedback Loop, Work Memory Reflection

### Community 22 - "beside_file_output.rs"
Cohesion: 0.48
Nodes (6): extracts_beside_input_when_output_omitted(), has_png_files(), Path, PathBuf, temp_test_dir(), write_minimal_docx()

### Community 23 - "Media Transcription Flow"
Cohesion: 0.33
Nodes (6): Domain Hint Prompt, Media Transcription Flow, Transcript Documents, Whisper Configuration, Parallel Structural and Semantic Extraction, Semantic Extraction Cache

### Community 24 - "mislabeled_docx.rs"
Cohesion: 0.53
Nodes (5): extracts_mislabeled_png_when_filtering_for_png(), Path, PathBuf, temp_test_dir(), write_mislabeled_docx()

### Community 25 - "Extraction Subagent Contract"
Cohesion: 0.40
Nodes (5): Deterministic Node Identity, Extraction Subagent Contract, Hyperedge Extraction, Provenance Confidence Model, Semantic Similarity Edges

### Community 26 - "Graph Query Flow"
Cohesion: 0.40
Nodes (5): Claude Project Integration, Graph Query Flow, Node Explanation, Shortest Path Query, Existing Graph Fast Path

### Community 27 - "Graph Build and Analysis"
Cohesion: 0.40
Nodes (5): Cluster-Only Refresh, Community-Labeled Graph Outputs, Graph Build and Analysis, Graph Health Gate, Graph Honesty Rules

### Community 28 - "pre_run_notices_cli.rs"
Cohesion: 0.83
Nodes (3): renders_ordered_pre_run_notices_on_existing_streams(), PathBuf, temp_test_dir()

### Community 29 - ".new"
Cohesion: 0.22
Nodes (12): assert_terminal_observation_finishes_extraction(), combined_conversion_and_gif_summary_uses_semantic_outcome(), conversion_summary_reports_preserved_matching_source_as_unconverted(), default_output_summary_preserves_existing_wording(), epub_cover_fallback_summary_reports_normal_images(), epub_filter_description(), final_summary_message(), main() (+4 more)

### Community 30 - "RecordingTerm"
Cohesion: 0.27
Nodes (3): RecordingTerm, Result, TermLike

### Community 31 - "IndicatifRunObserver"
Cohesion: 0.30
Nodes (7): F, ProgressBar, ProgressStyle, create_progress_style(), create_spinner_style(), IndicatifRunObserver, Option

### Community 32 - "Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?, Source Nodes

### Community 33 - "What You Must Do When Invoked"
Cohesion: 0.08
Nodes (24): For /graphify add and --watch, For /graphify query, For the commit hook and native CLAUDE.md integration, For --update and --cluster-only, /graphify, Honesty Rules, Interpreter guard for subcommands, Part A - Structural extraction for code files (+16 more)

### Community 34 - "resource_archive/tests.rs"
Cohesion: 0.19
Nodes (20): catalog_acquisition_is_lazy_repeatable_and_keyed_to_its_session(), consumer_failure_propagates_with_its_concrete_error_identity(), ConsumerFailure, exact_manifest_path_wins_before_percent_decoded_alias(), invalid_percent_encoded_path_is_retained_as_typed_acquisition_failure(), malformed_percent_escape_is_retained_as_typed_acquisition_failure(), mark_first_entry_encrypted(), percent_decoded_aliases_share_archive_resource_identity() (+12 more)

### Community 35 - "EpubFilter"
Cohesion: 0.29
Nodes (9): deduplicate_epubs_by_declarations(), discover_documents(), Vec, epub_dedupe_key(), EpubFilter, filter_epub_files(), matches_filter(), DocumentSelectionLifecycle (+1 more)

### Community 36 - "DocumentExtractionWarning"
Cohesion: 0.28
Nodes (8): Arc, Mutex, DocumentExtractionWarning, String, FilesystemIndicatifObserver, Vec, TerminalActivity, WarningPresentationObserver

### Community 37 - "graphify reference: extra exports and benchmark"
Cohesion: 0.22
Nodes (8): graphify reference: extra exports and benchmark, Step 6b - Wiki (only if --wiki flag), Step 7 - Neo4j export (only if --neo4j or --neo4j-push flag), Step 7a - FalkorDB export (only if --falkordb or --falkordb-push flag), Step 7b - SVG export (only if --svg flag), Step 7c - GraphML export (only if --graphml flag), Step 7d - MCP server (only if --mcp flag), Step 8 - Token reduction benchmark (only if total_words > 5000)

### Community 38 - ".classify"
Cohesion: 0.24
Nodes (8): RequestedInput, RequestedInputFailure, Error, Option, Path, PathBuf, Result, Self

### Community 39 - "graphify reference: query, path, explain"
Cohesion: 0.33
Nodes (5): For /graphify explain, For /graphify path, graphify reference: query, path, explain, Step 0 — Constrained query expansion (REQUIRED before traversal), Step 1 — Traversal

### Community 40 - ".from"
Cohesion: 0.40
Nodes (5): ConversionTarget, ConversionTargetArg, gif_routing_summary_preserves_existing_wording(), From, Self

### Community 41 - "Q: Does the refreshed graph reflect Document extraction as the warning-wording owner?"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: Does the refreshed graph reflect Document extraction as the warning-wording owner?, Source Nodes

### Community 42 - "graphify reference: add a URL and watch a folder"
Cohesion: 0.50
Nodes (3): For /graphify add, For --watch, graphify reference: add a URL and watch a folder

### Community 43 - "graphify reference: commit hook and native CLAUDE.md integration"
Cohesion: 0.50
Nodes (3): For git commit hook, For native CLAUDE.md integration, graphify reference: commit hook and native CLAUDE.md integration

### Community 44 - "graphify reference: incremental update and cluster-only"
Cohesion: 0.50
Nodes (3): For --cluster-only, For --update (incremental re-extraction), graphify reference: incremental update and cluster-only

### Community 49 - "epub_declarations/tests.rs"
Cohesion: 0.47
Nodes (8): acquires_complete_payload_free_epub_declarations(), Path, PathBuf, sparse_epub_declarations_are_a_successful_acquisition(), temp_test_dir(), write_epub(), write_epub_with_cover(), write_sparse_epub()

### Community 50 - "DocumentSelectionLifecycle<'observer>"
Cohesion: 0.48
Nodes (3): R, DocumentSelectionLifecycle<'observer>, FnOnce

## Knowledge Gaps
- **79 isolated node(s):** `SessionBrand<'session>`, `DocumentSelectionObservationAdapter<'observer, Observer>`, `ImageWriteRequest<'a>`, `RequiredCoverWriteRequest<'a>`, `graphify` (+74 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **4 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ImageFormat` connect `ImageFormat` to `image_write_pipeline.rs`, `epub.rs`, `extraction_run.rs`, `conversion.rs`, `extraction_run_intake/tests.rs`, `complete_reserved_file`?**
  _High betweenness centrality (0.099) - this node is a cross-community bridge._
- **Why does `EpubDeclarations` connect `EpubDeclarations` to `EpubFilter`, `document_selection.rs`, `.new`, `epub.rs`?**
  _High betweenness centrality (0.052) - this node is a cross-community bridge._
- **Why does `ArchiveImageSource` connect `ImageFormat` to `image_write_pipeline.rs`?**
  _High betweenness centrality (0.025) - this node is a cross-community bridge._
- **Are the 4 inferred relationships involving `select_documents()` (e.g. with `retained_epub_declarations_are_authoritative_during_extraction()` and `select_one_document()`) actually correct?**
  _`select_documents()` has 4 INFERRED edges - model-reasoned connections that need verification._
- **What connects `SessionBrand<'session>`, `DocumentSelectionObservationAdapter<'observer, Observer>`, `ImageWriteRequest<'a>` to the rest of the system?**
  _79 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `image_write_pipeline.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.06872393661384488 - nodes in this community are weakly interconnected._
- **Should `epub.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.062421972534332085 - nodes in this community are weakly interconnected._