//! Collision-safe Image file emission inside the Image write pipeline.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{EmittedImageRole, PreparedImage};
use crate::output_placement::OutputPlacement;

const MAX_COLLISION_ATTEMPTS: u32 = 1000;

/// How one pipeline invocation names the images it emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputNaming {
    /// The invocation emits exactly one image, named after the base name alone.
    Singular,
    /// The invocation emits several images, numbered from 1 in emission order.
    Numbered,
}

/// Emits one pipeline invocation's images without overwriting existing output.
///
/// Bound to one Output placement for its whole lifetime: the output directory and
/// base name are given once at construction, and each image's directory is chosen
/// here from its Emitted image role rather than passed in per image.
pub(super) struct ImageFileEmission<'placement> {
    output_dir: &'placement Path,
    base_name: &'placement str,
    naming: OutputNaming,
    next_ordinal: usize,
}

impl<'placement> ImageFileEmission<'placement> {
    /// Starts Image file emission under one Output placement, after singular
    /// versus numbered naming is known.
    pub(super) fn new(placement: &'placement OutputPlacement, naming: OutputNaming) -> Self {
        Self {
            output_dir: placement.output_dir(),
            base_name: placement.base_name(),
            naming,
            next_ordinal: 1,
        }
    }

    /// Reserves and writes the next image without clobbering an existing file.
    ///
    /// The image lands in the directory its Emitted image role sends it to: a
    /// routed GIF in its GIF destination, every other role in the placement's
    /// output directory. That directory is created before every file, so it
    /// appears only once something is written to it.
    ///
    /// Only an `AlreadyExists` create failure advances to another collision
    /// suffix. Other directory, create, write, and flush failures are returned
    /// immediately with destination-specific context.
    pub(super) fn emit(&mut self, prepared: &PreparedImage<'_>) -> Result<()> {
        // Exhaustive on purpose: a new Emitted image role must fail to compile
        // here rather than silently defaulting to the document's output directory.
        let output_dir = match prepared.role {
            EmittedImageRole::RoutedGif(destination) => destination,
            EmittedImageRole::Converted
            | EmittedImageRole::ConversionSkipped
            | EmittedImageRole::Preserved => self.output_dir,
        };
        let format = prepared.format;
        let data = prepared.data.as_slice();

        fs::create_dir_all(output_dir).with_context(|| {
            format!(
                "Failed to create output directory: {}",
                output_dir.display()
            )
        })?;

        let output_filename = match self.naming {
            OutputNaming::Numbered => format!(
                "{}_{}.{}",
                self.base_name,
                self.next_ordinal,
                format.extension()
            ),
            OutputNaming::Singular => format!("{}.{}", self.base_name, format.extension()),
        };
        self.next_ordinal += 1;

        let base_stem = Path::new(&output_filename)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_default();
        let extension = format.extension();

        for collision_index in 0..=MAX_COLLISION_ATTEMPTS {
            let output_path = candidate_path(
                output_dir,
                &output_filename,
                &base_stem,
                extension,
                collision_index,
            );

            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output_path)
            {
                Ok(file) => return write_and_flush(file, &output_path, data),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to create output file: {}", output_path.display())
                    });
                }
            }
        }

        anyhow::bail!(
            "Could not find unique filename after {} attempts for {}",
            MAX_COLLISION_ATTEMPTS,
            base_stem
        )
    }
}

/// Builds one deterministic filename candidate while preserving existing naming.
fn candidate_path(
    output_dir: &Path,
    output_filename: &str,
    base_stem: &str,
    extension: &str,
    collision_index: u32,
) -> PathBuf {
    if collision_index == 0 {
        output_dir.join(output_filename)
    } else {
        output_dir.join(format!("{}_{}.{}", base_stem, collision_index, extension))
    }
}

/// Writes and flushes one atomically reserved output file.
fn write_and_flush(file: File, output_path: &Path, data: &[u8]) -> Result<()> {
    complete_reserved_file(output_path, file, |file| {
        let mut writer = io::BufWriter::new(file);
        writer.write_all(data).map_err(FileCompletionError::write)?;
        writer.flush().map_err(FileCompletionError::flush)?;
        Ok(())
    })
}

#[derive(Debug, Clone, Copy)]
enum FileCompletionStage {
    Write,
    Flush,
}

impl FileCompletionStage {
    /// Builds path-specific context for the failed completion stage.
    fn context(self, output_path: &Path) -> String {
        match self {
            FileCompletionStage::Write => {
                format!("Failed to write image data to {}", output_path.display())
            }
            FileCompletionStage::Flush => {
                format!("Failed to flush data to {}", output_path.display())
            }
        }
    }
}

#[derive(Debug)]
struct FileCompletionError {
    stage: FileCompletionStage,
    source: io::Error,
}

impl FileCompletionError {
    /// Records an image data write failure for cleanup and error reporting.
    fn write(source: io::Error) -> Self {
        Self {
            stage: FileCompletionStage::Write,
            source,
        }
    }

    /// Records a buffered output flush failure for cleanup and error reporting.
    fn flush(source: io::Error) -> Self {
        Self {
            stage: FileCompletionStage::Flush,
            source,
        }
    }
}

/// Completes a reserved file and removes partial output after a reported failure.
///
/// The completion operation owns the file handle so it is dropped before cleanup,
/// which is required for removal on Windows. Cleanup failure is included in the
/// returned context without replacing the original write or flush error source.
fn complete_reserved_file(
    output_path: &Path,
    file: File,
    complete: impl FnOnce(File) -> std::result::Result<(), FileCompletionError>,
) -> Result<()> {
    let Err(failure) = complete(file) else {
        return Ok(());
    };

    let failure_context = failure.stage.context(output_path);
    let context = match fs::remove_file(output_path) {
        Ok(()) => failure_context,
        Err(cleanup_error) => format!(
            "{}; additionally failed to remove partial output file {}: {}",
            failure_context,
            output_path.display(),
            cleanup_error
        ),
    };

    Err(failure.source).context(context)
}

#[cfg(test)]
mod tests;
