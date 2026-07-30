//! Tests for EPUB declaration acquisition.

use super::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

/// Returns an isolated temporary directory for one EPUB declaration test.
fn temp_test_dir(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "word-image-extractor-epub-declarations-{test_name}-{}-{nanos}",
        std::process::id()
    ))
}

const CONTAINER_XML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

/// Writes a minimal EPUB from the supplied package declarations and payload entries.
fn write_epub(path: &Path, package: &[u8], resources: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("test EPUB should be creatable");
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    archive
        .start_file("mimetype", options)
        .expect("mimetype entry should start");
    archive
        .write_all(b"application/epub+zip")
        .expect("mimetype should be writable");
    archive
        .start_file("META-INF/container.xml", options)
        .expect("container entry should start");
    archive
        .write_all(CONTAINER_XML)
        .expect("container should be writable");
    archive
        .start_file("OEBPS/content.opf", options)
        .expect("OPF entry should start");
    archive.write_all(package).expect("OPF should be writable");
    for (name, payload) in resources {
        archive
            .start_file(*name, options)
            .expect("resource entry should start");
        archive
            .write_all(payload)
            .expect("resource payload should be writable");
    }
    archive.finish().expect("test EPUB should finish");
}

/// Writes an EPUB with descriptive declarations and one declared cover resource.
fn write_epub_with_cover(path: &Path) {
    write_epub(
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
fn write_sparse_epub(path: &Path) {
    write_epub(
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

#[test]
fn acquires_complete_payload_free_epub_declarations() {
    let temp_dir = temp_test_dir("complete");
    let epub_path = temp_dir.join("book.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_epub_with_cover(&epub_path);

    let declarations = EpubDeclarations::acquire(&epub_path)
        .expect("complete EPUB declarations should be acquired");

    assert_eq!(declarations.title(), Some("Retained Title"));
    assert_eq!(declarations.creator(), Some("Retained Creator"));
    assert_eq!(declarations.cover_id(), Some("cover"));
    let cover = declarations
        .resources()
        .iter()
        .find(|resource| resource.id() == "cover")
        .expect("cover declaration should be retained");
    assert_eq!(cover.path(), Path::new("OEBPS/cover.png"));
    assert_eq!(cover.mime(), "image/png");

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn sparse_epub_declarations_are_a_successful_acquisition() {
    let temp_dir = temp_test_dir("sparse");
    let epub_path = temp_dir.join("book.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_sparse_epub(&epub_path);

    let declarations = EpubDeclarations::acquire(&epub_path)
        .expect("sparse EPUB declarations should still be acquired");

    assert_eq!(declarations.title(), None);
    assert_eq!(declarations.creator(), None);
    assert_eq!(declarations.cover_id(), None);
    assert!(declarations.resources().is_empty());

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}
