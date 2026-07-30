//! Shared fixtures for the crate's integration tests.
//!
//! Every file under `tests/` is its own binary, so a helper written in one of them is
//! invisible to the rest. This module is what they share instead: the temporary
//! directory helper, the ZIP fixture builders, the platform directory-link pair, and
//! the one call that drives the library with a capturing destination. Adding an
//! integration test should never mean writing another copy of one.
//!
//! It lives in a directory rather than as `tests/support.rs` because Cargo builds
//! every top-level file under `tests/` as a test binary of its own; a file inside a
//! subdirectory is only ever compiled as part of the binary that declares it.
//!
//! # Why this duplicates part of `src/test_support.rs`
//!
//! `cfg(test)` is not set when the library is compiled for an integration test, so
//! nothing in the crate-private support module is reachable from here. The overlap —
//! the temporary directory helper, the DOCX builder, the directory-link pair — is
//! deliberate and was accepted when this module was created. The two alternatives, a
//! feature-gated module with the package depending on itself and a separate workspace
//! crate for fixtures, are argued and rejected in the header of
//! `src/test_support.rs`, which is also where the note lives about when the workspace
//! crate becomes the better answer.

// Each test binary uses part of this module and not the rest, and an unused `pub` item
// in a binary crate is dead code. The alternative is a per-file list of `#[allow]`s
// that has to be maintained every time a test moves between files.
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use clap::Parser;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, Rgb, RgbImage};
use zip::write::SimpleFileOptions;

use word_image_extractor::{Args, Capture, TerminalOutput, run_cli};

/// A PNG small enough to inline whose magic bytes still identify it as one.
const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

/// Drives the library entry point once, with a destination that captures everything.
///
/// The program name every `clap` parse expects is supplied here, so a call site lists
/// only the flags its own test is about.
///
/// Both halves of the return value matter, and the split between them is narrower than
/// it looks. The capture holds what the run said, split by stream and with the progress
/// display's activity counted separately — that is where nearly every assertion belongs.
/// The returned result holds only what Extraction run intake refused: an intake failure
/// travels as the returned error and never reaches the destination, because the process
/// exit path is what prints it. Nothing that happens after intake can make it an error,
/// so a run that found no documents, produced nothing, or failed every document it
/// opened still returns `Ok`. A test asserting intake wording therefore reads the error;
/// a test asserting anything else reads the capture and the files on disk.
pub fn run_captured(arguments: &[&str]) -> (Result<()>, Capture) {
    let args = Args::try_parse_from(
        std::iter::once("word-image-extractor").chain(arguments.iter().copied()),
    )
    .expect("integration test arguments should parse");
    let (output, capture) = TerminalOutput::captured();

    (run_cli(args, output), capture)
}

/// Runs `body` with the process working directory moved to `directory`.
///
/// Two behaviours are defined against the working directory — the input Extraction run
/// intake defaults to when none is named, and the "beside the input" output rule that
/// has to be distinguishable from it — so the tests covering them have to move it. It
/// is process-global state, which is why each of those tests is the only test in its
/// file: separate files are separate binaries, so a file holding one test can move its
/// own working directory without racing anything.
///
/// `body` receives the directory as the process now reports it, which is not always the
/// path passed in — a temporary directory reached through a symlink resolves
/// differently — and the resolved form is what intake puts in its notice. The original
/// is restored whether `body` returns or unwinds, because a directory cannot be removed
/// on Windows while it is some process's working directory — and a failing test is
/// exactly when someone wants to go and look at what it left behind.
pub fn with_current_dir<T>(directory: &Path, body: impl FnOnce(&Path) -> T) -> T {
    let _restore = WorkingDirectoryGuard {
        original: std::env::current_dir()
            .expect("the current working directory should be readable"),
    };
    std::env::set_current_dir(directory).expect("the test working directory should be enterable");
    let resolved = std::env::current_dir()
        .expect("the test working directory should be readable once entered");

    body(&resolved)
}

/// Restores the process working directory when it goes out of scope.
///
/// A guard rather than a call after `body` because the case that needs it is the failing
/// one: a panicking assertion never reaches a trailing statement, and the point of
/// restoring is to leave the temporary tree removable. This relies on a failing test
/// unwinding; the `panic = "abort"` in `Cargo.toml` applies to the release profile only,
/// and setting it for the test profile would silently disable this.
struct WorkingDirectoryGuard {
    original: PathBuf,
}

impl Drop for WorkingDirectoryGuard {
    /// Restores the original working directory, ignoring a failure to do so.
    ///
    /// The failure is swallowed because this runs during unwinding when a test fails,
    /// and panicking here would abort the process and take the failing test's own
    /// message with it. There is nothing useful left to do about a working directory
    /// that can no longer be re-entered.
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

/// Returns an unused temporary directory path for one integration test.
///
/// `area` names the behaviour under test and only makes the path readable when a
/// failing test leaves its directory behind; the process id and nanosecond stamp are
/// what actually keep concurrent tests from colliding. The directory is not created —
/// callers that need it on disk create it themselves.
pub fn temp_test_dir(area: &str, test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-{area}-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

/// Writes a ZIP archive containing the supplied entries in order.
///
/// Entry order is preserved because output numbering follows archive order.
fn write_zip_archive(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("test archive should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    for (name, data) in entries {
        zip.start_file(*name, SimpleFileOptions::default())
            .expect("ZIP entry should start");
        zip.write_all(data)
            .expect("ZIP entry payload should be writable");
    }
    zip.finish().expect("test archive should finish");
}

/// Writes a DOCX fixture containing the supplied archive entries in order.
///
/// A DOCX fixture is nothing but a ZIP, so this delegates rather than adding steps. It
/// is kept as a separate name for the same reason the crate-private builder is: DOCX
/// tests read as building a document, and the day a fixture needs real DOCX scaffolding
/// this is where it goes.
pub fn write_docx(path: &Path, entries: &[(&str, &[u8])]) {
    write_zip_archive(path, entries);
}

/// Writes a DOCX whose single image is a PNG that magic-byte detection identifies.
pub fn write_png_docx(path: &Path) {
    write_docx(path, &[("word/media/image1.png", MINIMAL_PNG)]);
}

/// Writes a DOCX whose PNG payload is stored under a `.bin` entry name.
///
/// The extension says nothing useful, so anything that extracts this file has
/// identified it from its magic bytes.
pub fn write_mislabeled_png_docx(path: &Path) {
    write_docx(path, &[("word/media/image1.bin", MINIMAL_PNG)]);
}

/// Writes a DOCX whose single image defeats magic detection and warns on fallback.
///
/// Byte-identical to the crate-private helper of the same name, deliberately: a shared
/// name that built a subtly different fixture would be a trap. The entry name is not
/// what the emitted file is named after — a document with one image is named after the
/// document — so nothing depends on which of the two a test happens to reach.
pub fn write_extension_fallback_docx(path: &Path) {
    write_docx(path, &[("word/media/image1.png", b"not actually a png")]);
}

/// Encodes a small valid JPEG whose bytes can be compared after extraction.
pub fn test_jpeg() -> Vec<u8> {
    let mut image = RgbImage::new(8, 8);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = Rgb([(x * 31) as u8, (y * 29) as u8, ((x + y) * 17) as u8]);
    }

    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 95)
        .encode_image(&DynamicImage::ImageRgb8(image))
        .expect("test JPEG should encode");
    bytes
}

/// Reports whether `directory` holds at least one file with the given extension.
pub fn has_file_with_extension(directory: &Path, extension: &str) -> bool {
    fs::read_dir(directory)
        .expect("directory should be readable")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == extension)
        })
}

/// Creates a directory link used to exercise requested-root inspection through a run.
#[cfg(unix)]
pub fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("test directory symlink should be creatable");
}

/// Creates a directory link without requiring Windows symbolic-link privileges.
#[cfg(windows)]
pub fn create_directory_link(target: &Path, link: &Path) {
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
pub fn remove_directory_link(link: &Path) {
    fs::remove_file(link).expect("test directory symlink should be removable");
}

/// Removes a Windows directory symlink or junction without following it.
#[cfg(windows)]
pub fn remove_directory_link(link: &Path) {
    fs::remove_dir(link).expect("test directory link should be removable");
}
