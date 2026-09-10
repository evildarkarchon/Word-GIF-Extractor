//! The Document search surface — what Document discovery can observe about the world it searches.
//!
//! ADR-0008 places this seam between Document discovery and the world. The
//! vocabulary below is owned by this crate rather than borrowed from `walkdir`
//! and `std::fs` for a mechanical reason: `walkdir::Error`, `std::fs::Metadata`
//! and `std::fs::DirEntry` have no public constructors, so a surface speaking
//! those types could never be implemented by anything but the real filesystem.
//!
//! ADR-0012 uses one `search` operation for immediate children and recursive
//! discovery, backed by direct listing and WalkDir respectively. Discovery consumes
//! both scopes in one loop using descent and failure-position facts.

use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// The directory-search scope, without changing either acquisition mechanism.
pub(crate) enum SearchScope {
    ImmediateChildren,
    Recursive,
}

/// Direct-listing acquisition shared by adapters without inspecting entry kinds.
pub(crate) struct ImmediateChildren<'surface> {
    entries: Box<dyn Iterator<Item = io::Result<PathBuf>> + 'surface>,
    opening_failure: Option<SearchFailure>,
}

impl<'surface> ImmediateChildren<'surface> {
    /// Retains an opened listing or one root-attributed failure followed by exhaustion.
    pub(crate) fn new(
        root: &Path,
        entries: io::Result<Box<dyn Iterator<Item = io::Result<PathBuf>> + 'surface>>,
    ) -> Self {
        match entries {
            Ok(entries) => Self {
                entries,
                opening_failure: None,
            },
            Err(error) => Self {
                entries: Box::new(std::iter::empty()),
                opening_failure: Some(SearchFailure::new(0, Some(root.to_path_buf()), error)),
            },
        }
    }
}

impl DirectorySearch for ImmediateChildren<'_> {
    /// Yields an opening failure first, otherwise the next unclassified direct child.
    fn next_entry(&mut self) -> Option<Result<SearchEntry, SearchFailure>> {
        if let Some(failure) = self.opening_failure.take() {
            return Some(Err(failure));
        }
        self.entries.next().map(|entry| {
            entry
                .map(|path| SearchEntry::new(path, 1, false))
                .map_err(|error| SearchFailure::new(1, None, error))
        })
    }

    fn skip_current_dir(&mut self) {
        // Direct listing has no pending descent to prune.
    }
}

/// What one inspected path is, once its kind has been established.
///
/// `Other` covers every inspectable object that is neither a file nor a
/// directory; Document discovery skips those silently rather than reporting them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InspectedKind {
    File,
    Directory,
    Other,
}

/// One search entry before Document discovery inspects it.
///
/// `may_descend` records pending descent, not a fresh inspection. Immediate-child
/// listing always reports false without classifying entries; recursive search
/// retains its enumeration-time directory fact so discovery can prune stale branches.
pub(crate) struct SearchEntry {
    path: PathBuf,
    depth: usize,
    may_descend: bool,
}

impl SearchEntry {
    /// Captures an entry with its depth and whether the search may descend through it.
    pub(crate) fn new(path: PathBuf, depth: usize, may_descend: bool) -> Self {
        Self {
            path,
            depth,
            may_descend,
        }
    }

    /// Returns the entry's path without consuming it.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Consumes the entry into the path Document discovery inspects.
    pub(crate) fn into_path(self) -> PathBuf {
        self.path
    }

    /// Returns the entry's depth below the search root, counting the root as zero.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Returns whether the search may descend through this entry unless pruned.
    pub(crate) fn may_descend(&self) -> bool {
        self.may_descend
    }
}

/// A failure opening a search root or reading an entry, in encounter order.
///
/// The depth is always known even when the path is not, which is what lets
/// Document discovery attribute a pathless failure to the nearest confirmed parent.
pub(crate) struct SearchFailure {
    depth: usize,
    path: Option<PathBuf>,
    error: io::Error,
}

impl SearchFailure {
    /// Captures a search failure at its depth with a path when acquisition knows one.
    pub(crate) fn new(depth: usize, path: Option<PathBuf>, error: io::Error) -> Self {
        Self { depth, path, error }
    }

    /// Returns the depth at which search acquisition failed.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the failing path when acquisition knew one.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns the underlying failure reported to the user as a diagnostic detail.
    pub(crate) fn error(&self) -> &io::Error {
        &self.error
    }
}

/// One directory search in progress, yielding entries and failures in encounter order.
///
/// This is an object rather than an iterator because `skip_current_dir` mutates
/// the traversal's own pending work: Document discovery calls it when an entry
/// enumerated as a directory turns out not to be one, so the stale branch is
/// abandoned instead of opened a second time.
pub(crate) trait DirectorySearch {
    /// Advances the search by one entry or failure, in encounter order.
    fn next_entry(&mut self) -> Option<Result<SearchEntry, SearchFailure>>;

    /// Abandons pending descent through the most recently yielded entry.
    /// Does nothing for immediate-child searches, which never descend.
    fn skip_current_dir(&mut self);
}

/// What Document discovery can observe about the world it searches.
///
/// Every observation may instead report a failure, and a failure to observe a
/// genuinely absent path is distinguishable from every other failure by its
/// [`io::ErrorKind::NotFound`] kind. Classifying a requested input is *not* on
/// this surface: ADR-0008 keeps that branch in Document discovery so it stays
/// testable against every implementation rather than being reimplemented by each.
pub(crate) trait DocumentSearchSurface {
    /// Reports what one path is, following links.
    fn inspect(&self, path: &Path) -> io::Result<InspectedKind>;

    /// Reports what one path is without following links.
    ///
    /// Used only after [`Self::inspect`] reports `NotFound`, so that a link whose
    /// target is gone stays distinct from a path that is not there at all.
    fn inspect_without_following(&self, path: &Path) -> io::Result<InspectedKind>;

    /// Starts a search excluding the root itself, retaining acquisition order.
    /// Immediate-child opening failures arrive at depth zero with the root path;
    /// unreadable immediate entries arrive at depth one without a path. Recursive
    /// failures retain the traversal's depth and optional path.
    fn search<'surface>(
        &'surface self,
        root: &Path,
        scope: SearchScope,
    ) -> Box<dyn DirectorySearch + 'surface>;
}

/// The Document search surface backed by the real filesystem.
pub(crate) struct FilesystemSearchSurface;

impl FilesystemSearchSurface {
    /// Reduces filesystem metadata to the kind Document discovery decides from.
    fn kind_of(metadata: &std::fs::Metadata) -> InspectedKind {
        if metadata.is_file() {
            InspectedKind::File
        } else if metadata.is_dir() {
            InspectedKind::Directory
        } else {
            InspectedKind::Other
        }
    }
}

impl DocumentSearchSurface for FilesystemSearchSurface {
    fn inspect(&self, path: &Path) -> io::Result<InspectedKind> {
        std::fs::metadata(path).map(|metadata| Self::kind_of(&metadata))
    }

    fn inspect_without_following(&self, path: &Path) -> io::Result<InspectedKind> {
        std::fs::symlink_metadata(path).map(|metadata| Self::kind_of(&metadata))
    }

    /// Uses direct listing for immediate children; recursive acquisition stays on WalkDir.
    fn search<'surface>(
        &'surface self,
        root: &Path,
        scope: SearchScope,
    ) -> Box<dyn DirectorySearch + 'surface> {
        match scope {
            SearchScope::ImmediateChildren => {
                // read_dir must open the requested directory itself: shallow WalkDir
                // can silently exclude a root replaced by a file after inspection.
                let entries = std::fs::read_dir(root).map(|entries| {
                    Box::new(entries.map(|entry| entry.map(|entry| entry.path())))
                        as Box<dyn Iterator<Item = io::Result<PathBuf>>>
                });
                Box::new(ImmediateChildren::new(root, entries))
            }
            SearchScope::Recursive => Box::new(WalkDirTraversal {
                traversal: WalkDir::new(root).min_depth(1).into_iter(),
            }),
        }
    }
}

/// The recursive traversal `walkdir` performs, expressed in this crate's vocabulary.
struct WalkDirTraversal {
    traversal: walkdir::IntoIter,
}

impl DirectorySearch for WalkDirTraversal {
    fn next_entry(&mut self) -> Option<Result<SearchEntry, SearchFailure>> {
        self.traversal
            .next()
            .map(|entry_result| match entry_result {
                Ok(entry) => Ok(SearchEntry::new(
                    entry.path().to_path_buf(),
                    entry.depth(),
                    entry.file_type().is_dir(),
                )),
                Err(error) => {
                    let depth = error.depth();
                    let path = error.path().map(Path::to_path_buf);
                    // Keep walkdir's own wording rather than unwrapping to the inner
                    // io error, whose message drops the operation and path context a
                    // user needs; the kind is carried across separately so the seam
                    // still speaks one failure vocabulary.
                    let kind = error
                        .io_error()
                        .map_or(io::ErrorKind::Other, io::Error::kind);
                    let error = io::Error::new(kind, error.to_string());
                    Err(SearchFailure::new(depth, path, error))
                }
            })
    }

    fn skip_current_dir(&mut self) {
        self.traversal.skip_current_dir();
    }
}

#[cfg(test)]
mod tests;
