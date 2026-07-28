//! Requested-input discovery inside Document selection.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use super::{
    DocumentCandidate, DocumentSelectionDiagnostic, DocumentSelectionLifecycle,
    DocumentSelectionScanScope, ScanningProgress,
};

/// One successfully inspected requested input retained for the scanning phase.
enum RequestedInput {
    File(PathBuf),
    Directory(PathBuf),
}

/// A failed requested-input classification before scanning begins.
enum RequestedInputFailure {
    Missing,
    Inspection(io::Error),
}

impl RequestedInput {
    /// Classifies one requested path with followed metadata while preserving broken-link identity.
    ///
    /// The non-following lookup is used only after a not-found target lookup so a
    /// genuinely absent path remains distinct from a link whose target is missing.
    /// A supported file or directory returns its retained classification, an
    /// inspectable unsupported object returns `None`, and failures distinguish a
    /// genuinely missing path from every other inspection error.
    fn classify(path: &Path) -> Result<Option<Self>, RequestedInputFailure> {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(Self::File(path.to_path_buf()))),
            Ok(metadata) if metadata.is_dir() => Ok(Some(Self::Directory(path.to_path_buf()))),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::symlink_metadata(path) {
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
/// Every requested root is classified once before scanning, so root diagnostics
/// precede the initial snapshot and independently readable inputs still continue.
/// Requested directory links are followed, while existing nested traversal policy
/// and later EPUB selection phases remain unchanged.
pub(super) fn discover_documents(
    inputs: &[PathBuf],
    recursive: bool,
    lifecycle: &mut DocumentSelectionLifecycle<'_>,
) -> Vec<DocumentCandidate> {
    let mut requested_inputs = Vec::new();
    for input_path in inputs {
        match RequestedInput::classify(input_path) {
            Ok(Some(input)) => requested_inputs.push(input),
            Ok(None) => {}
            Err(RequestedInputFailure::Missing) => {
                lifecycle.diagnostic(DocumentSelectionDiagnostic::MissingInput {
                    path: input_path.clone(),
                });
            }
            Err(RequestedInputFailure::Inspection(error)) => {
                lifecycle.diagnostic(DocumentSelectionDiagnostic::DocumentDiscoveryFailed {
                    path: input_path.clone(),
                    detail: error.to_string(),
                });
            }
        }
    }

    let scope = if recursive
        && requested_inputs
            .iter()
            .any(|input| matches!(input, RequestedInput::Directory(_)))
    {
        DocumentSelectionScanScope::RecursiveDirectories
    } else {
        DocumentSelectionScanScope::RequestedInputs
    };

    lifecycle.scanning(!inputs.is_empty(), scope, |progress| {
        let mut candidates = Vec::new();
        let record_supported =
            |path: PathBuf,
             candidates: &mut Vec<DocumentCandidate>,
             progress: &mut ScanningProgress<'_>| {
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
                    for entry in WalkDir::new(path).min_depth(1).into_iter().flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_file() {
                            record_supported(entry_path.to_path_buf(), &mut candidates, progress);
                        }
                    }
                }
                RequestedInput::Directory(path) => {
                    match fs::read_dir(&path) {
                        Ok(entries) => {
                            for entry_result in entries {
                                match entry_result {
                                    Ok(entry) => {
                                        let entry_path = entry.path();
                                        // Follow nested file links as before, while retaining
                                        // the inspection error for broken links.
                                        match fs::metadata(&entry_path) {
                                            Ok(metadata) if metadata.is_file() => {
                                                record_supported(
                                                    entry_path,
                                                    &mut candidates,
                                                    progress,
                                                );
                                            }
                                            Ok(_) => {}
                                            Err(error) => {
                                                progress.diagnostic(
                                                    DocumentSelectionDiagnostic::DocumentDiscoveryFailed {
                                                        path: entry_path,
                                                        detail: error.to_string(),
                                                    },
                                                );
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        progress.diagnostic(
                                            DocumentSelectionDiagnostic::DocumentDiscoveryFailed {
                                                path: path.clone(),
                                                detail: error.to_string(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            progress.diagnostic(
                                DocumentSelectionDiagnostic::DocumentDiscoveryFailed {
                                    path,
                                    detail: error.to_string(),
                                },
                            );
                        }
                    }
                }
            }
        }

        candidates
    })
}
