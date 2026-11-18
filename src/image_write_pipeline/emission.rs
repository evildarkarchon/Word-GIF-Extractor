//! Collision-safe Image file emission inside the Image write pipeline.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::discovery::{PayloadCopyError, PayloadShortfall};
use super::{EmittedImageRole, PreparedImage, StreamedImage};
use crate::image_format::ImageFormat;
use crate::output_placement::OutputPlacement;

const MAX_COLLISION_ATTEMPTS: u32 = 1000;

/// How one pipeline invocation names the images it emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputNaming {
    /// The invocation emits exactly one image, named after the base name alone.
    Singular,
    /// The invocation emits several images, numbered from 1 in emission order;
    /// this one takes the given ordinal.
    Numbered(usize),
}

/// What staging a streamed image did with its source.
#[derive(Debug)]
pub(super) enum StreamStaging {
    /// The source was read through: the image is written or held back, exactly as
    /// a prepared image would have been.
    Staged,
    /// The source ran short, from a read failure or by running past the Image size
    /// limit. Nothing was written for it, and naming stands where it stood before
    /// it arrived (ADR-0030).
    SourceFellShort(PayloadShortfall),
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
/// [`Self::stage`], [`Self::stage_streamed`] or [`Self::finish`], in completion
/// order and only once the file is fully written. Image file emission keeps no
/// extraction counts itself.
///
/// A streamed image's bytes arrive through a payload Archive image discovery hands
/// over, which enforces the Image size limit. Emission copies it to the file it
/// claimed and never interprets it (ADR-0030).
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
                self.write(&first, OutputNaming::Numbered(self.next_ordinal))?;
                record(first.role);
                self.write(&prepared, OutputNaming::Numbered(self.next_ordinal + 1))?;
                record(prepared.role);
                self.next_ordinal += 2;
            }
            NamingLookahead::Numbering => {
                self.write(&prepared, OutputNaming::Numbered(self.next_ordinal))?;
                record(prepared.role);
                self.next_ordinal += 1;
            }
        }

        Ok(())
    }

    /// Stages one image whose original bytes are still in its reader.
    ///
    /// Naming follows [`Self::stage`] exactly, and only the source's bytes differ.
    /// A first image is held back, so its payload is buffered, up to the Image size
    /// limit: the reader cannot outlive this call. Every later image is copied from
    /// its reader straight into its file. When the held-back image is still waiting,
    /// the arrival is written under its numbered name before the held-back image is
    /// written under the one before it. If the arrival's source then runs short, its
    /// partial file is removed, the held-back image keeps waiting, and it can still
    /// take the singular name. A source that runs short never uses up an ordinal.
    /// `record` is called once per completed file, in completion order.
    ///
    /// Returns [`StreamStaging::SourceFellShort`] when the source failed to read or
    /// ran past the limit. That is a warning for the caller to record, not an error.
    ///
    /// # Errors
    ///
    /// Returns the directory, collision, create, write or flush failure of the file
    /// that could not be completed, or a failure to remove a source's partial file.
    /// When the arrival fails this way while an image is held back, the held-back
    /// image is written under its numbered name and recorded first, so the files on
    /// disk are what [`Self::stage`] would have left. The instance must be dropped
    /// after an error.
    pub(super) fn stage_streamed(
        &mut self,
        image: StreamedImage<'policy, '_>,
        mut record: impl FnMut(EmittedImageRole<'policy>),
    ) -> Result<StreamStaging> {
        let role = image.role;
        // Taken out as in `stage`; every path below puts the right state back, and
        // an error leaves `Numbering`, which nothing reads again.
        match std::mem::replace(&mut self.lookahead, NamingLookahead::Numbering) {
            NamingLookahead::Undecided => match image.payload.buffer() {
                Ok(data) => {
                    self.lookahead = NamingLookahead::HeldBack(PreparedImage {
                        data,
                        format: image.format,
                        role,
                    });
                    Ok(StreamStaging::Staged)
                }
                Err(shortfall) => {
                    self.lookahead = NamingLookahead::Undecided;
                    Ok(StreamStaging::SourceFellShort(shortfall))
                }
            },
            NamingLookahead::HeldBack(first) => {
                let first_ordinal = self.next_ordinal;
                // The arrival goes first, so a source that runs short leaves the
                // held-back image uncommitted to a numbered name (ADR-0030).
                match self.stream(image, OutputNaming::Numbered(first_ordinal + 1)) {
                    Ok(None) => {
                        record(role);
                        self.write(&first, OutputNaming::Numbered(first_ordinal))?;
                        record(first.role);
                        self.next_ordinal += 2;
                        Ok(StreamStaging::Staged)
                    }
                    Ok(Some(shortfall)) => {
                        self.lookahead = NamingLookahead::HeldBack(first);
                        Ok(StreamStaging::SourceFellShort(shortfall))
                    }
                    Err(error) => {
                        // `stage` would have written the held-back image before the
                        // arrival failed, so it is written now to leave the same files.
                        self.write(&first, OutputNaming::Numbered(first_ordinal))?;
                        record(first.role);
                        Err(error)
                    }
                }
            }
            NamingLookahead::Numbering => {
                match self.stream(image, OutputNaming::Numbered(self.next_ordinal))? {
                    None => {
                        record(role);
                        self.next_ordinal += 1;
                        Ok(StreamStaging::Staged)
                    }
                    Some(shortfall) => Ok(StreamStaging::SourceFellShort(shortfall)),
                }
            }
        }
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

    /// Claims a name for one buffered image and writes its bytes.
    ///
    /// # Errors
    ///
    /// Returns any failure [`Self::claim`] returns, and the write or flush failure of
    /// the claimed file, whose partial output is then removed.
    fn write(&self, prepared: &PreparedImage<'_>, naming: OutputNaming) -> Result<()> {
        let (file, output_path) = self.claim(prepared.role, prepared.format, naming)?;
        write_and_flush(file, &output_path, &prepared.data)
    }

    /// Claims a name for one streamed image and copies its payload into the file.
    ///
    /// Returns `None` once the file is complete, or the shortfall when the source
    /// failed to read or ran past the Image size limit; the partial file has then
    /// been removed, so the name is free again.
    ///
    /// # Errors
    ///
    /// Returns any failure [`Self::claim`] returns, the write or flush failure of the
    /// claimed file, whose partial output is then removed, or a failure to remove the
    /// partial file a short source left. That last one fails the document, because
    /// the output would otherwise keep a truncated image that no warning accounts for.
    fn stream(
        &self,
        image: StreamedImage<'_, '_>,
        naming: OutputNaming,
    ) -> Result<Option<PayloadShortfall>> {
        let (file, output_path) = self.claim(image.role, image.format, naming)?;
        let shortfall = complete_reserved_file(&output_path, file, |file| {
            let mut writer = io::BufWriter::new(file);
            match image.payload.copy_into(&mut writer) {
                Ok(()) => {}
                Err(PayloadCopyError::Output(error)) => {
                    return Err(FileCompletionError::write(error));
                }
                Err(PayloadCopyError::Source(shortfall)) => {
                    // The file is removed next, so whatever is still buffered is
                    // discarded rather than flushed. Dropping the parts closes the
                    // handle, which Windows needs before the file can be removed.
                    drop(writer.into_parts());
                    return Ok(Some(shortfall));
                }
            }
            writer.flush().map_err(FileCompletionError::flush)?;
            Ok(None)
        })?;

        if let Some(shortfall) = shortfall {
            fs::remove_file(&output_path).with_context(|| {
                format!(
                    "Failed to remove partial output file {} after {shortfall}",
                    output_path.display()
                )
            })?;
            return Ok(Some(shortfall));
        }
        Ok(None)
    }

    /// Reserves the file one image will be written to, without clobbering an existing one.
    ///
    /// The image lands in the directory its Emitted image role sends it to: a
    /// routed GIF in its GIF destination, every other role in the placement's
    /// output directory. That directory is created before every file, so it
    /// appears only once something is written to it.
    ///
    /// Returns the newly created, empty file and its path.
    ///
    /// # Errors
    ///
    /// Only an `AlreadyExists` create failure advances to another collision
    /// suffix. Other directory and create failures are returned immediately with
    /// destination-specific context, as is running out of collision suffixes.
    fn claim(
        &self,
        role: EmittedImageRole<'_>,
        format: ImageFormat,
        naming: OutputNaming,
    ) -> Result<(File, PathBuf)> {
        // Exhaustive on purpose: a new Emitted image role must fail to compile
        // here rather than silently defaulting to the document's output directory.
        let output_dir = match role {
            EmittedImageRole::RoutedGif(destination) => destination,
            EmittedImageRole::Converted
            | EmittedImageRole::ConversionSkipped
            | EmittedImageRole::Preserved => self.placement.output_dir(),
        };

        fs::create_dir_all(output_dir).with_context(|| {
            format!(
                "Failed to create output directory: {}",
                output_dir.display()
            )
        })?;

        let output_filename = match naming {
            OutputNaming::Numbered(ordinal) => format!(
                "{}_{}.{}",
                self.placement.base_name(),
                ordinal,
                format.extension()
            ),
            OutputNaming::Singular => {
                format!("{}.{}", self.placement.base_name(), format.extension())
            }
        };

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
                Ok(file) => return Ok((file, output_path)),
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
/// Whatever the operation returns on success is handed back: a streamed image uses
/// it to report a source that ran short, which this function does not clean up.
fn complete_reserved_file<T>(
    output_path: &Path,
    file: File,
    complete: impl FnOnce(File) -> std::result::Result<T, FileCompletionError>,
) -> Result<T> {
    let failure = match complete(file) {
        Ok(completed) => return Ok(completed),
        Err(failure) => failure,
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
