//! Shared fixtures for the crate's in-crate tests.
//!
//! Every temporary-directory helper, ZIP fixture builder, platform directory-link
//! helper, failing reader adapter and silent observer used by a `#[cfg(test)] mod tests`
//! sibling lives here, so that adding a test never means writing another copy of one.
//!
//! # Why this is a plain `#[cfg(test)]` module
//!
//! Unit tests and integration tests cannot share a plain module: `cfg(test)` is not
//! set when the library is compiled for an integration test, so nothing in here is
//! reachable from `tests/`. The integration side therefore has a second support module
//! of its own, `tests/support/mod.rs`, and a small deliberate overlap between the two —
//! the temporary directory helper, the DOCX builder, the directory-link pair — is
//! expected and accepted. Helpers that look duplicated across `src/` and `tests/` are
//! that overlap, not a leftover of this consolidation.
//!
//! Two ways of removing that overlap were considered and rejected:
//!
//! - A feature-gated support module with the package depending on itself would let
//!   both sides import the same code, but a package listed in its own dependencies
//!   is surprising to every future reader, and the whole benefit is removing one
//!   small duplicated helper. Not worth the confusion.
//! - A separate workspace crate holding the fixtures is the textbook answer, but it
//!   is disproportionate for a single-package repository: it adds a workspace, a
//!   manifest and a build unit to share a few hundred lines of test scaffolding.
//!
//! If either trade-off changes — the repository gains a second package, or the
//! overlap grows past a few helpers — the workspace crate becomes the better option.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

use crate::document_search_surface::{
    DocumentSearchSurface, DocumentSearchTraversal, InspectedKind, TraversalFailure, TraversedEntry,
};
use crate::epub_declarations::{EpubDeclarationError, EpubDeclarationSource, EpubDeclarations};
use crate::extraction_run_observation::{ExtractionRunObservation, ExtractionRunObserver};

/// One node in a declared Document search surface.
#[derive(Debug, Clone)]
enum SearchNode {
    File,
    Directory,
    /// A link, whose `None` target is a link whose target is gone.
    Link {
        target: Option<PathBuf>,
    },
    /// An inspectable object that is neither a file nor a directory.
    Other,
}

/// How one declared directory fails when something tries to open it.
#[derive(Debug, Clone)]
struct ListingFailure {
    kind: io::ErrorKind,
    /// Whether a traversal failure names the directory or arrives without a path.
    ///
    /// A pathless failure is what forces Document discovery's nearest-parent
    /// attribution, which no real-filesystem fixture has ever been able to stage.
    reports_path: bool,
}

/// A Document search surface backed by a declared tree rather than the filesystem.
///
/// Every case Document discovery branches on is a value here: a link whose target
/// is gone, a directory that cannot be opened, and an entry that enumerated as a
/// directory but no longer inspects as one. Nothing touches disk, so no test using
/// this surface needs a temporary directory, a symlink, or a teardown.
pub(crate) struct InMemorySearchSurface {
    nodes: BTreeMap<PathBuf, SearchNode>,
    listing_failures: BTreeMap<PathBuf, ListingFailure>,
    unreadable_entries: BTreeMap<PathBuf, io::ErrorKind>,
    inspection_overrides: BTreeMap<PathBuf, Result<InspectedKind, io::ErrorKind>>,
}

impl InMemorySearchSurface {
    /// Creates a surface on which nothing exists.
    pub(crate) fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            listing_failures: BTreeMap::new(),
            unreadable_entries: BTreeMap::new(),
            inspection_overrides: BTreeMap::new(),
        }
    }

    /// Declares one regular file.
    pub(crate) fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.nodes.insert(path.into(), SearchNode::File);
        self
    }

    /// Declares one directory.
    pub(crate) fn with_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.nodes.insert(path.into(), SearchNode::Directory);
        self
    }

    /// Declares one link, or a link whose target is gone when `target` is `None`.
    pub(crate) fn with_link(mut self, path: impl Into<PathBuf>, target: Option<&str>) -> Self {
        self.nodes.insert(
            path.into(),
            SearchNode::Link {
                target: target.map(PathBuf::from),
            },
        );
        self
    }

    /// Declares one inspectable object that is neither a file nor a directory.
    pub(crate) fn with_other(mut self, path: impl Into<PathBuf>) -> Self {
        self.nodes.insert(path.into(), SearchNode::Other);
        self
    }

    /// Declares a directory that classifies normally but cannot be opened.
    ///
    /// This is the shape a directory removed between classification and opening
    /// leaves behind, which the deleted observer-as-filesystem hook used to stage
    /// by removing a real directory from inside a progress callback.
    pub(crate) fn with_listing_failure(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.nodes.insert(path.clone(), SearchNode::Directory);
        self.listing_failures.insert(
            path,
            ListingFailure {
                kind: io::ErrorKind::NotFound,
                reports_path: true,
            },
        );
        self
    }

    /// Declares a directory that opens but whose listing fails on one entry.
    ///
    /// Opening a directory and reading one of its entries are separate operations
    /// that fail separately, which is why [`DocumentSearchSurface::read_directory`]
    /// returns a fallible iterator of fallible items. Document discovery reports a
    /// per-entry failure against the directory and keeps reading the rest; without
    /// this, only a real filesystem could produce that condition.
    ///
    /// The failing entry is not a node. An entry that never materialises has no
    /// path to inspect — the failure is the only thing observable about it.
    pub(crate) fn with_unreadable_entry(mut self, directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        self.nodes.insert(directory.clone(), SearchNode::Directory);
        self.unreadable_entries
            .insert(directory, io::ErrorKind::PermissionDenied);
        self
    }

    /// Declares a directory whose traversal failure arrives without a path.
    pub(crate) fn with_pathless_listing_failure(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.nodes.insert(path.clone(), SearchNode::Directory);
        self.listing_failures.insert(
            path,
            ListingFailure {
                kind: io::ErrorKind::PermissionDenied,
                reports_path: false,
            },
        );
        self
    }

    /// Declares a directory that a traversal enumerates but inspection reports as `kind`.
    pub(crate) fn with_stale_directory(
        mut self,
        path: impl Into<PathBuf>,
        kind: InspectedKind,
    ) -> Self {
        let path = path.into();
        self.nodes.insert(path.clone(), SearchNode::Directory);
        self.inspection_overrides.insert(path, Ok(kind));
        self
    }

    /// Declares a directory that a traversal enumerates but inspection fails on.
    pub(crate) fn with_stale_directory_inspection_failure(
        mut self,
        path: impl Into<PathBuf>,
    ) -> Self {
        let path = path.into();
        self.nodes.insert(path.clone(), SearchNode::Directory);
        self.inspection_overrides
            .insert(path, Err(io::ErrorKind::PermissionDenied));
        self
    }

    /// Builds one failure with a non-empty message, as a real inspection would carry.
    fn failure(kind: io::ErrorKind, path: &Path) -> io::Error {
        io::Error::new(kind, format!("declared {kind:?} for {}", path.display()))
    }

    /// Resolves one path through any links, returning the node it names.
    ///
    /// Links are followed wherever they appear, not only in the last position, so
    /// a path below a link resolves the way it would on a filesystem. The bound
    /// stops a link cycle from recursing forever; a cycle resolves to a link node,
    /// which every caller reads as a path that is not usable.
    fn resolve(&self, path: &Path) -> Option<(PathBuf, SearchNode)> {
        let mut resolved = PathBuf::new();
        for component in path.components() {
            resolved.push(component);
            for _ in 0..8 {
                match self.nodes.get(&resolved) {
                    Some(SearchNode::Link { target }) => resolved = target.clone()?,
                    _ => break,
                }
            }
        }
        self.nodes
            .get(&resolved)
            .cloned()
            .map(|node| (resolved, node))
    }

    /// Returns the direct children of one directory, in a stable order.
    ///
    /// Entries are named under `display_root` rather than under the resolved
    /// directory, which is what listing a link reports: the link's own path with
    /// the target's entry names below it.
    fn children(&self, resolved_root: &Path, display_root: &Path) -> Vec<(PathBuf, SearchNode)> {
        self.nodes
            .iter()
            .filter(|(path, _)| path.parent() == Some(resolved_root))
            .filter_map(|(path, node)| {
                let name = path.file_name()?;
                Some((display_root.join(name), node.clone()))
            })
            .collect()
    }

    /// Flattens one recursive traversal into the items it yields, in encounter order.
    fn traversal_items(&self, root: &Path) -> Vec<PendingItem> {
        let mut items = Vec::new();
        match self.resolve(root) {
            Some((resolved, SearchNode::Directory)) => {
                self.traverse_into(&resolved, root, 1, &mut items);
            }
            // A traversal of anything that is not a readable directory fails at its
            // root, in the position the root itself would have occupied.
            _ => items.push(PendingItem {
                listed_directory: root.to_path_buf(),
                item: Err(TraversalFailure::new(
                    0,
                    Some(root.to_path_buf()),
                    Self::failure(io::ErrorKind::NotFound, root),
                )),
            }),
        }
        items
    }

    /// Appends one directory's entries and their descendants to a traversal.
    fn traverse_into(
        &self,
        resolved_root: &Path,
        display_root: &Path,
        depth: usize,
        items: &mut Vec<PendingItem>,
    ) {
        // Keyed on the resolved directory, because that is what a filesystem
        // opens: a listing declared unopenable still fails when it is reached
        // through a link. The failure itself is reported against the display
        // path, which is the path the traversal actually tried to open.
        if let Some(failure) = self.listing_failures.get(resolved_root) {
            // The failure takes the directory's own depth, so truncating by it
            // leaves the nearest confirmed parent at the top of the caller's stack.
            items.push(PendingItem {
                listed_directory: display_root.to_path_buf(),
                item: Err(TraversalFailure::new(
                    depth.saturating_sub(1),
                    failure.reports_path.then(|| display_root.to_path_buf()),
                    Self::failure(failure.kind, display_root),
                )),
            });
            return;
        }

        for (path, node) in self.children(resolved_root, display_root) {
            let is_directory = matches!(node, SearchNode::Directory);
            items.push(PendingItem {
                listed_directory: display_root.to_path_buf(),
                item: Ok(TraversedEntry::new(path.clone(), depth, is_directory)),
            });
            if is_directory {
                // Nested links are enumerated but never descended into, so a
                // traversal cannot widen its scope through one. A directory below
                // an already-followed root still resolves, which is why the
                // descent takes the resolved path and the display path separately.
                let resolved_child = self
                    .resolve(&path)
                    .map_or_else(|| path.clone(), |(resolved, _)| resolved);
                self.traverse_into(&resolved_child, &path, depth + 1, items);
            }
        }
    }
}

impl DocumentSearchSurface for InMemorySearchSurface {
    fn inspect(&self, path: &Path) -> io::Result<InspectedKind> {
        if let Some(override_result) = self.inspection_overrides.get(path) {
            return (*override_result).map_err(|kind| Self::failure(kind, path));
        }

        match self.resolve(path) {
            Some((_, SearchNode::File)) => Ok(InspectedKind::File),
            Some((_, SearchNode::Directory)) => Ok(InspectedKind::Directory),
            Some((_, SearchNode::Other)) => Ok(InspectedKind::Other),
            // A resolved link is never itself a node; an unresolvable one reads as absent.
            _ => Err(Self::failure(io::ErrorKind::NotFound, path)),
        }
    }

    fn inspect_without_following(&self, path: &Path) -> io::Result<InspectedKind> {
        match self.nodes.get(path) {
            Some(SearchNode::File) => Ok(InspectedKind::File),
            Some(SearchNode::Directory) => Ok(InspectedKind::Directory),
            // A link is neither a file nor a directory until it is followed.
            Some(SearchNode::Link { .. }) | Some(SearchNode::Other) => Ok(InspectedKind::Other),
            None => Err(Self::failure(io::ErrorKind::NotFound, path)),
        }
    }

    fn read_directory<'surface>(
        &'surface self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<PathBuf>> + 'surface>> {
        match self.resolve(path) {
            Some((resolved, SearchNode::Directory)) => {
                // Both declarations key on the resolved directory rather than on
                // the requested path, so a request arriving through a link fails
                // the way the filesystem would: opening the target is what fails,
                // not naming the link.
                if let Some(failure) = self.listing_failures.get(&resolved) {
                    return Err(Self::failure(failure.kind, path));
                }

                let mut entries: Vec<io::Result<PathBuf>> = self
                    .children(&resolved, path)
                    .into_iter()
                    .map(|(child, _)| Ok(child))
                    .collect();
                if let Some(kind) = self.unreadable_entries.get(&resolved) {
                    // Placed ahead of the entries that did materialise, so that a
                    // test asserting the listing continued past it has something
                    // left to find. A failing entry has no name to order it by.
                    entries.insert(0, Err(Self::failure(*kind, path)));
                }
                Ok(Box::new(entries.into_iter()))
            }
            Some(_) => Err(Self::failure(io::ErrorKind::InvalidInput, path)),
            None => Err(Self::failure(io::ErrorKind::NotFound, path)),
        }
    }

    fn traverse<'surface>(
        &'surface self,
        root: &Path,
    ) -> Box<dyn DocumentSearchTraversal + 'surface> {
        Box::new(DeclaredTraversal {
            pending: self.traversal_items(root).into(),
            last_directory: None,
        })
    }
}

/// A declaration source that answers from declared facts and records what it was asked.
///
/// The recording is what makes ADR-0002's retention rule assertable: the rule is
/// that filtering acquires once and deduplication reuses what filtering retained,
/// and until this existed the reuse was invisible to every test.
pub(crate) struct DeclaredEpubDeclarations {
    declarations: BTreeMap<PathBuf, DeclaredEpub>,
    acquisitions: RefCell<Vec<PathBuf>>,
}

/// What one declared EPUB reports when its declarations are acquired.
enum DeclaredEpub {
    Readable {
        creator: Option<String>,
        title: Option<String>,
    },
    Unreadable,
}

impl DeclaredEpubDeclarations {
    /// Creates a source that reports every EPUB as unreadable.
    pub(crate) fn new() -> Self {
        Self {
            declarations: BTreeMap::new(),
            acquisitions: RefCell::new(Vec::new()),
        }
    }

    /// Declares the creator and title one EPUB reports.
    pub(crate) fn with_declarations(
        mut self,
        path: impl Into<PathBuf>,
        creator: Option<&str>,
        title: Option<&str>,
    ) -> Self {
        self.declarations.insert(
            path.into(),
            DeclaredEpub::Readable {
                creator: creator.map(str::to_string),
                title: title.map(str::to_string),
            },
        );
        self
    }

    /// Declares one EPUB whose declarations cannot be read.
    pub(crate) fn with_unreadable(mut self, path: impl Into<PathBuf>) -> Self {
        self.declarations
            .insert(path.into(), DeclaredEpub::Unreadable);
        self
    }

    /// Returns every path this source was asked about, in the order it was asked.
    pub(crate) fn acquisitions(&self) -> Vec<PathBuf> {
        self.acquisitions.borrow().clone()
    }
}

impl EpubDeclarationSource for DeclaredEpubDeclarations {
    fn acquire(&self, path: &Path) -> Result<EpubDeclarations, EpubDeclarationError> {
        self.acquisitions.borrow_mut().push(path.to_path_buf());
        match self.declarations.get(path) {
            Some(DeclaredEpub::Readable { creator, title }) => Ok(EpubDeclarations::declared(
                creator.as_deref(),
                title.as_deref(),
            )),
            // An undeclared path reads as an EPUB that will not parse, which is
            // what an unreadable file on disk produces.
            _ => Err(EpubDeclarationError::unreadable()),
        }
    }
}

/// One flattened traversal item together with the directory whose listing produced it.
///
/// The producing directory is what makes `skip_current_dir` exact. A failure can
/// arrive without a path, and a predicate reading only the item's own path has
/// nothing to test such a failure against — so it would keep it, and report a
/// branch a real traversal skipped without ever opening. Every item belongs to a
/// listing whether or not it can name itself, which is the fact worth recording.
struct PendingItem {
    listed_directory: PathBuf,
    item: Result<TraversedEntry, TraversalFailure>,
}

/// One traversal of a declared tree, flattened ahead of time.
struct DeclaredTraversal {
    pending: std::collections::VecDeque<PendingItem>,
    last_directory: Option<PathBuf>,
}

impl DocumentSearchTraversal for DeclaredTraversal {
    fn next_entry(&mut self) -> Option<Result<TraversedEntry, TraversalFailure>> {
        let pending = self.pending.pop_front()?;
        self.last_directory = match &pending.item {
            Ok(entry) if entry.is_directory() => Some(entry.path().to_path_buf()),
            _ => None,
        };
        Some(pending.item)
    }

    fn skip_current_dir(&mut self) {
        let Some(directory) = self.last_directory.take() else {
            return;
        };
        // Dropping by producing directory rather than by item path drops the whole
        // abandoned branch, including the descendant listings under it.
        self.pending
            .retain(|pending| !pending.listed_directory.starts_with(&directory));
    }
}

/// Returns an unused temporary directory path for one test.
///
/// `area` names the module under test and only makes the path readable when a
/// failing test leaves its directory behind; the process id and nanosecond stamp
/// are what actually keep concurrent tests from colliding. The directory is not
/// created — callers that need it on disk create it themselves.
///
/// # `!path.exists()` is not evidence of non-emission
///
/// Because nothing here touches the filesystem, the returned path is absent the
/// moment it is handed back. Asserting that it is still absent therefore passes
/// whether the code under test rejected its input, produced nothing, failed
/// partway, or never ran at all — it is a property of this function, not of the
/// subject. Prove non-emission from the subject's own return value instead: a
/// count that stayed at zero, or an outcome that says nothing was written.
///
/// The assertion is only meaningful once something is known to have created a
/// directory here, which is why a test that writes to one subdirectory may
/// legitimately assert a sibling was never created.
pub(crate) fn temp_test_dir(area: &str, test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-{area}-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

/// Returns an unused temporary `.epub` file path for one test.
///
/// `area` means what it does in [`temp_test_dir`]. Used by tests that want a single
/// archive file rather than a directory to fill.
pub(crate) fn temp_epub_path(area: &str, test_name: &str) -> PathBuf {
    // Appended rather than set with `with_extension`, which would truncate at the last
    // `.` — an `area` or `test_name` containing one would silently eat the stamp that
    // makes the path unique.
    let path = temp_test_dir(area, test_name);
    let mut file_name = path
        .file_name()
        .expect("generated temporary path should have a file name")
        .to_os_string();
    file_name.push(".epub");
    path.with_file_name(file_name)
}

/// Creates a directory link used to exercise the platform filesystem through selection.
#[cfg(unix)]
pub(crate) fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("test directory symlink should be creatable");
}

/// Creates a directory link without requiring Windows symbolic-link privileges.
#[cfg(windows)]
pub(crate) fn create_directory_link(target: &Path, link: &Path) {
    if std::os::windows::fs::symlink_dir(target, link).is_ok() {
        return;
    }

    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("Windows junction command should run");
    assert!(
        output.status.success(),
        "test directory link should be creatable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Removes a directory link without following it into its target.
#[cfg(unix)]
pub(crate) fn remove_directory_link(link: &Path) {
    fs::remove_file(link).expect("test directory symlink should be removable");
}

/// Removes a Windows directory symlink or junction without following it.
#[cfg(windows)]
pub(crate) fn remove_directory_link(link: &Path) {
    fs::remove_dir(link).expect("test directory link should be removable");
}

/// Creates a file symlink used to preserve nested supported-file eligibility.
#[cfg(unix)]
pub(crate) fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("test file symlink should be creatable");
    true
}

/// Attempts to create a genuine Windows file symlink when the host permits it.
///
/// Returns whether the link was created, because unprivileged Windows hosts cannot
/// create one and there is no junction equivalent for files.
#[cfg(windows)]
pub(crate) fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

/// Removes a file symlink without removing its target.
pub(crate) fn remove_file_symlink(link: &Path) {
    fs::remove_file(link).expect("test file symlink should be removable");
}

/// A reader that serves a buffered payload and then fails partway through it.
///
/// Archive image discovery reads a source in two phases, so a source that fails
/// only in the second phase is the fixture that separates a bounded evidence read
/// from a failed acquisition. Both the discovery tests and the pipeline tests need
/// one, which is what puts it here rather than in either module.
pub(crate) struct FailAfterReader {
    cursor: Cursor<Vec<u8>>,
    fail_at: u64,
}

impl FailAfterReader {
    /// Creates a reader that reports an error after the requested byte offset.
    pub(crate) fn new(data: Vec<u8>, fail_at: u64) -> Self {
        Self {
            cursor: Cursor::new(data),
            fail_at,
        }
    }
}

impl Read for FailAfterReader {
    /// Reads through `fail_at`, then returns the injected acquisition failure.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let position = self.cursor.position();
        if position >= self.fail_at {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "injected archive resource failure",
            ));
        }

        let remaining = (self.fail_at - position) as usize;
        let read_len = buffer.len().min(remaining);
        self.cursor.read(&mut buffer[..read_len])
    }
}

/// Ignores the live run timeline, for tests that assert files and semantic outcomes.
///
/// Document selection reports into the same stream as the rest of the run, so
/// this one observer also serves tests whose seam is `select_documents`.
#[derive(Default)]
pub(crate) struct SilentExtractionRunObserver;

impl ExtractionRunObserver for SilentExtractionRunObserver {
    /// Ignores live observations because the calling test asserts the returned outcome.
    fn on_observation(&mut self, _observation: ExtractionRunObservation) {}
}

/// Records every live fact observed through the production run seam.
#[derive(Default)]
pub(crate) struct RecordingRunObserver {
    pub(crate) observations: Vec<ExtractionRunObservation>,
}

impl ExtractionRunObserver for RecordingRunObserver {
    /// Records the complete ordered live Extraction run timeline.
    fn on_observation(&mut self, observation: ExtractionRunObservation) {
        self.observations.push(observation);
    }
}

/// Which part of a run one observation reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationKind {
    SelectionProgress,
    SelectionDiagnostic,
    Extraction,
}

/// Classifies one observation for the filtered views below.
///
/// The match is exhaustive on purpose: a new observation variant fails to
/// compile here rather than being silently dropped from every filtered view,
/// which would quietly weaken the `is_empty` assertions that use them.
fn observation_kind(observation: &ExtractionRunObservation) -> ObservationKind {
    match observation {
        ExtractionRunObservation::DiscoveringDocuments { .. }
        | ExtractionRunObservation::DocumentDiscoveryFinished { .. }
        | ExtractionRunObservation::FilteringEpubs { .. }
        | ExtractionRunObservation::EpubFilteringFinished { .. }
        | ExtractionRunObservation::DeduplicatingEpubs { .. }
        | ExtractionRunObservation::EpubDeduplicationFinished { .. } => {
            ObservationKind::SelectionProgress
        }
        ExtractionRunObservation::MissingInput { .. }
        | ExtractionRunObservation::SkippedNonEpubInput { .. }
        | ExtractionRunObservation::DocumentDiscoveryFailed { .. }
        | ExtractionRunObservation::UnreadableEpubDeclarations { .. } => {
            ObservationKind::SelectionDiagnostic
        }
        ExtractionRunObservation::ExtractionStarted { .. }
        | ExtractionRunObservation::DocumentStarted { .. }
        | ExtractionRunObservation::DocumentWarning { .. }
        | ExtractionRunObservation::DocumentError { .. }
        | ExtractionRunObservation::DocumentFinished { .. }
        | ExtractionRunObservation::Terminal(_) => ObservationKind::Extraction,
    }
}

impl RecordingRunObserver {
    /// Returns the observations of one kind, in recorded order.
    fn of_kind(&self, kind: ObservationKind) -> Vec<ExtractionRunObservation> {
        self.observations
            .iter()
            .filter(|observation| observation_kind(observation) == kind)
            .cloned()
            .collect()
    }

    /// Returns the Document selection phase observations, in recorded order.
    ///
    /// Selection progress and selection diagnostics interleave in one stream, so
    /// tests that assert one without the other filter rather than read a second
    /// collection. Assert against `observations` when the interleaving matters.
    pub(crate) fn selection_progress(&self) -> Vec<ExtractionRunObservation> {
        self.of_kind(ObservationKind::SelectionProgress)
    }

    /// Returns the Document selection diagnostics, in recorded order.
    pub(crate) fn selection_diagnostics(&self) -> Vec<ExtractionRunObservation> {
        self.of_kind(ObservationKind::SelectionDiagnostic)
    }
}

/// Writes a ZIP archive containing the supplied entries in order, with the given options.
///
/// Entry order is preserved because several tests assert output numbering follows
/// archive order.
fn write_zip(path: &Path, options: SimpleFileOptions, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("test archive should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    for (name, data) in entries {
        zip.start_file(*name, options)
            .expect("ZIP entry should start");
        zip.write_all(data)
            .expect("ZIP entry payload should be writable");
    }
    zip.finish().expect("test archive should finish");
}

/// Writes a ZIP archive containing the supplied entries in order.
pub(crate) fn write_zip_archive(path: &Path, entries: &[(&str, &[u8])]) {
    write_zip(path, SimpleFileOptions::default(), entries);
}

/// Returns options that store payloads uncompressed.
///
/// Tests that corrupt one payload by searching the archive bytes need stored entries,
/// because a deflated payload does not appear verbatim in the file.
fn stored_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
}

/// Writes a DOCX fixture containing the supplied archive entries in order.
///
/// A DOCX fixture is nothing but a ZIP, so this delegates rather than adding steps. It
/// is kept as a separate name on purpose: DOCX tests read as building a document, and
/// the day a fixture needs real DOCX scaffolding this is where it goes.
pub(crate) fn write_docx(path: &Path, entries: &[(&str, &[u8])]) {
    write_zip_archive(path, entries);
}

/// Writes a DOCX whose single entry has an image extension but no matching magic bytes.
pub(crate) fn write_extension_fallback_docx(path: &Path) {
    write_docx(path, &[("word/media/image1.png", b"not actually a png")]);
}

/// The EPUB container declaration pointing at the package document every fixture writes.
const EPUB_CONTAINER_XML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

/// The navigation document written whenever a fixture declares a navigation item.
///
/// Its body is deliberately empty, and no test asserts anything about it: navigation is
/// declared as `application/xhtml+xml`, so extraction never treats it as an image
/// resource. Only its presence matters, so the declared manifest item resolves to a
/// real archive entry.
const EPUB_NAV_XHTML: &[u8] = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body/></html>";

/// Whether a package document declares a navigation item and references it from the spine.
enum EpubNavigation {
    /// The package declares `nav.xhtml` and the spine references it.
    Declared,
    /// The package declares no navigation document and its spine is empty.
    Absent,
}

/// Renders manifest item declarations from `(id, href, media type, properties)` tuples.
fn epub_manifest_items(resources: &[(&str, &str, &str, Option<&str>)]) -> String {
    resources
        .iter()
        .map(|(id, href, mime, properties)| {
            let properties = properties
                .map(|value| format!(" properties=\"{value}\""))
                .unwrap_or_default();
            format!("    <item id=\"{id}\" href=\"{href}\" media-type=\"{mime}\"{properties}/>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Renders one EPUB package document from its descriptive and manifest declarations.
///
/// [`EpubNavigation::Declared`] adds both the navigation manifest item and its spine
/// reference together, since a declared navigation document is only valid when the
/// spine references it.
fn epub_package(
    title: &str,
    creator: Option<&str>,
    navigation: EpubNavigation,
    manifest_items: &str,
) -> String {
    let creator = creator
        .map(|creator| format!("\n    <dc:creator>{creator}</dc:creator>"))
        .unwrap_or_default();
    let (nav_item, spine) = match navigation {
        EpubNavigation::Declared => (
            "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
            "<itemref idref=\"nav\"/>",
        ),
        EpubNavigation::Absent => ("", ""),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>{title}</dc:title>{creator}
  </metadata>
  <manifest>
{nav_item}{manifest_items}
  </manifest>
  <spine>{spine}</spine>
</package>"#
    )
}

/// Writes an EPUB from one package document and the archive entries that follow it.
///
/// Every EPUB fixture in this module funnels through here, so the `mimetype` and
/// container entries are declared once and always come first — the order the format
/// requires. `options` decides compression for the whole archive.
fn write_epub_archive(
    path: &Path,
    options: SimpleFileOptions,
    package: &[u8],
    entries: &[(&str, &[u8])],
) {
    let mut all_entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", EPUB_CONTAINER_XML),
        ("OEBPS/content.opf", package),
    ];
    all_entries.extend_from_slice(entries);
    write_zip(path, options, &all_entries);
}

/// Writes an EPUB from the supplied package declarations and payload entries.
///
/// Takes the package document verbatim so that declaration-level tests can write
/// packages a higher-level builder would not produce.
pub(crate) fn write_epub_package(path: &Path, package: &[u8], resources: &[(&str, &[u8])]) {
    write_epub_archive(path, SimpleFileOptions::default(), package, resources);
}

/// Writes a navigable EPUB declaring one image resource, with its payload present.
pub(crate) fn write_epub_with_one_image(
    path: &Path,
    image_href: &str,
    image_mime: &str,
    image_data: &[u8],
) {
    let image_path = format!("OEBPS/{image_href}");
    write_navigable_epub(
        path,
        SimpleFileOptions::default(),
        "Magic Test",
        Some("Tester"),
        &[("img", image_href, image_mime, None)],
        &[(image_path.as_str(), image_data)],
    );
}

/// Writes an EPUB whose manifest declarations and ZIP payloads vary independently.
///
/// Nothing forces the two to agree, which is what lets tests build an EPUB declaring
/// a resource the archive does not contain. The package declares the title `Test` and
/// no creator.
pub(crate) fn write_epub_fixture(
    path: &Path,
    manifest_resources: &[(&str, &str, &str, Option<&str>)],
    archive_resources: &[(&str, &[u8])],
) {
    write_navigable_epub(
        path,
        SimpleFileOptions::default(),
        "Test",
        None,
        manifest_resources,
        archive_resources,
    );
}

/// Writes [`write_epub_fixture`]'s shape with uncompressed payloads and named declarations.
///
/// Two things differ from [`write_epub_fixture`], and tests depend on both: payloads are
/// stored rather than deflated, so a test can locate and corrupt one inside the archive
/// bytes; and the package declares the title `Magic Test` and creator `Tester`, which is
/// what the resulting output file names are built from.
pub(crate) fn write_stored_epub_fixture(
    path: &Path,
    manifest_resources: &[(&str, &str, &str, Option<&str>)],
    archive_resources: &[(&str, &[u8])],
) {
    write_navigable_epub(
        path,
        stored_options(),
        "Magic Test",
        Some("Tester"),
        manifest_resources,
        archive_resources,
    );
}

/// Writes an EPUB that declares a navigation document alongside its resources.
fn write_navigable_epub(
    path: &Path,
    options: SimpleFileOptions,
    title: &str,
    creator: Option<&str>,
    manifest_resources: &[(&str, &str, &str, Option<&str>)],
    archive_resources: &[(&str, &[u8])],
) {
    let package = epub_package(
        title,
        creator,
        EpubNavigation::Declared,
        &epub_manifest_items(manifest_resources),
    );
    let mut entries: Vec<(&str, &[u8])> = vec![("OEBPS/nav.xhtml", EPUB_NAV_XHTML)];
    entries.extend_from_slice(archive_resources);
    write_epub_archive(path, options, package.as_bytes(), &entries);
}

/// Writes an EPUB declaring one JPEG image, with an optional manifest property.
pub(crate) fn write_epub_image(
    path: &Path,
    image_href: &str,
    properties: Option<&str>,
    data: &[u8],
) {
    let archive_path = format!("OEBPS/{image_href}");
    write_epub_with_resources(
        path,
        image_href,
        properties,
        &[(archive_path.as_str(), data)],
    );
}

/// Writes an EPUB declaring one JPEG image whose available payloads are chosen freely.
pub(crate) fn write_epub_with_resources(
    path: &Path,
    image_href: &str,
    properties: Option<&str>,
    archive_resources: &[(&str, &[u8])],
) {
    write_epub_fixture(
        path,
        &[("image", image_href, "image/jpeg", properties)],
        archive_resources,
    );
}

/// Writes an EPUB with declaration-derived identity and at most one image.
///
/// The image tuple is `(file name, payload, is cover)`; the file is declared under
/// `images/` and stored at `OEBPS/images/`. The archive declares no navigation
/// document, so it exercises the sparser package shape.
pub(crate) fn write_epub_document(
    path: &Path,
    creator: &str,
    title: &str,
    image: Option<(&str, &[u8], bool)>,
) {
    let href = image.map(|(name, _, _)| format!("images/{name}"));
    let manifest = match (&href, image) {
        (Some(href), Some((_, _, is_cover))) => epub_manifest_items(&[(
            "image",
            href.as_str(),
            "image/jpeg",
            is_cover.then_some("cover-image"),
        )]),
        _ => String::new(),
    };
    let package = epub_package(title, Some(creator), EpubNavigation::Absent, &manifest);
    let image_path = href.as_ref().map(|href| format!("OEBPS/{href}"));
    let entries: Vec<(&str, &[u8])> = match (&image_path, image) {
        (Some(image_path), Some((_, data, _))) => vec![(image_path.as_str(), data)],
        _ => Vec::new(),
    };
    write_epub_archive(
        path,
        SimpleFileOptions::default(),
        package.as_bytes(),
        &entries,
    );
}

/// Writes an EPUB with descriptive declarations and one declared cover resource.
pub(crate) fn write_epub_with_cover(path: &Path) {
    write_epub_package(
        path,
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">declarations-test</dc:identifier>
    <dc:title>Retained Title</dc:title>
    <dc:creator>Retained Creator</dc:creator>
  </metadata>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine></spine>
</package>"#,
        &[("OEBPS/cover.png", b"cover payload")],
    );
}

/// Writes a readable EPUB with no descriptive, cover, or resource declarations.
pub(crate) fn write_sparse_epub(path: &Path) {
    write_epub_package(
        path,
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">sparse-test</dc:identifier>
  </metadata>
  <manifest></manifest>
  <spine></spine>
</package>"#,
        &[],
    );
}
