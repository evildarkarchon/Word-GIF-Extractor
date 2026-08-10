//! The Document search surface — what Document discovery can observe about the world it searches.
//!
//! ADR-0008 places this seam between Document discovery and the world. The
//! vocabulary below is owned by this crate rather than borrowed from `walkdir`
//! and `std::fs` for a mechanical reason: `walkdir::Error`, `std::fs::Metadata`
//! and `std::fs::DirEntry` have no public constructors, so a surface speaking
//! those types could never be implemented by anything but the real filesystem.

use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

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

/// One entry yielded by a recursive traversal, before Document discovery inspects it.
///
/// `is_directory` is the traversal's own classification at enumeration time and
/// may already be stale by the time the entry is inspected; that staleness is
/// exactly what Document discovery reacts to.
pub(crate) struct TraversedEntry {
    path: PathBuf,
    depth: usize,
    is_directory: bool,
}

impl TraversedEntry {
    /// Captures one traversed entry with the depth and kind the traversal saw.
    pub(crate) fn new(path: PathBuf, depth: usize, is_directory: bool) -> Self {
        Self {
            path,
            depth,
            is_directory,
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

    /// Returns the entry's depth below the traversal root, counting the root as zero.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Returns whether the traversal classified this entry as a directory.
    pub(crate) fn is_directory(&self) -> bool {
        self.is_directory
    }
}

/// A failure encountered part-way through a recursive traversal.
///
/// The depth is always known even when the path is not, which is what lets
/// Document discovery attribute a pathless failure to the nearest confirmed parent.
pub(crate) struct TraversalFailure {
    depth: usize,
    path: Option<PathBuf>,
    error: io::Error,
}

impl TraversalFailure {
    /// Captures one traversal failure at its position in the traversal.
    pub(crate) fn new(depth: usize, path: Option<PathBuf>, error: io::Error) -> Self {
        Self { depth, path, error }
    }

    /// Returns the depth at which the traversal failed.
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the failing path when the traversal knew one.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns the underlying failure reported to the user as a diagnostic detail.
    pub(crate) fn error(&self) -> &io::Error {
        &self.error
    }
}

/// One recursive traversal in progress.
///
/// This is an object rather than an iterator because `skip_current_dir` mutates
/// the traversal's own pending work: Document discovery calls it when an entry
/// enumerated as a directory turns out not to be one, so the stale branch is
/// abandoned instead of opened a second time.
pub(crate) trait DocumentSearchTraversal {
    /// Advances the traversal by one entry or failure, in encounter order.
    fn next_entry(&mut self) -> Option<Result<TraversedEntry, TraversalFailure>>;

    /// Abandons the remaining contents of the directory most recently yielded.
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

    /// Lists what one directory directly contains, in the order the world reports.
    ///
    /// The outer failure is a failure to open the directory; an inner failure is a
    /// failure to read one of its entries, and the entry it belonged to is unknown.
    fn read_directory<'surface>(
        &'surface self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<PathBuf>> + 'surface>>;

    /// Begins a recursive traversal of one directory, excluding the root itself.
    fn traverse<'surface>(
        &'surface self,
        root: &Path,
    ) -> Box<dyn DocumentSearchTraversal + 'surface>;
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

    fn read_directory<'surface>(
        &'surface self,
        path: &Path,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<PathBuf>> + 'surface>> {
        let entries = std::fs::read_dir(path)?;
        Ok(Box::new(
            entries.map(|entry| entry.map(|entry| entry.path())),
        ))
    }

    fn traverse<'surface>(
        &'surface self,
        root: &Path,
    ) -> Box<dyn DocumentSearchTraversal + 'surface> {
        Box::new(WalkDirTraversal {
            traversal: WalkDir::new(root).min_depth(1).into_iter(),
        })
    }
}

/// The recursive traversal `walkdir` performs, expressed in this crate's vocabulary.
struct WalkDirTraversal {
    traversal: walkdir::IntoIter,
}

impl DocumentSearchTraversal for WalkDirTraversal {
    fn next_entry(&mut self) -> Option<Result<TraversedEntry, TraversalFailure>> {
        self.traversal
            .next()
            .map(|entry_result| match entry_result {
                Ok(entry) => Ok(TraversedEntry::new(
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
                    Err(TraversalFailure::new(depth, path, error))
                }
            })
    }

    fn skip_current_dir(&mut self) {
        self.traversal.skip_current_dir();
    }
}
