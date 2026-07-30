# Graph Report - .  (2026-07-30)

## Corpus Check
- cluster-only mode — file stats not available

## Summary
- 949 nodes · 2223 edges · 42 communities (39 shown, 3 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 68 edges (avg confidence: 0.81)
- Token cost: 2,391 input · 393 output

## Graph Freshness
- Built from commit: `7c9a0aa6`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- Image Write Pipeline
- Image Conversion
- Image Format Handling
- Image Write Pipeline Tests
- Document Extraction
- Document Archive Extraction
- Document Selection Management
- Archive Resource Identity
- Extraction Run Intake
- Extraction Run Observation
- Document Selection Observation
- EPUB Selection Tests
- Extraction Run Tests
- Document Selection Logic
- File Emission
- Document Extraction Tests
- EPUB Declaration Errors
- Resource Archive Management
- Document Discovery
- Conversion Summary
- Progress Bar and UI
- Conversion CLI and JPEG
- Command Line Arguments
- Terminal Output Control
- Extraction Run Observers
- Document Selection Lifecycle
- DOCX Processing
- Graphify Documentation
- Document Selection Diagnostics
- EPUB Declarations Tests
- Beside File Output
- EPUB Declarations
- Document Extraction Warnings CLI
- Mislabeled DOCX Handling
- Pre-Run Notices CLI
- Domain Documentation
- Issue Tracker Documentation
- Triage Labels Documentation
- Terminal Observation Tests

## God Nodes (most connected - your core abstractions)
1. `ImageFormat` - 38 edges
2. `select_documents()` - 37 edges
3. `temp_test_dir()` - 30 edges
4. `ArchiveImageSource` - 26 edges
5. `select_epub()` - 25 edges
6. `temp_test_dir()` - 24 edges
7. `temp_test_dir()` - 23 edges
8. `convert_image()` - 21 edges
9. `write_sources()` - 21 edges
10. `write_epub_fixture()` - 20 edges

## Surprising Connections (you probably didn't know these)
- `Document Archive Extraction Flow` --semantically_similar_to--> `Extraction run`  [INFERRED] [semantically similar]
  AGENTS.md → CONTEXT.md
- `EPUB Declaration Acquisition` --semantically_similar_to--> `EPUB declarations`  [INFERRED] [semantically similar]
  docs/adr/0001-reopen-epub-for-incremental-resource-reads.md → CONTEXT.md
- `Word Image Extractor` --semantically_similar_to--> `Word Image Extractor CLI`  [INFERRED] [semantically similar]
  README.md → AGENTS.md
- `Metadata-Based EPUB Naming` --semantically_similar_to--> `EPUB Metadata Naming`  [INFERRED] [semantically similar]
  plans/epub-support-plan.md → README.md
- `Image Conversion Behavior` --semantically_similar_to--> `Conversion policy`  [INFERRED] [semantically similar]
  README.md → CONTEXT.md

## Import Cycles
- 2-file cycle: `src/extraction_run_intake.rs -> src/main.rs -> src/extraction_run_intake.rs`
- 2-file cycle: `src/document_selection.rs -> src/document_selection/progress.rs -> src/document_selection.rs`

## Hyperedges (group relationships)
- **Extraction Flow Concepts** — context_extraction_run, context_extraction_run_request, context_document_selection, context_document_extraction, context_extraction_run_outcome [INFERRED 0.75]
- **Image Pipeline Concepts** — context_image_write_pipeline, context_archive_image_discovery, context_image_file_emission, context_image_write_policy, context_image_write_purpose [INFERRED 0.75]
- **EPUB Archive Model** — context_epub_cover_extraction, context_epub_resource_archive, context_archive_resource_identity, context_epub_declarations [INFERRED 0.75]

## Communities (42 total, 3 thin omitted)

### Community 0 - "Image Write Pipeline"
Cohesion: 0.08
Nodes (37): AcceptedImage, ArchiveImageVisitor, ArchiveImageVisitor<'policy, 'request>, ImageFileEmission, emit_prepared_image(), ImageWriteCounts, ImageWriteFailure, ImageWritePolicy (+29 more)

### Community 1 - "Image Conversion"
Cohesion: 0.07
Nodes (55): DynamicImage, CodecTarget, composite_on_white(), ConversionMode, ConversionOutcome, ConversionPolicy, ConversionPolicyError, ConversionRequest (+47 more)

### Community 2 - "Image Format Handling"
Cohesion: 0.07
Nodes (35): Action, ImageFormat, HashSet, Option, ArchiveImageDiscoveryOutcome, ArchiveImageSource, discover_image(), DiscoveredImage (+27 more)

### Community 3 - "Image Write Pipeline Tests"
Cohesion: 0.11
Nodes (48): Cursor, accepted_source_reuses_evidence_prefix_and_completes_payload_incrementally(), AssertOutputBeforeTailReader, bom_prefixed_svg_at_end_of_evidence_window_is_discovered(), bom_prefixed_svg_beyond_evidence_window_is_not_discovered(), concurrent_image_emissions_preserve_every_payload(), earlier_images_are_emitted_before_third_payload_is_fully_read(), eligible_extension_outranks_mime_and_emits_fallback_warning() (+40 more)

### Community 4 - "Document Extraction"
Cohesion: 0.06
Nodes (31): DocumentExtraction, DocumentExtractionError, DocumentExtractionFacts, DocumentExtractionOutcome, DocumentExtractionPolicy, DocumentExtractionWarning, Display, Error (+23 more)

### Community 5 - "Document Archive Extraction"
Cohesion: 0.06
Nodes (45): Document Archive Extraction Flow, Graphify Workflow, Word Image Extractor CLI, Archive image discovery, Archive resource identity, Conversion policy, Document discovery, Document extraction (+37 more)

### Community 6 - "Document Selection Management"
Cohesion: 0.13
Nodes (44): format_epub_base_name(), select_documents(), create_directory_link(), create_file_symlink(), declaration_deduplication_falls_back_to_filename_when_declarations_cannot_be_read(), remove_directory_link(), remove_file_symlink(), resolves_output_dir_absolute_input() (+36 more)

### Community 7 - "Archive Resource Identity"
Cohesion: 0.09
Nodes (33): PhantomData, archive_path(), ArchiveResourceIdentity, CatalogSeed, EpubResource, EpubResource<'session>, EpubResourceArchive, EpubResourceArchiveSession (+25 more)

### Community 8 - "Extraction Run Intake"
Cohesion: 0.09
Nodes (36): ExtractionRunIntakeError, prepare(), PreparedExtractionRun, PreRunNotice, Display, Error, Formatter, HashSet (+28 more)

### Community 9 - "Extraction Run Observation"
Cohesion: 0.11
Nodes (22): NonZeroUsize, Observer, ConversionAggregation, ConversionFacts, DocumentSelectionObservationAdapter, DocumentSelectionObservationAdapter<'observer, Observer>, ExtractionOutputKind, ExtractionRunObservation (+14 more)

### Community 10 - "Document Selection Observation"
Cohesion: 0.09
Nodes (20): SilentDocumentSelectionObserver, DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionPhaseStatus, DocumentSelectionProgress, EpubDeduplicationCheck, EpubDeduplicationProgress, EpubFilterCheck (+12 more)

### Community 11 - "EPUB Selection Tests"
Cohesion: 0.17
Nodes (35): acquisition_failure_sources(), archive_open_failure_after_selection_is_a_fatal_extraction_error(), archive_parse_failure_after_selection_is_a_fatal_extraction_error(), corrupt_stored_payload(), cover_emission_failure_aborts_the_document(), cover_retries_precede_partial_normal_fallback_facts(), cover_retry_warnings_precede_normal_fallback_warning(), encoded_test_png() (+27 more)

### Community 12 - "Extraction Run Tests"
Cohesion: 0.20
Nodes (31): all_failed_requested_inputs_reach_one_no_documents_terminal_observation(), assert_single_terminal_observation(), create_directory_link(), epub_identity_is_consistent_across_normal_and_cover_runs(), epub_normal_fallback_is_classified_as_images(), execute(), nested_discovery_failure_precedes_later_progress_and_extraction_in_run_stream(), no_selected_documents_returns_no_documents_outcome() (+23 more)

### Community 13 - "Document Selection Logic"
Cohesion: 0.19
Nodes (23): deduplicate_epubs_by_declarations(), DocumentCandidate, DocumentSelectionOptions, epub_dedupe_key(), EpubFilter, fallback_base_name(), fallback_display_name(), filename_dedupe_key() (+15 more)

### Community 15 - "File Emission"
Cohesion: 0.16
Nodes (18): candidate_path(), complete_reserved_file(), FileCompletionError, FileCompletionStage, ImageFileEmission<'name>, Error, File, FnOnce (+10 more)

### Community 16 - "Document Extraction Tests"
Cohesion: 0.25
Nodes (22): conversion_policy(), docx_uses_normal_images_when_policy_requests_an_epub_cover(), docx_warning_bodies_keep_source_format_base_name_detail_multiplicity_and_phase_order(), epub_cover_conversion_warning_bodies_keep_format_and_lower_error_detail(), epub_cover_fallback_is_classified_as_normal_images(), epub_cover_output_is_not_classified_as_normal_images(), epub_cover_retry_warning_bodies_precede_filename_retry_and_normal_fallback(), epub_cover_warning_bodies_keep_declared_mime_and_filtered_format() (+14 more)

### Community 17 - "EPUB Declaration Errors"
Cohesion: 0.13
Nodes (12): DocError, EpubDeclarationError, EpubResourceDeclaration, Display, Error, Formatter, Into, Path (+4 more)

### Community 18 - "Resource Archive Management"
Cohesion: 0.19
Nodes (20): catalog_acquisition_is_lazy_repeatable_and_keyed_to_its_session(), consumer_failure_propagates_with_its_concrete_error_identity(), ConsumerFailure, exact_manifest_path_wins_before_percent_decoded_alias(), invalid_percent_encoded_path_is_retained_as_typed_acquisition_failure(), malformed_percent_escape_is_retained_as_typed_acquisition_failure(), mark_first_entry_encrypted(), percent_decoded_aliases_share_archive_resource_identity() (+12 more)

### Community 19 - "Document Discovery"
Cohesion: 0.21
Nodes (11): discover_documents(), RequestedInput, RequestedInputFailure, Error, Option, Path, PathBuf, Result (+3 more)

### Community 20 - "Conversion Summary"
Cohesion: 0.21
Nodes (12): combined_conversion_and_gif_summary_uses_semantic_outcome(), conversion_summary_reports_preserved_matching_source_as_unconverted(), ConversionTarget, ConversionTargetArg, default_output_summary_preserves_existing_wording(), epub_cover_fallback_summary_reports_normal_images(), final_summary_message(), gif_routing_summary_preserves_existing_wording() (+4 more)

### Community 21 - "Progress Bar and UI"
Cohesion: 0.30
Nodes (7): F, ProgressBar, ProgressStyle, create_progress_style(), create_spinner_style(), IndicatifRunObserver, Option

### Community 22 - "Conversion CLI and JPEG"
Cohesion: 0.32
Nodes (11): Output, matching_jpeg_is_preserved_when_quality_is_implicit(), matching_jpeg_is_reencoded_when_quality_is_explicit(), Option, Path, PathBuf, Vec, run_jpeg_conversion() (+3 more)

### Community 23 - "Command Line Arguments"
Cohesion: 0.40
Nodes (5): Args, document_warning_line(), epub_filter_description(), String, Vec

### Community 24 - "Terminal Output Control"
Cohesion: 0.27
Nodes (3): RecordingTerm, Result, TermLike

### Community 25 - "Extraction Run Observers"
Cohesion: 0.53
Nodes (6): Arc, Mutex, ExtractionRunObserver, FilesystemIndicatifObserver, TerminalActivity, WarningPresentationObserver

### Community 26 - "Document Selection Lifecycle"
Cohesion: 0.27
Nodes (5): R, DocumentSelectionLifecycle<'observer>, DocumentSelectionScanScope, FnOnce, ScanningProgress

### Community 27 - "DOCX Processing"
Cohesion: 0.29
Nodes (9): process_file(), ImageWriteOutcome, Path, preserves_zip_order_for_numbered_outputs(), returns_extension_fallback_warning_fact(), Path, PathBuf, temp_test_dir() (+1 more)

### Community 28 - "Graphify Documentation"
Cohesion: 0.20
Nodes (9): AGENTS.md, graphify-out/graph.json, graphify-out/GRAPH_REPORT.md, graphify explain, graphify-out, graphify path, graphify query, graphify update . (+1 more)

### Community 29 - "Document Selection Diagnostics"
Cohesion: 0.47
Nodes (9): create_directory_link(), remove_directory_link(), Path, PathBuf, temp_test_dir(), warns_for_broken_requested_link_before_no_documents_summary(), warns_once_for_broken_nested_link_during_non_recursive_discovery(), warns_once_for_broken_nested_link_during_recursive_discovery() (+1 more)

### Community 30 - "EPUB Declarations Tests"
Cohesion: 0.47
Nodes (8): acquires_complete_payload_free_epub_declarations(), Path, PathBuf, sparse_epub_declarations_are_a_successful_acquisition(), temp_test_dir(), write_epub(), write_epub_with_cover(), write_sparse_epub()

### Community 31 - "Beside File Output"
Cohesion: 0.48
Nodes (6): extracts_beside_input_when_output_omitted(), has_png_files(), Path, PathBuf, temp_test_dir(), write_minimal_docx()

### Community 32 - "EPUB Declarations"
Cohesion: 0.53
Nodes (3): EpubDeclarations, Option, Vec

### Community 33 - "Document Extraction Warnings CLI"
Cohesion: 0.53
Nodes (5): renders_document_extraction_warning_with_one_prefix_and_no_document_path(), Path, PathBuf, temp_test_dir(), write_warning_docx()

### Community 34 - "Mislabeled DOCX Handling"
Cohesion: 0.53
Nodes (5): extracts_mislabeled_png_when_filtering_for_png(), Path, PathBuf, temp_test_dir(), write_mislabeled_docx()

### Community 36 - "Pre-Run Notices CLI"
Cohesion: 0.83
Nodes (3): renders_ordered_pre_run_notices_on_existing_streams(), PathBuf, temp_test_dir()

### Community 41 - "Terminal Observation Tests"
Cohesion: 0.24
Nodes (9): assert_terminal_observation_finishes_extraction(), document_warning_presentation_adds_one_prefix_and_suspends_extraction_progress(), main(), observer_temp_test_dir(), recursive_discovery_diagnostic_suspends_active_scan_spinner(), Path, PathBuf, terminal_observer_finishes_every_nonempty_outcome_with_existing_wording() (+1 more)

## Knowledge Gaps
- **28 isolated node(s):** `SessionBrand<'session>`, `DocumentSelectionObservationAdapter<'observer, Observer>`, `ImageWriteRequest<'a>`, `RequiredCoverWriteRequest<'a>`, `Graphify Workflow` (+23 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `ImageFormat` connect `Image Format Handling` to `Image Write Pipeline`, `Image Conversion`, `Image Write Pipeline Tests`, `Extraction Run Intake`, `Extraction Run Observation`, `EPUB Selection Tests`, `File Emission`?**
  _High betweenness centrality (0.137) - this node is a cross-community bridge._
- **Why does `EpubDeclarations` connect `EPUB Declarations` to `EPUB Declaration Errors`, `Document Extraction`, `Document Selection Logic`?**
  _High betweenness centrality (0.066) - this node is a cross-community bridge._
- **Why does `select_documents()` connect `Document Selection Management` to `Document Extraction Tests`, `Document Selection Observation`, `EPUB Selection Tests`, `Document Selection Logic`?**
  _High betweenness centrality (0.059) - this node is a cross-community bridge._
- **Are the 27 inferred relationships involving `select_documents()` (e.g. with `retained_epub_declarations_are_authoritative_during_extraction()` and `select_one_document()`) actually correct?**
  _`select_documents()` has 27 INFERRED edges - model-reasoned connections that need verification._
- **What connects `SessionBrand<'session>`, `DocumentSelectionObservationAdapter<'observer, Observer>`, `ImageWriteRequest<'a>` to the rest of the system?**
  _28 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Image Write Pipeline` be split into smaller, more focused modules?**
  _Cohesion score 0.08158508158508158 - nodes in this community are weakly interconnected._
- **Should `Image Conversion` be split into smaller, more focused modules?**
  _Cohesion score 0.0706605222734255 - nodes in this community are weakly interconnected._