//! Requested-input discovery inside Document selection.

use std::io;
use std::path::{Path, PathBuf};

use crate::document_search_surface::{DocumentSearchSurface, InspectedKind};
use crate::extraction_run_observation::DocumentDiscoveryScope;

use super::{DocumentCandidate, DocumentDiscoveryProgress, DocumentSelectionLifecycle};

/// One successfully inspected requested input retained for the Document discovery phase.
enum RequestedInput {
    File(PathBuf),
    Directory(PathBuf),
}

/// A failed requested-input classification before Document discovery begins.
enum RequestedInputFailure {
    Missing,
    Inspection(io::Error),
}

impl RequestedInput {
    /// Classifies one requested path with followed inspection while preserving broken-link identity.
    ///
    /// The non-following lookup is used only after a not-found target lookup so a
    /// genuinely absent path remains distinct from a link whose target is missing.
    /// A supported file or directory returns its retained classification, an
    /// inspectable unsupported object returns `None`, and failures distinguish a
    /// genuinely missing path from every other inspection error.
    ///
    /// ADR-0008 keeps this two-step branch in front of the Document search
    /// surface rather than on it, so every implementation of the surface is
    /// exercised by the same classification rather than repeating it.
    fn classify(
        path: &Path,
        surface: &dyn DocumentSearchSurface,
    ) -> Result<Option<Self>, RequestedInputFailure> {
        match surface.inspect(path) {
            Ok(InspectedKind::File) => Ok(Some(Self::File(path.to_path_buf()))),
            Ok(InspectedKind::Directory) => Ok(Some(Self::Directory(path.to_path_buf()))),
            Ok(InspectedKind::Other) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match surface.inspect_without_following(path) {
                    Ok(_) => Err(RequestedInputFailure::Inspection(error)),
                    Err(link_error) if link_error.kind() == io::ErrorKind::NotFound => {
                        Err(RequestedInputFailure::Missing)
                    }
                    Err(link_error) => Err(RequestedInputFailure::Inspection(link_error)),
                }
            }
            Err(error) => Err(RequestedInputFailure::Inspection(error)),
        }
    }
}

/// Discovers supported document candidates from one ordered set of requested inputs.
///
/// Every requested root is classified once before discovery, so root diagnostics
/// precede the initial snapshot and independently readable inputs still continue.
/// Recursive failures retain their traversal position and nearest available path.
/// Requested directory links are followed, while nested directory links are not.
///
/// Every observation of the world is made through `surface`; this function makes
/// no assumption that the world is a filesystem.
pub(super) fn discover_documents(
    inputs: &[PathBuf],
    recursive: bool,
    surface: &dyn DocumentSearchSurface,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    let mut requested_inputs = Vec::new();
    for input_path in inputs {
        match RequestedInput::classify(input_path, surface) {
            Ok(Some(input)) => requested_inputs.push(input),
            Ok(None) => {}
            Err(RequestedInputFailure::Missing) => {
                lifecycle.missing_input(input_path.clone());
            }
            Err(RequestedInputFailure::Inspection(error)) => {
                lifecycle.discovery_failed(input_path.clone(), error.to_string());
            }
        }
    }

    let scope = if recursive
        && requested_inputs
            .iter()
            .any(|input| matches!(input, RequestedInput::Directory(_)))
    {
        DocumentDiscoveryScope::RecursiveDirectories
    } else {
        DocumentDiscoveryScope::RequestedInputs
    };

    lifecycle.discovering(!inputs.is_empty(), scope, |progress| {
        let mut candidates = Vec::new();
        let record_supported =
            |path: PathBuf,
             candidates: &mut Vec<DocumentCandidate>,
             progress: &mut DocumentDiscoveryProgress<'_>| {
                if let Some(candidate) = DocumentCandidate::from_path(path) {
                    candidates.push(candidate);
                    progress.document_discovered();
                }
            };

        for input in requested_inputs {
            match input {
                RequestedInput::File(path) => {
                    record_supported(path, &mut candidates, progress);
                }
                RequestedInput::Directory(path) if recursive => {
                    // Index each known directory by traversal depth so truncation leaves
                    // the nearest confirmed parent available when a failure has no path.
                    let mut known_directories = vec![path.clone()];
                    let mut traversal = surface.traverse(&path);
                    while let Some(entry_result) = traversal.next_entry() {
                        match entry_result {
                            Ok(entry) => {
                                known_directories.truncate(entry.depth());
                                let was_directory = entry.is_directory();
                                if was_directory {
                                    known_directories.push(entry.path().to_path_buf());
                                }

                                let entry_path = entry.into_path();
                                match surface.inspect(&entry_path) {
                                    Ok(InspectedKind::File) => {
                                        if was_directory {
                                            // The entry changed after enumeration; do not descend
                                            // using its now-stale directory classification.
                                            traversal.skip_current_dir();
                                        }
                                        record_supported(entry_path, &mut candidates, progress);
                                    }
                                    Ok(kind) => {
                                        if was_directory && kind != InspectedKind::Directory {
                                            // The stale directory entry no longer names a
                                            // directory, so its old traversal branch is invalid.
                                            traversal.skip_current_dir();
                                        }
                                    }
                                    Err(error) => {
                                        if was_directory {
                                            // One failed inspection should not be followed by a
                                            // second failure while opening the same directory.
                                            traversal.skip_current_dir();
                                        }
                                        progress.discovery_failed(entry_path, error.to_string());
                                    }
                                }
                            }
                            Err(failure) => {
                                known_directories.truncate(failure.depth());
                                let failure_path = failure.path().map_or_else(
                                    || {
                                        known_directories
                                            .last()
                                            .cloned()
                                            .unwrap_or_else(|| path.clone())
                                    },
                                    Path::to_path_buf,
                                );
                                progress
                                    .discovery_failed(failure_path, failure.error().to_string());
                            }
                        }
                    }
                }
                RequestedInput::Directory(path) => {
                    match surface.read_directory(&path) {
                        Ok(entries) => {
                            for entry_result in entries {
                                match entry_result {
                                    Ok(entry_path) => {
                                        // Follow nested file links as before, while retaining
                                        // the inspection error for broken links.
                                        match surface.inspect(&entry_path) {
                                            Ok(InspectedKind::File) => {
                                                record_supported(
                                                    entry_path,
                                                    &mut candidates,
                                                    progress,
                                                );
                                            }
                                            Ok(_) => {}
                                            Err(error) => {
                                                progress.discovery_failed(
                                                    entry_path,
                                                    error.to_string(),
                                                );
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        progress.discovery_failed(path.clone(), error.to_string());
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            progress.discovery_failed(path, error.to_string());
                        }
                    }
                }
            }
        }

        candidates
    })
}
