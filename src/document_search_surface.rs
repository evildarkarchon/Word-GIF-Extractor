//! The Document search surface — what Document discovery can observe about the world it searches.
//!
//! This seam sits between Document discovery and the world, so discovery can be
//! tested against a declared surface with no disk behind it; ADR-0031 keeps the
//! seam and this vocabulary. The vocabulary below is owned by this crate rather
//! than borrowed from `walkdir` and `std::fs` for a mechanical reason: `walkdir::Error`, `std::fs::Metadata`
//! and `std::fs::DirEntry` have no public constructors, so a surface speaking
//! those types could never be implemented by anything but the real filesystem.

use std::cmp::Ordering;
use std::ffi::OsStr;
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
    ///
    /// The real traversal encounters each directory's contents in
    /// [`file_name_order`], after any failure to read one of its entries (ADR-0031).
    fn next_entry(&mut self) -> Option<Result<TraversedEntry, TraversalFailure>>;

    /// Abandons the remaining contents of the directory most recently yielded.
    fn skip_current_dir(&mut self);
}

/// What Document discovery can observe about the world it searches.
///
/// Every observation may instead report a failure, and a failure to observe a
/// genuinely absent path is distinguishable from every other failure by its
/// [`io::ErrorKind::NotFound`] kind. Classifying a requested input is *not* on
/// this surface: that branch stays in Document discovery so it is tested
/// against every implementation rather than reimplemented by each.
pub(crate) trait DocumentSearchSurface {
    /// Reports what one path is, following links.
    fn inspect(&self, path: &Path) -> io::Result<InspectedKind>;

    /// Reports what one path is without following links.
    ///
    /// Used only after [`Self::inspect`] reports `NotFound`, so that a link whose
    /// target is gone stays distinct from a path that is not there at all.
    fn inspect_without_following(&self, path: &Path) -> io::Result<InspectedKind>;

    /// Lists what one directory directly contains.
    ///
    /// The outer failure is a failure to open the directory; an inner failure is a
    /// failure to read one of its entries, and the entry it belonged to is unknown.
    /// The real surface yields entry failures first and then the entries in
    /// [`file_name_order`], the same order its traversal uses (ADR-0031).
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
        // Collected rather than streamed: the operating system's order is not
        // stable across filesystems, and Image file emission and deduplication
        // both decide by first come, so an unsorted listing would let the
        // filesystem pick which document keeps the unsuffixed names (ADR-0031).
        let mut entries: Vec<io::Result<PathBuf>> = std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect();
        // Failures first, as `walkdir` orders them when it sorts, so the flat
        // listing and the recursive traversal agree. `sort_by` is stable, so
        // failures keep their relative order.
        entries.sort_by(|a, b| match (a, b) {
            (Ok(a), Ok(b)) => path_name_order(a, b),
            (Err(_), Err(_)) => Ordering::Equal,
            (Ok(_), Err(_)) => Ordering::Greater,
            (Err(_), Ok(_)) => Ordering::Less,
        });
        Ok(Box::new(entries.into_iter()))
    }

    fn traverse<'surface>(
        &'surface self,
        root: &Path,
    ) -> Box<dyn DocumentSearchTraversal + 'surface> {
        Box::new(WalkDirTraversal {
            traversal: WalkDir::new(root)
                .min_depth(1)
                .sort_by(|a, b| file_name_order(a.file_name(), b.file_name()))
                .into_iter(),
        })
    }
}

/// Orders two file names the way Document discovery encounters them (ADR-0031).
///
/// Names compare case-insensitively, by their uppercase form, which is how NTFS
/// lists a directory, so output on Windows stays where it was. Names that fold
/// to the same text then compare by their exact bytes, so the order is total and
/// does not depend on which filesystem produced the listing. A name that is not
/// valid Unicode folds lossily and is still told apart by the byte comparison.
fn file_name_order(a: &OsStr, b: &OsStr) -> Ordering {
    let folded = |name: &OsStr| {
        name.to_string_lossy()
            .chars()
            .flat_map(char::to_uppercase)
            .collect::<String>()
    };
    folded(a).cmp(&folded(b)).then_with(|| a.cmp(b))
}

/// Orders two entries of one directory by [`file_name_order`].
///
/// A path with no final component cannot come out of `read_dir`; if one ever
/// did, it compares as an empty name rather than panicking.
fn path_name_order(a: &Path, b: &Path) -> Ordering {
    file_name_order(
        a.file_name().unwrap_or_default(),
        b.file_name().unwrap_or_default(),
    )
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

#[cfg(test)]
mod tests;
