//! Collision-safe Image file emission inside the Image write pipeline.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::image_format::ImageFormat;

const MAX_COLLISION_ATTEMPTS: u32 = 1000;

/// Emits one pipeline invocation's images without overwriting existing output.
pub(super) struct ImageFileEmission<'name> {
    base_name: &'name str,
    multiple_images: bool,
    next_ordinal: usize,
}

impl<'name> ImageFileEmission<'name> {
    /// Starts Image file emission after singular versus multiple naming is known.
    pub(super) fn new(base_name: &'name str, multiple_images: bool) -> Self {
        Self {
            base_name,
            multiple_images,
            next_ordinal: 1,
        }
    }

    /// Reserves and writes the next image without clobbering an existing file.
    ///
    /// Only an `AlreadyExists` create failure advances to another collision
    /// suffix. Other directory, create, write, and flush failures are returned
    /// immediately with destination-specific context.
    pub(super) fn emit(
        &mut self,
        output_dir: &Path,
        format: ImageFormat,
        data: &[u8],
    ) -> Result<()> {
        fs::create_dir_all(output_dir).with_context(|| {
            format!(
                "Failed to create output directory: {}",
                output_dir.display()
            )
        })?;

        let output_filename = if self.multiple_images {
            format!(
                "{}_{}.{}",
                self.base_name,
                self.next_ordinal,
                format.extension()
            )
        } else {
            format!("{}.{}", self.base_name, format.extension())
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
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Returns a unique local filesystem directory for one emission test.
    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-emission-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn write_failure_removes_the_reserved_partial_file() {
        let temp_dir = temp_test_dir("partial-cleanup");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let output_path = temp_dir.join("sample.png");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .expect("test output should be reservable");

        let result = complete_reserved_file(&output_path, file, |mut file| {
            file.write_all(b"partial")
                .expect("partial test bytes should be writable");
            Err(FileCompletionError::write(io::Error::other(
                "injected write failure",
            )))
        });

        assert!(result.is_err());
        assert!(!output_path.exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn cleanup_failure_is_reported_with_the_original_write_failure() {
        let temp_dir = temp_test_dir("cleanup-failure");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let output_path = temp_dir.join("sample.png");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .expect("test output should be reservable");
        let replacement_path = output_path.clone();

        let error = complete_reserved_file(&output_path, file, move |mut file| {
            file.write_all(b"partial")
                .expect("partial test bytes should be writable");
            drop(file);
            fs::remove_file(&replacement_path).expect("reserved test file should be removable");
            fs::create_dir(&replacement_path)
                .expect("replacement directory should make file cleanup fail");
            fs::write(replacement_path.join("child"), b"occupied")
                .expect("replacement directory should be non-empty");
            Err(FileCompletionError::write(io::Error::other(
                "injected write failure",
            )))
        })
        .expect_err("injected write and cleanup failures should be returned");
        let error_report = format!("{error:#}");

        assert!(error_report.contains("injected write failure"));
        assert!(error_report.contains("failed to remove partial output file"));
        assert!(error_report.contains(&output_path.display().to_string()));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
