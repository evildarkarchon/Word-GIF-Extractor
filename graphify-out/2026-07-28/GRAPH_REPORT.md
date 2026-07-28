# Graph Report - Word-GIF-Extractor  (2026-07-28)

## Corpus Check
- 40 files · ~49,909 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 918 nodes · 2385 edges · 33 communities
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 19 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `00d78e58`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- image_write_pipeline.rs
- resource_archive.rs
- document_selection.rs
- extraction_run.rs
- ImageFormat
- main.rs
- conversion.rs
- document_extraction.rs
- Document Selection
- DocumentSelectionDiagnostic
- epub.rs
- extraction_run_intake.rs
- emission.rs
- .classify
- EpubResourceDeclaration
- conversion_policy_cli.rs
- Incremental Update Flow
- Full Graphify Pipeline
- docx.rs
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
- produced_outcome
- RecordingTerm
- IndicatifRunObserver
- Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?

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
- 2-file cycle: `src/extraction_run_intake.rs -> src/main.rs -> src/extraction_run_intake.rs`
- 2-file cycle: `src/extraction_run.rs -> src/main.rs -> src/extraction_run.rs`
- 2-file cycle: `src/document_selection.rs -> src/document_selection/progress.rs -> src/document_selection.rs`
- 3-file cycle: `src/extraction_run.rs -> src/main.rs -> src/extraction_run_intake.rs -> src/extraction_run.rs`
- 3-file cycle: `src/extraction_run.rs -> src/extraction_run_intake.rs -> src/main.rs -> src/extraction_run.rs`

## Hyperedges (group relationships)
- **Graphify Extraction and Build Flow** — _codex_skills_graphify_skill_parallel_structural_and_semantic_extraction, _codex_skills_graphify_references_extraction_spec_extraction_subagent_contract, _codex_skills_graphify_skill_graph_build_and_analysis, _codex_skills_graphify_skill_graph_health_gate [EXTRACTED 1.00]
- **Graph Maintenance Paths** — _codex_skills_graphify_references_add_watch_folder_watch_mode, _codex_skills_graphify_references_hooks_post_commit_graph_hook, _codex_skills_graphify_references_update_incremental_update_flow [INFERRED 0.85]
- **Query Feedback Cycle** — _codex_skills_graphify_references_query_constrained_query_expansion, _codex_skills_graphify_references_query_graph_grounded_answering, _codex_skills_graphify_references_query_save_result_feedback, _codex_skills_graphify_references_query_work_memory_reflection [EXTRACTED 1.00]
- **Extraction Run Flow** — context_extraction_run_intake, context_extraction_run_request, context_extraction_run, context_document_selection, context_document_extraction, context_extraction_run_outcome, context_extraction_run_observation [EXTRACTED 1.00]
- **Image Write Flow** — context_image_write_policy, context_image_write_pipeline, context_archive_image_discovery, context_image_file_emission, context_image_format, context_conversion_policy, context_image_write_purpose [EXTRACTED 1.00]

## Communities (33 total, 0 thin omitted)

### Community 0 - "image_write_pipeline.rs"
Cohesion: 0.07
Nodes (77): Cursor, accepted_source_reuses_evidence_prefix_and_completes_payload_incrementally(), AcceptedImage, ArchiveImageVisitor, ArchiveImageVisitor<'policy, 'request>, AssertOutputBeforeTailReader, bom_prefixed_svg_at_end_of_evidence_window_is_discovered(), bom_prefixed_svg_beyond_evidence_window_is_not_discovered() (+69 more)

### Community 1 - "resource_archive.rs"
Cohesion: 0.09
Nodes (48): PhantomData, archive_path(), ArchiveResourceIdentity, catalog_acquisition_is_lazy_repeatable_and_keyed_to_its_session(), CatalogSeed, consumer_failure_propagates_with_its_concrete_error_identity(), ConsumerFailure, EpubResource (+40 more)

### Community 2 - "document_selection.rs"
Cohesion: 0.09
Nodes (66): create_directory_link(), create_file_symlink(), declaration_deduplication_falls_back_to_filename_when_declarations_cannot_be_read(), deduplicate_epubs_by_declarations(), DocumentCandidate, DocumentSelectionOptions, epub_dedupe_key(), EpubFilter (+58 more)

### Community 3 - "extraction_run.rs"
Cohesion: 0.09
Nodes (50): NonZeroUsize, Observer, all_failed_requested_inputs_reach_one_no_documents_terminal_observation(), assert_single_terminal_observation(), ConversionAggregation, ConversionFacts, create_directory_link(), DocumentSelectionObservationAdapter (+42 more)

### Community 4 - "ImageFormat"
Cohesion: 0.06
Nodes (37): Action, EpubImagePlan<'session>, Self, ImageFormat, HashSet, Option, ArchiveImageDiscoveryOutcome, ArchiveImageSource (+29 more)

### Community 5 - "main.rs"
Cohesion: 0.07
Nodes (3): observer_temp_test_dir(), recursive_discovery_diagnostic_suspends_active_scan_spinner(), PathBuf

### Community 6 - "conversion.rs"
Cohesion: 0.09
Nodes (52): DynamicImage, CodecTarget, composite_on_white(), conversion_policy_converts_supported_source(), ConversionMode, ConversionOutcome, ConversionPolicy, ConversionPolicyError (+44 more)

### Community 7 - "document_extraction.rs"
Cohesion: 0.09
Nodes (29): DocumentExtraction, DocumentExtractionError, DocumentExtractionFacts, DocumentExtractionOutcome, DocumentExtractionPolicy, DocumentExtractionWarning, docx_uses_normal_images_when_policy_requests_an_epub_cover(), epub_cover_fallback_is_classified_as_normal_images() (+21 more)

### Community 8 - "Document Selection"
Cohesion: 0.06
Nodes (44): Document Archive Extraction Flow, Graphify Workflow, Word Image Extractor CLI, Repository Guidance, Archive Image Discovery, Archive Resource Identity, Conversion Policy, Document Discovery (+36 more)

### Community 9 - "DocumentSelectionDiagnostic"
Cohesion: 0.08
Nodes (24): R, SilentDocumentSelectionObserver, DocumentSelectionDiagnostic, DocumentSelectionLifecycle, DocumentSelectionLifecycle<'observer>, DocumentSelectionObserver, DocumentSelectionPhaseStatus, DocumentSelectionProgress (+16 more)

### Community 10 - "epub.rs"
Cohesion: 0.16
Nodes (44): acquisition_failure_sources(), archive_open_failure_after_selection_is_a_fatal_extraction_error(), archive_parse_failure_after_selection_is_a_fatal_extraction_error(), corrupt_stored_payload(), cover_emission_failure_aborts_the_document(), cover_retries_precede_partial_normal_fallback_facts(), cover_retry_warnings_precede_normal_fallback_warning(), encoded_test_png() (+36 more)

### Community 11 - "extraction_run_intake.rs"
Cohesion: 0.11
Nodes (35): builds_default_conversion_policy(), builds_validated_epub_cover_extraction_policy(), combines_positional_and_named_inputs(), defaults_to_current_directory_when_inputs_are_empty(), execute(), ExtractionRunIntakeError, falls_back_to_all_formats_when_no_valid_formats_are_supplied(), gif_only_overrides_format_selection() (+27 more)

### Community 12 - "emission.rs"
Cohesion: 0.21
Nodes (17): candidate_path(), cleanup_failure_is_reported_with_the_original_write_failure(), complete_reserved_file(), FileCompletionError, FileCompletionStage, ImageFileEmission<'name>, Error, File (+9 more)

### Community 13 - ".classify"
Cohesion: 0.23
Nodes (10): discover_documents(), RequestedInput, RequestedInputFailure, Error, Option, Path, PathBuf, Result (+2 more)

### Community 14 - "EpubResourceDeclaration"
Cohesion: 0.13
Nodes (18): DocError, acquires_complete_payload_free_epub_declarations(), EpubDeclarationError, EpubResourceDeclaration, Display, Error, Formatter, Into (+10 more)

### Community 15 - "conversion_policy_cli.rs"
Cohesion: 0.32
Nodes (11): Output, matching_jpeg_is_preserved_when_quality_is_implicit(), matching_jpeg_is_reencoded_when_quality_is_explicit(), Option, Path, PathBuf, Vec, run_jpeg_conversion() (+3 more)

### Community 16 - "Incremental Update Flow"
Cohesion: 0.20
Nodes (10): Supported URL Types, URL Ingestion, Verbatim Source Identity, Changed Subset and Full Corpus, Deletion Pruning, Directed Update Parity, Graph Diff, Incremental Update Flow (+2 more)

### Community 17 - "Full Graphify Pipeline"
Cohesion: 0.20
Nodes (10): Graph Database Exports, MCP Graph Server, Optional Graph Exports, Token Reduction Benchmark, Cross-Repository Graph Merge, GitHub Clone Flow, Monorepo Isolated Extraction, Repository Origin Attribute (+2 more)

### Community 18 - "docx.rs"
Cohesion: 0.44
Nodes (8): preserves_zip_order_for_numbered_outputs(), process_file(), returns_extension_fallback_warning_fact(), ImageWriteOutcome, Path, PathBuf, temp_test_dir(), write_extension_fallback_docx()

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

### Community 29 - "produced_outcome"
Cohesion: 0.16
Nodes (17): assert_terminal_observation_finishes_extraction(), combined_conversion_and_gif_summary_uses_semantic_outcome(), conversion_summary_reports_preserved_matching_source_as_unconverted(), ConversionTarget, ConversionTargetArg, default_output_summary_preserves_existing_wording(), epub_cover_fallback_summary_reports_normal_images(), epub_filter_description() (+9 more)

### Community 30 - "RecordingTerm"
Cohesion: 0.20
Nodes (7): Arc, Mutex, FilesystemIndicatifObserver, RecordingTerm, Result, TerminalActivity, TermLike

### Community 31 - "IndicatifRunObserver"
Cohesion: 0.30
Nodes (7): F, ProgressBar, ProgressStyle, create_progress_style(), create_spinner_style(), IndicatifRunObserver, Option

### Community 32 - "Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?"
Cohesion: 0.40
Nodes (4): Answer, Outcome, Q: How should GitHub issue #22 be decomposed into dependency-aware vertical tracer-bullet tickets?, Source Nodes

## Knowledge Gaps
- **34 isolated node(s):** `SessionBrand<'session>`, `DocumentSelectionObservationAdapter<'observer, Observer>`, `ImageWriteRequest<'a>`, `RequiredCoverWriteRequest<'a>`, `Answer` (+29 more)
  These have ≤1 connection - possible missing edges or undocumented components.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ImageFormat` connect `ImageFormat` to `image_write_pipeline.rs`, `extraction_run.rs`, `conversion.rs`, `epub.rs`, `extraction_run_intake.rs`, `emission.rs`?**
  _High betweenness centrality (0.145) - this node is a cross-community bridge._
- **Why does `EpubDeclarations` connect `document_selection.rs` to `epub.rs`, `EpubResourceDeclaration`?**
  _High betweenness centrality (0.062) - this node is a cross-community bridge._
- **Why does `ArchiveImageSource` connect `ImageFormat` to `image_write_pipeline.rs`?**
  _High betweenness centrality (0.030) - this node is a cross-community bridge._
- **Are the 4 inferred relationships involving `select_documents()` (e.g. with `retained_epub_declarations_are_authoritative_during_extraction()` and `select_one_document()`) actually correct?**
  _`select_documents()` has 4 INFERRED edges - model-reasoned connections that need verification._
- **What connects `SessionBrand<'session>`, `DocumentSelectionObservationAdapter<'observer, Observer>`, `ImageWriteRequest<'a>` to the rest of the system?**
  _34 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `image_write_pipeline.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.06781326781326781 - nodes in this community are weakly interconnected._
- **Should `resource_archive.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.0907103825136612 - nodes in this community are weakly interconnected._