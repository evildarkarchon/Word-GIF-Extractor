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
enum OutputNaming {
    /// The invocation emits exactly one image, named after the base name alone.
    Singular,
    /// The invocation emits several images, numbered from 1 in emission order.
    Numbered,
}

/// Where Image file emission stands on singular versus numbered naming.
///
/// A lone image is named after the base name alone and several are numbered, so the
/// first staged image cannot be written until emission knows whether a second
/// follows. These three states are the whole of that wait: holding an image back
/// and numbering are never both true.
enum NamingLookahead<'policy> {
    /// No image has been staged yet.
    Undecided,
    /// One image is held back until the next one, or finishing, names it.
    HeldBack(PreparedImage<'policy>),
    /// A second image arrived, so every image from here on is numbered.
    Numbering,
}

/// Emits one pipeline invocation's images without overwriting existing output.
///
/// Bound to one Output placement for its whole lifetime: the output directory and
/// base name are given once at construction, and each image's directory is chosen
/// here from its Emitted image role rather than passed in per image. Whether names
/// are singular or numbered is decided here too, by holding the first staged image
/// back until a second arrives or emission finishes, so no caller ever chooses a
/// naming rule of its own.
///
/// Each completed file is reported back through the recording callback passed to
/// [`Self::stage`] or [`Self::finish`], in completion order and only once the file
/// is fully written. Image file emission keeps no extraction counts itself.
///
/// # Unusable after an error
///
/// Once [`Self::stage`] returns an error the instance must be dropped: staging or
/// finishing it again is not supported and may name files wrongly. This is a
/// documented contract rather than one the type enforces (ADR-0022), because both
/// callers already abort their document on that error.
pub(super) struct ImageFileEmission<'policy, 'placement> {
    placement: &'placement OutputPlacement,
    lookahead: NamingLookahead<'policy>,
    next_ordinal: usize,
}

impl<'policy, 'placement> ImageFileEmission<'policy, 'placement> {
    /// Starts Image file emission under one Output placement.
    ///
    /// Nothing is written and no directory is created until an image is staged and
    /// its name is known.
    pub(super) fn new(placement: &'placement OutputPlacement) -> Self {
        Self {
            placement,
            lookahead: NamingLookahead::Undecided,
            next_ordinal: 1,
        }
    }

    /// Stages one prepared image, writing whatever its arrival lets emission name.
    ///
    /// The first image is held back and nothing is written, because singular versus
    /// numbered naming is not yet known. The second writes the held-back image and
    /// then itself, both numbered, and every later image is written numbered as it
    /// arrives. `record` is called once per completed file, in completion order,
    /// with that file's Emitted image role.
    ///
    /// # Errors
    ///
    /// Returns the directory, collision, create, write or flush failure of the file
    /// that could not be completed. Files completed before it have already been
    /// recorded, including a held-back image written just before a failing second
    /// one. The instance must be dropped after an error.
    pub(super) fn stage(
        &mut self,
        prepared: PreparedImage<'policy>,
        mut record: impl FnMut(EmittedImageRole<'policy>),
    ) -> Result<()> {
        // The state is taken out so writing can borrow emission mutably. An error
        // leaves it `Numbering`, which the type's contract says nothing reads again.
        match std::mem::replace(&mut self.lookahead, NamingLookahead::Numbering) {
            NamingLookahead::Undecided => {
                self.lookahead = NamingLookahead::HeldBack(prepared);
            }
            NamingLookahead::HeldBack(first) => {
                // The held-back image is recorded before the second is attempted, so
                // a failure on the second still leaves the first in the caller's facts.
                self.write(&first, OutputNaming::Numbered)?;
                record(first.role);
                self.write(&prepared, OutputNaming::Numbered)?;
                record(prepared.role);
            }
            NamingLookahead::Numbering => {
                self.write(&prepared, OutputNaming::Numbered)?;
                record(prepared.role);
            }
        }

        Ok(())
    }

    /// Ends emission, writing a still-held-back image under the singular name.
    ///
    /// A lone staged image is named after the base name alone and reported to
    /// `record` once complete. If nothing was staged, nothing is written and no
    /// directory is created; if numbering already began, every image is written.
    ///
    /// # Errors
    ///
    /// Returns the directory, collision, create, write or flush failure of the lone
    /// held-back image, which is then not recorded.
    pub(super) fn finish(mut self, record: impl FnOnce(EmittedImageRole<'policy>)) -> Result<()> {
        // Taken out rather than moved, so writing can still borrow emission mutably.
        if let NamingLookahead::HeldBack(lone) =
            std::mem::replace(&mut self.lookahead, NamingLookahead::Undecided)
        {
            self.write(&lone, OutputNaming::Singular)?;
            record(lone.role);
        }

        Ok(())
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
    fn write(&mut self, prepared: &PreparedImage<'_>, naming: OutputNaming) -> Result<()> {
        // Exhaustive on purpose: a new Emitted image role must fail to compile
        // here rather than silently defaulting to the document's output directory.
        let output_dir = match prepared.role {
            EmittedImageRole::RoutedGif(destination) => destination,
            EmittedImageRole::Converted
            | EmittedImageRole::ConversionSkipped
            | EmittedImageRole::Preserved => self.placement.output_dir(),
        };
        let format = prepared.format;
        let data = prepared.data.as_slice();

        fs::create_dir_all(output_dir).with_context(|| {
            format!(
                "Failed to create output directory: {}",
                output_dir.display()
            )
        })?;

        let output_filename = match naming {
            OutputNaming::Numbered => format!(
                "{}_{}.{}",
                self.placement.base_name(),
                self.next_ordinal,
                format.extension()
            ),
            OutputNaming::Singular => {
                format!("{}.{}", self.placement.base_name(), format.extension())
            }
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
