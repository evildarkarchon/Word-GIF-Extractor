//! Document selection for turning requested input paths into extraction work.

mod discovery;
mod document_identity;
mod progress;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::document_search_surface::DocumentSearchSurface;
use crate::epub_declarations::{EpubDeclarationSource, EpubDeclarations};
use crate::extraction_run_observation::ExtractionRunObserver;
use crate::output_placement::OutputPlacement;

use self::document_identity::{DedupeKey, DocumentIdentity, EpubFilterTerms};
use self::progress::{
    DocumentDiscoveryProgress, DocumentSelectionLifecycle, EpubDeduplicationCheck, EpubFilterCheck,
};

/// Filter criteria for selecting EPUB files by title and creator declarations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EpubFilter {
    /// Case-insensitive title substring required for an EPUB to be selected.
    pub title: Option<String>,
    /// Case-insensitive author substring required for an EPUB to be selected.
    pub author: Option<String>,
}

impl EpubFilter {
    /// Returns true if no EPUB declaration filter criteria are set.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.author.is_none()
    }
}

/// Immutable handoff produced by Document selection for one eligible document.
///
/// The authoritative variant is consumed by Document extraction exactly once.
#[derive(Debug)]
pub(crate) enum SelectedDocument {
    /// A selected DOCX document with DOCX-only extraction facts.
    Docx(SelectedDocx),
    /// A selected EPUB document with its optional declaration snapshot.
    Epub(SelectedEpub),
}

/// Opaque DOCX payload whose fields and construction belong to Document selection.
#[derive(Debug)]
pub(crate) struct SelectedDocx {
    path: PathBuf,
    placement: OutputPlacement,
    display_name: String,
}

/// Opaque EPUB payload whose fields and construction belong to Document selection.
#[derive(Debug)]
pub(crate) struct SelectedEpub {
    path: PathBuf,
    placement: OutputPlacement,
    display_name: String,
    epub_declarations: Option<EpubDeclarations>,
}

impl SelectedDocument {
    /// Returns the source path visible to the Extraction run before handoff.
    pub(crate) fn get_path(&self) -> &Path {
        match self {
            Self::Docx(document) => &document.path,
            Self::Epub(document) => &document.path,
        }
    }

    /// Returns the stable progress identity visible to the Extraction run.
    pub(crate) fn get_display_name(&self) -> &str {
        match self {
            Self::Docx(document) => &document.display_name,
            Self::Epub(document) => &document.display_name,
        }
    }
}

impl SelectedDocx {
    /// Creates a DOCX handoff after selection has established eligibility.
    fn new(path: PathBuf, placement: OutputPlacement, display_name: String) -> Self {
        Self {
            path,
            placement,
            display_name,
        }
    }

    /// Consumes the DOCX payload into the facts owned by Document extraction.
    pub(crate) fn into_extraction_parts(self) -> (PathBuf, OutputPlacement) {
        (self.path, self.placement)
    }
}

impl SelectedEpub {
    /// Creates an EPUB handoff after selection has established eligibility.
    fn new(
        path: PathBuf,
        placement: OutputPlacement,
        display_name: String,
        epub_declarations: Option<EpubDeclarations>,
    ) -> Self {
        Self {
            path,
            placement,
            display_name,
            epub_declarations,
        }
    }

    /// Transfers the EPUB handoff into the facts interpreted by the EPUB adapter.
    ///
    /// The selected identity and output placement remain fixed even when extraction must
    /// reacquire declarations because selection retained none.
    pub(crate) fn into_extraction_parts(
        self,
    ) -> (PathBuf, OutputPlacement, Option<EpubDeclarations>) {
        (self.path, self.placement, self.epub_declarations)
    }
}

/// Options used to select documents for one extraction run.
pub struct DocumentSelectionOptions<'a> {
    /// Input files or directories to inspect.
    pub inputs: &'a [PathBuf],
    /// Whether directory inputs should be traversed recursively.
    pub recursive: bool,
    /// Optional output directory shared by every selected document.
    pub output: Option<&'a Path>,
    /// EPUB title and creator filter criteria.
    pub epub_filter: &'a EpubFilter,
    /// Whether only EPUB documents are eligible for this run.
    ///
    /// Selection is told that eligibility is restricted, not why: the reason is
    /// Document extraction's business, and a plain flag keeps selection's imports
    /// answering what it depends on — the same rule ADR-0005 applies one level down.
    pub epub_only: bool,
}

/// How one candidate reached Document discovery's output.
///
/// Discovery is the only stage that knows this, so it records it rather than
/// leaving later stages to infer it. Path equality cannot answer the question:
/// naming a directory and a file inside it discovers that file twice, and the two
/// candidates are equal as paths while only one of them was named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateOrigin {
    /// The user named this exact path as an input.
    Requested,
    /// A directory search turned the path up.
    Traversed,
}

/// The document kind of a supported candidate, as its extension classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentKind {
    Docx,
    Epub,
}

/// Private pre-eligibility representation of one sighting, from Document discovery
/// until [`split_by_kind`] turns it into a typed EPUB or DOCX candidate.
///
/// Every sighting carries its origin here, whatever its kind, because collapsing
/// repeat sightings merges origins before anything knows which side will read them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentCandidate {
    kind: DocumentKind,
    /// The path as Document discovery spelled it.
    path: PathBuf,
    /// How this candidate reached Document discovery's output.
    origin: CandidateOrigin,
}

/// Private pre-eligibility DOCX, as the EPUB-only restriction and the join see it.
///
/// It keeps its [`CandidateOrigin`] because the EPUB-only restriction diagnoses
/// only a DOCX the user named (ADR-0011).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DocxCandidate {
    path: PathBuf,
    origin: CandidateOrigin,
}

/// Private pre-eligibility EPUB that the EPUB filtering and deduplication phases check.
///
/// It carries no origin: nothing on the EPUB side reads it (ADR-0019).
#[derive(Debug, Clone, PartialEq, Eq)]
struct EpubCandidate {
    path: PathBuf,
    /// The declarations selection has retained for this EPUB so far.
    ///
    /// Before deduplication, `None` means no phase has acquired them yet: filtering keeps
    /// only EPUBs it read successfully. Deduplication attempts every EPUB, so after it
    /// `None` means selection could not read them, which is what the Selected EPUB hands on.
    epub_declarations: Option<EpubDeclarations>,
}

impl DocumentCandidate {
    /// Classifies a supported path into a candidate of its document kind.
    ///
    /// Returns `None` for a path whose extension names no supported document kind.
    fn from_path(path: PathBuf, origin: CandidateOrigin) -> Option<Self> {
        let kind = match path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("docx") => DocumentKind::Docx,
            Some("epub") => DocumentKind::Epub,
            _ => return None,
        };
        Some(Self { kind, path, origin })
    }
}

/// Selects documents for extraction while reporting live progress snapshots and diagnostics.
///
/// Selection owns document discovery, EPUB declaration filtering, EPUB dedupe,
/// Document identity, and per-document output placement. Returned documents are
/// already eligible for extraction; adapters should not re-check selection
/// filters. Missing inputs, requested-root inspection failures, named inputs
/// skipped because only EPUBs are eligible
/// ([`SkippedNonEpubInput`](crate::extraction_run_observation::ExtractionRunObservation::SkippedNonEpubInput)),
/// and unreadable EPUB declarations are reported as structured, non-fatal
/// diagnostics through the informational observer. A path sighted more than once
/// is selected once, silently.
///
/// Selection reports into the Extraction run observation stream directly. The
/// observer is informational: callbacks cannot cancel selection or alter which
/// documents are returned.
///
/// Every observation of the world is made through `surface` and every EPUB
/// declaration through `declarations`. ADR-0008 keeps both as separate
/// parameters rather than fields on [`DocumentSelectionOptions`]: those are the
/// per-run policy choices, and neither a way of seeing the world nor a way of
/// reading declarations is one of them.
///
/// Substituting `declarations` cannot change ADR-0002's retention rule. This
/// function decides when a declaration is acquired and when a retained one is
/// reused; the source only answers.
pub fn select_documents(
    options: DocumentSelectionOptions<'_>,
    surface: &dyn DocumentSearchSurface,
    declarations: &dyn EpubDeclarationSource,
    observer: &mut impl ExtractionRunObserver,
) -> Vec<SelectedDocument> {
    let mut lifecycle = DocumentSelectionLifecycle::new(observer);

    let candidates =
        discovery::discover_documents(options.inputs, options.recursive, surface, &mut lifecycle);
    let candidates = collapse_repeat_sightings(candidates);
    let (epubs, docx_files) = split_by_kind(candidates);

    // The EPUB-only restriction runs before filtering opens, so its diagnostics
    // precede every EPUB phase observation, as ADR-0011 places it.
    let docx_files = if options.epub_only {
        skip_non_epub_candidates(docx_files, &mut lifecycle);
        Vec::new()
    } else {
        docx_files
    };
    let filtered = if !options.epub_filter.is_empty() {
        filter_epub_files(epubs, options.epub_filter, declarations, &mut lifecycle)
    } else {
        epubs
    };
    let deduplicated = deduplicate_epubs_by_declarations(filtered, declarations, &mut lifecycle);

    // The one join that makes the Selected documents: EPUBs first, then other
    // documents, each in encounter order. The order is kept on purpose rather than
    // inherited (ADR-0019). Image file emission claims names first come, first
    // served, so extraction order decides which of two documents sharing an Output
    // placement gets the unsuffixed names; a DOCX and an undeclared EPUB with the
    // same stem in one directory share one. Changing it would move output names.
    deduplicated
        .into_iter()
        .map(|epub| selected_epub_from_candidate(epub, options.output))
        .chain(
            docx_files
                .into_iter()
                .map(|candidate| selected_docx_from_candidate(candidate, options.output)),
        )
        .collect()
}

/// Drops every candidate whose path equals an earlier candidate's, for every document kind.
///
/// Document discovery yields one candidate per sighting, so a file named directly
/// and also reached through a requested directory, or named twice, arrives here
/// twice. ADR-0019 makes a path one candidate. "Equal" is Rust's `Path` equality,
/// component by component as discovery spelled the paths, with no canonicalisation:
/// Document identity's path key would collapse `a/report.docx` and `b/report.docx`,
/// and canonicalising would need a new Document search surface operation (ADR-0008).
/// So `./dir/x.docx` and `dir/x.docx` deliberately stay two candidates.
///
/// The earlier sighting keeps its position, and it becomes requested if a dropped
/// sighting was requested. That origin merge is what keeps ADR-0011's diagnostic
/// for a named DOCX in an EPUB-only run when the directory above it was listed first
/// and so sighted it first through a directory search.
///
/// The collapse is silent: it reports no observation and no Document selection
/// diagnostic, and it is not a progress phase. Discovery's count has already been
/// reported and still counts every sighting, which keeps it a fact about the search
/// (ADR-0011). Spelling the same request twice says something about the command
/// line, not about the user's documents, so it earns no warning.
///
/// It runs before every eligibility stage so that none of them has to recognise a
/// repeat sighting itself: the EPUB-only restriction diagnoses a named DOCX once,
/// and an EPUB reached twice enters filtering and deduplication once rather than
/// being acquired twice and counted as a duplicate of itself.
fn collapse_repeat_sightings(candidates: Vec<DocumentCandidate>) -> Vec<DocumentCandidate> {
    // Maps each kept path to its index in `kept`, so a later sighting can merge its
    // origin into the earlier one. `Path`'s hash agrees with its component equality.
    let mut kept_index: HashMap<PathBuf, usize> = HashMap::new();
    let mut kept: Vec<DocumentCandidate> = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        if let Some(&index) = kept_index.get(&candidate.path) {
            // The user named this path, whichever sighting was kept.
            if candidate.origin == CandidateOrigin::Requested {
                kept[index].origin = CandidateOrigin::Requested;
            }
        } else {
            kept_index.insert(candidate.path.clone(), kept.len());
            kept.push(candidate);
        }
    }

    kept
}

/// Splits the collapsed candidates by document kind into typed EPUB and DOCX lists.
///
/// This is the only place selection splits by kind (ADR-0019). Both lists keep
/// encounter order, and from here each stage sees only the kind it acts on: the
/// EPUB-only restriction the DOCX list, filtering and deduplication the EPUB list.
/// The EPUB side drops the [`CandidateOrigin`], which it never reads.
fn split_by_kind(candidates: Vec<DocumentCandidate>) -> (Vec<EpubCandidate>, Vec<DocxCandidate>) {
    let mut epubs = Vec::new();
    let mut docx_files = Vec::new();
    for DocumentCandidate { kind, path, origin } in candidates {
        match kind {
            DocumentKind::Epub => epubs.push(EpubCandidate {
                path,
                epub_declarations: None,
            }),
            DocumentKind::Docx => docx_files.push(DocxCandidate { path, origin }),
        }
    }
    (epubs, docx_files)
}

/// Drops every non-EPUB candidate for a run in which only EPUBs are eligible.
///
/// Eligibility is decided here rather than inside Document discovery, which
/// yields supported candidates and owns no filter. Discovery therefore still
/// counts a document it found and this stage removes it, which is what keeps the
/// discovery count a fact about the search rather than about the run's policy.
///
/// Only inputs the user named are diagnosed. A path swept up by directory
/// traversal is dropped in silence, matching how an EPUB rejected by
/// [`EpubFilter`] is dropped: reporting one line per traversal hit would make a
/// recursive run over a large tree unreadable. Which candidate was named is read
/// off the candidate's own [`CandidateOrigin`] rather than recovered by comparing
/// paths against the requested inputs: a run naming both a directory and a file
/// inside it discovers that file twice, both sightings equal to the named path.
/// [`collapse_repeat_sightings`] makes them the one candidate this stage sees and
/// merges their origins into it, so the origin, not the path, is what records that
/// the user named it.
///
/// It takes the DOCX list from [`split_by_kind`] and consumes all of it, since
/// none of it stays eligible; the EPUB list never passes through here.
fn skip_non_epub_candidates(
    candidates: Vec<DocxCandidate>,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) {
    for candidate in candidates {
        if let DocxCandidate {
            path,
            origin: CandidateOrigin::Requested,
        } = candidate
        {
            lifecycle.skipped_non_epub_input(path);
        }
    }
}

/// Filters EPUB candidates by title and creator declarations.
fn filter_epub_files(
    epub_files: Vec<EpubCandidate>,
    filter: &EpubFilter,
    declarations: &dyn EpubDeclarationSource,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<EpubCandidate> {
    let terms = EpubFilterTerms::new(filter);

    lifecycle.filtering(
        filter,
        epub_files,
        |EpubCandidate { path, .. }, diagnostics| {
            match declarations.acquire(&path) {
                Ok(declarations)
                    if DocumentIdentity::of_epub_declarations(Some(&declarations), &path)
                        .matches(&terms) =>
                {
                    EpubFilterCheck::Matched(EpubCandidate {
                        path,
                        epub_declarations: Some(declarations),
                    })
                }
                Ok(_) => EpubFilterCheck::Rejected, // File doesn't match filter, skip.
                Err(error) => {
                    // Filtering cannot accept an EPUB whose requested declarations are unreadable.
                    diagnostics.declarations_unreadable(path, error.to_string());
                    EpubFilterCheck::Rejected
                }
            }
        },
    )
}

/// Deduplicates EPUB files based on their creator and title declarations.
///
/// Keeps the first occurrence of each unique (author, title) combination.
/// EPUBs without a non-blank creator or title declaration are deduplicated by filename.
fn deduplicate_epubs_by_declarations(
    epub_files: Vec<EpubCandidate>,
    declarations: &dyn EpubDeclarationSource,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<EpubCandidate> {
    // Track the Document identity keys already seen. Keys are case-insensitive,
    // and declared keys never equal filename keys.
    let mut seen: HashSet<DedupeKey> = HashSet::new();

    lifecycle.deduplicating(
        epub_files,
        |EpubCandidate {
             path,
             epub_declarations,
         },
         diagnostics| {
            // ADR-0002: declarations retained by filtering are authoritative for the
            // run, so deduplication asks the source only when it has none. From here
            // on, `None` means selection could not read the declarations.
            let epub_declarations =
                match EpubDeclarations::retained_or_acquire(epub_declarations, &path, declarations)
                {
                    Ok(declarations) => Some(declarations),
                    Err(error) => {
                        diagnostics.declarations_unreadable(path.clone(), error.to_string());
                        None
                    }
                };

            let key = DocumentIdentity::of_epub_declarations(epub_declarations.as_ref(), &path)
                .dedupe_key();

            // Only add if we haven't seen this combination before.
            if seen.insert(key) {
                EpubDeduplicationCheck::Unique(EpubCandidate {
                    path,
                    epub_declarations,
                })
            } else {
                EpubDeduplicationCheck::Duplicate
            }
        },
    )
}

/// Builds one selected EPUB from a filtered and deduplicated candidate.
fn selected_epub_from_candidate(
    EpubCandidate {
        path,
        epub_declarations,
    }: EpubCandidate,
    global_output: Option<&Path>,
) -> SelectedDocument {
    // Selection fixes the run identity from retained declarations only;
    // extraction-time declaration retries cannot revise this fallback.
    let identity = DocumentIdentity::of_epub_declarations(epub_declarations.as_ref(), &path);
    let placement = output_placement(&path, &identity, global_output);

    SelectedDocument::Epub(SelectedEpub::new(
        path,
        placement,
        identity.display_name(),
        epub_declarations,
    ))
}

/// Builds one selected DOCX from a candidate that survived the EPUB-only restriction.
fn selected_docx_from_candidate(
    DocxCandidate { path, .. }: DocxCandidate,
    global_output: Option<&Path>,
) -> SelectedDocument {
    let identity = DocumentIdentity::of_path(&path);
    let placement = output_placement(&path, &identity, global_output);
    SelectedDocument::Docx(SelectedDocx::new(path, placement, identity.display_name()))
}

/// Builds the Output placement for one selected document, whatever its kind.
///
/// The directory comes from [`resolve_output_dir`] and the base name from the
/// document's already-fixed Document identity.
fn output_placement(
    path: &Path,
    identity: &DocumentIdentity,
    global_output: Option<&Path>,
) -> OutputPlacement {
    OutputPlacement::new(
        resolve_output_dir(path, global_output),
        identity.base_name(),
    )
}

/// Resolves the output directory for a single input file.
///
/// When `global_output` is set, all files use that directory. Otherwise images
/// are written beside the source file (its parent directory).
fn resolve_output_dir(input_path: &Path, global_output: Option<&Path>) -> PathBuf {
    match global_output {
        Some(dir) => dir.to_path_buf(),
        None => input_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

#[cfg(test)]
mod tests;
