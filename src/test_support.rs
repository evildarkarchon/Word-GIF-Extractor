//! Shared fixtures for the crate's in-crate tests.
//!
//! Every temporary-directory helper, ZIP fixture builder, platform directory-link
//! helper and silent observer used by a `#[cfg(test)] mod tests` sibling lives here,
//! so that adding a test never means writing another copy of one.
//!
//! # Why this is a plain `#[cfg(test)]` module
//!
//! Unit tests and integration tests cannot share a plain module: `cfg(test)` is not
//! set when the library is compiled for an integration test, so nothing in here is
//! reachable from `tests/`. The integration side therefore has a second support module
//! of its own, `tests/support/mod.rs`, and a small deliberate overlap between the two —
//! the temporary directory helper, the DOCX builder, the directory-link pair — is
//! expected and accepted. Helpers that look duplicated across `src/` and `tests/` are
//! that overlap, not a leftover of this consolidation.
//!
//! Two ways of removing that overlap were considered and rejected:
//!
//! - A feature-gated support module with the package depending on itself would let
//!   both sides import the same code, but a package listed in its own dependencies
//!   is surprising to every future reader, and the whole benefit is removing one
//!   small duplicated helper. Not worth the confusion.
//! - A separate workspace crate holding the fixtures is the textbook answer, but it
//!   is disproportionate for a single-package repository: it adds a workspace, a
//!   manifest and a build unit to share a few hundred lines of test scaffolding.
//!
//! If either trade-off changes — the repository gains a second package, or the
//! overlap grows past a few helpers — the workspace crate becomes the better option.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

use crate::document_selection::{
    DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionProgress,
};
use crate::extraction_run::{ExtractionRunObservation, ExtractionRunObserver};

/// Returns an unused temporary directory path for one test.
///
/// `area` names the module under test and only makes the path readable when a
/// failing test leaves its directory behind; the process id and nanosecond stamp
/// are what actually keep concurrent tests from colliding. The directory is not
/// created — callers that need it on disk create it themselves.
pub(crate) fn temp_test_dir(area: &str, test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-{area}-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

/// Returns an unused temporary `.epub` file path for one test.
///
/// `area` means what it does in [`temp_test_dir`]. Used by tests that want a single
/// archive file rather than a directory to fill.
pub(crate) fn temp_epub_path(area: &str, test_name: &str) -> PathBuf {
    // Appended rather than set with `with_extension`, which would truncate at the last
    // `.` — an `area` or `test_name` containing one would silently eat the stamp that
    // makes the path unique.
    let path = temp_test_dir(area, test_name);
    let mut file_name = path
        .file_name()
        .expect("generated temporary path should have a file name")
        .to_os_string();
    file_name.push(".epub");
    path.with_file_name(file_name)
}

/// Creates a directory link used to exercise the platform filesystem through selection.
#[cfg(unix)]
pub(crate) fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("test directory symlink should be creatable");
}

/// Creates a directory link without requiring Windows symbolic-link privileges.
#[cfg(windows)]
pub(crate) fn create_directory_link(target: &Path, link: &Path) {
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
pub(crate) fn remove_directory_link(link: &Path) {
    fs::remove_file(link).expect("test directory symlink should be removable");
}

/// Removes a Windows directory symlink or junction without following it.
#[cfg(windows)]
pub(crate) fn remove_directory_link(link: &Path) {
    fs::remove_dir(link).expect("test directory link should be removable");
}

/// Creates a file symlink used to preserve nested supported-file eligibility.
#[cfg(unix)]
pub(crate) fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("test file symlink should be creatable");
    true
}

/// Attempts to create a genuine Windows file symlink when the host permits it.
///
/// Returns whether the link was created, because unprivileged Windows hosts cannot
/// create one and there is no junction equivalent for files.
#[cfg(windows)]
pub(crate) fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

/// Removes a file symlink without removing its target.
pub(crate) fn remove_file_symlink(link: &Path) {
    fs::remove_file(link).expect("test file symlink should be removable");
}

/// Ignores every Document selection fact, for tests whose seam is further downstream.
#[derive(Default)]
pub(crate) struct SilentDocumentSelectionObserver;

impl DocumentSelectionObserver for SilentDocumentSelectionObserver {
    /// Ignores progress facts that are outside the calling test's seam.
    fn on_document_selection_progress(&mut self, _progress: DocumentSelectionProgress) {}

    /// Ignores diagnostics because the fixtures these tests build are readable.
    fn on_document_selection_diagnostic(&mut self, _diagnostic: DocumentSelectionDiagnostic) {}
}

/// Ignores the live run timeline, for tests that assert files and semantic outcomes.
#[derive(Default)]
pub(crate) struct SilentExtractionRunObserver;

impl ExtractionRunObserver for SilentExtractionRunObserver {
    /// Ignores live observations because the calling test asserts the returned outcome.
    fn on_observation(&mut self, _observation: ExtractionRunObservation) {}
}

/// Writes a ZIP archive containing the supplied entries in order, with the given options.
///
/// Entry order is preserved because several tests assert output numbering follows
/// archive order.
fn write_zip(path: &Path, options: SimpleFileOptions, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("test archive should be creatable");
    let mut zip = zip::ZipWriter::new(file);
    for (name, data) in entries {
        zip.start_file(*name, options)
            .expect("ZIP entry should start");
        zip.write_all(data)
            .expect("ZIP entry payload should be writable");
    }
    zip.finish().expect("test archive should finish");
}

/// Writes a ZIP archive containing the supplied entries in order.
pub(crate) fn write_zip_archive(path: &Path, entries: &[(&str, &[u8])]) {
    write_zip(path, SimpleFileOptions::default(), entries);
}

/// Returns options that store payloads uncompressed.
///
/// Tests that corrupt one payload by searching the archive bytes need stored entries,
/// because a deflated payload does not appear verbatim in the file.
fn stored_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
}

/// Writes a DOCX fixture containing the supplied archive entries in order.
///
/// A DOCX fixture is nothing but a ZIP, so this delegates rather than adding steps. It
/// is kept as a separate name on purpose: DOCX tests read as building a document, and
/// the day a fixture needs real DOCX scaffolding this is where it goes.
pub(crate) fn write_docx(path: &Path, entries: &[(&str, &[u8])]) {
    write_zip_archive(path, entries);
}

/// Writes a DOCX whose single entry has an image extension but no matching magic bytes.
pub(crate) fn write_extension_fallback_docx(path: &Path) {
    write_docx(path, &[("word/media/image1.png", b"not actually a png")]);
}

/// The EPUB container declaration pointing at the package document every fixture writes.
const EPUB_CONTAINER_XML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

/// The navigation document written whenever a fixture declares a navigation item.
///
/// Its body is deliberately empty, and no test asserts anything about it: navigation is
/// declared as `application/xhtml+xml`, so extraction never treats it as an image
/// resource. Only its presence matters, so the declared manifest item resolves to a
/// real archive entry.
const EPUB_NAV_XHTML: &[u8] = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body/></html>";

/// Whether a package document declares a navigation item and references it from the spine.
enum EpubNavigation {
    /// The package declares `nav.xhtml` and the spine references it.
    Declared,
    /// The package declares no navigation document and its spine is empty.
    Absent,
}

/// Renders manifest item declarations from `(id, href, media type, properties)` tuples.
fn epub_manifest_items(resources: &[(&str, &str, &str, Option<&str>)]) -> String {
    resources
        .iter()
        .map(|(id, href, mime, properties)| {
            let properties = properties
                .map(|value| format!(" properties=\"{value}\""))
                .unwrap_or_default();
            format!("    <item id=\"{id}\" href=\"{href}\" media-type=\"{mime}\"{properties}/>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Renders one EPUB package document from its descriptive and manifest declarations.
///
/// [`EpubNavigation::Declared`] adds both the navigation manifest item and its spine
/// reference together, since a declared navigation document is only valid when the
/// spine references it.
fn epub_package(
    title: &str,
    creator: Option<&str>,
    navigation: EpubNavigation,
    manifest_items: &str,
) -> String {
    let creator = creator
        .map(|creator| format!("\n    <dc:creator>{creator}</dc:creator>"))
        .unwrap_or_default();
    let (nav_item, spine) = match navigation {
        EpubNavigation::Declared => (
            "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
            "<itemref idref=\"nav\"/>",
        ),
        EpubNavigation::Absent => ("", ""),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>{title}</dc:title>{creator}
  </metadata>
  <manifest>
{nav_item}{manifest_items}
  </manifest>
  <spine>{spine}</spine>
</package>"#
    )
}

/// Writes an EPUB from one package document and the archive entries that follow it.
///
/// Every EPUB fixture in this module funnels through here, so the `mimetype` and
/// container entries are declared once and always come first — the order the format
/// requires. `options` decides compression for the whole archive.
fn write_epub_archive(
    path: &Path,
    options: SimpleFileOptions,
    package: &[u8],
    entries: &[(&str, &[u8])],
) {
    let mut all_entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", EPUB_CONTAINER_XML),
        ("OEBPS/content.opf", package),
    ];
    all_entries.extend_from_slice(entries);
    write_zip(path, options, &all_entries);
}

/// Writes an EPUB from the supplied package declarations and payload entries.
///
/// Takes the package document verbatim so that declaration-level tests can write
/// packages a higher-level builder would not produce.
pub(crate) fn write_epub_package(path: &Path, package: &[u8], resources: &[(&str, &[u8])]) {
    write_epub_archive(path, SimpleFileOptions::default(), package, resources);
}

/// Writes a readable EPUB whose only declarations are its descriptive ones.
pub(crate) fn write_epub_with_descriptive_declarations(path: &Path, author: &str, title: &str) {
    write_epub_package(
        path,
        epub_package(title, Some(author), EpubNavigation::Absent, "").as_bytes(),
        &[],
    );
}

/// Writes a navigable EPUB declaring one image resource, with its payload present.
pub(crate) fn write_epub_with_one_image(
    path: &Path,
    image_href: &str,
    image_mime: &str,
    image_data: &[u8],
) {
    let image_path = format!("OEBPS/{image_href}");
    write_navigable_epub(
        path,
        SimpleFileOptions::default(),
        "Magic Test",
        Some("Tester"),
        &[("img", image_href, image_mime, None)],
        &[(image_path.as_str(), image_data)],
    );
}

/// Writes an EPUB whose manifest declarations and ZIP payloads vary independently.
///
/// Nothing forces the two to agree, which is what lets tests build an EPUB declaring
/// a resource the archive does not contain. The package declares the title `Test` and
/// no creator.
pub(crate) fn write_epub_fixture(
    path: &Path,
    manifest_resources: &[(&str, &str, &str, Option<&str>)],
    archive_resources: &[(&str, &[u8])],
) {
    write_navigable_epub(
        path,
        SimpleFileOptions::default(),
        "Test",
        None,
        manifest_resources,
        archive_resources,
    );
}

/// Writes [`write_epub_fixture`]'s shape with uncompressed payloads and named declarations.
///
/// Two things differ from [`write_epub_fixture`], and tests depend on both: payloads are
/// stored rather than deflated, so a test can locate and corrupt one inside the archive
/// bytes; and the package declares the title `Magic Test` and creator `Tester`, which is
/// what the resulting output file names are built from.
pub(crate) fn write_stored_epub_fixture(
    path: &Path,
    manifest_resources: &[(&str, &str, &str, Option<&str>)],
    archive_resources: &[(&str, &[u8])],
) {
    write_navigable_epub(
        path,
        stored_options(),
        "Magic Test",
        Some("Tester"),
        manifest_resources,
        archive_resources,
    );
}

/// Writes an EPUB that declares a navigation document alongside its resources.
fn write_navigable_epub(
    path: &Path,
    options: SimpleFileOptions,
    title: &str,
    creator: Option<&str>,
    manifest_resources: &[(&str, &str, &str, Option<&str>)],
    archive_resources: &[(&str, &[u8])],
) {
    let package = epub_package(
        title,
        creator,
        EpubNavigation::Declared,
        &epub_manifest_items(manifest_resources),
    );
    let mut entries: Vec<(&str, &[u8])> = vec![("OEBPS/nav.xhtml", EPUB_NAV_XHTML)];
    entries.extend_from_slice(archive_resources);
    write_epub_archive(path, options, package.as_bytes(), &entries);
}

/// Writes an EPUB declaring one JPEG image, with an optional manifest property.
pub(crate) fn write_epub_image(
    path: &Path,
    image_href: &str,
    properties: Option<&str>,
    data: &[u8],
) {
    let archive_path = format!("OEBPS/{image_href}");
    write_epub_with_resources(
        path,
        image_href,
        properties,
        &[(archive_path.as_str(), data)],
    );
}

/// Writes an EPUB declaring one JPEG image whose available payloads are chosen freely.
pub(crate) fn write_epub_with_resources(
    path: &Path,
    image_href: &str,
    properties: Option<&str>,
    archive_resources: &[(&str, &[u8])],
) {
    write_epub_fixture(
        path,
        &[("image", image_href, "image/jpeg", properties)],
        archive_resources,
    );
}

/// Writes an EPUB with declaration-derived identity and at most one image.
///
/// The image tuple is `(file name, payload, is cover)`; the file is declared under
/// `images/` and stored at `OEBPS/images/`. The archive declares no navigation
/// document, so it exercises the sparser package shape.
pub(crate) fn write_epub_document(
    path: &Path,
    creator: &str,
    title: &str,
    image: Option<(&str, &[u8], bool)>,
) {
    let href = image.map(|(name, _, _)| format!("images/{name}"));
    let manifest = match (&href, image) {
        (Some(href), Some((_, _, is_cover))) => epub_manifest_items(&[(
            "image",
            href.as_str(),
            "image/jpeg",
            is_cover.then_some("cover-image"),
        )]),
        _ => String::new(),
    };
    let package = epub_package(title, Some(creator), EpubNavigation::Absent, &manifest);
    let image_path = href.as_ref().map(|href| format!("OEBPS/{href}"));
    let entries: Vec<(&str, &[u8])> = match (&image_path, image) {
        (Some(image_path), Some((_, data, _))) => vec![(image_path.as_str(), data)],
        _ => Vec::new(),
    };
    write_epub_archive(
        path,
        SimpleFileOptions::default(),
        package.as_bytes(),
        &entries,
    );
}

/// Writes an EPUB with descriptive declarations and one declared cover resource.
pub(crate) fn write_epub_with_cover(path: &Path) {
    write_epub_package(
        path,
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">declarations-test</dc:identifier>
    <dc:title>Retained Title</dc:title>
    <dc:creator>Retained Creator</dc:creator>
  </metadata>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine></spine>
</package>"#,
        &[("OEBPS/cover.png", b"cover payload")],
    );
}

/// Writes a readable EPUB with no descriptive, cover, or resource declarations.
pub(crate) fn write_sparse_epub(path: &Path) {
    write_epub_package(
        path,
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">sparse-test</dc:identifier>
  </metadata>
  <manifest></manifest>
  <spine></spine>
</package>"#,
        &[],
    );
}
