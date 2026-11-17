//! Tests for EPUB declaration acquisition.

use super::*;
use crate::test_support::{
    temp_test_dir, write_epub_package, write_epub_with_cover, write_sparse_epub,
};
use std::fs;
use std::path::Path;

#[test]
fn acquires_complete_payload_free_epub_declarations() {
    let temp_dir = temp_test_dir("epub-declarations", "complete");
    let epub_path = temp_dir.join("book.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_epub_with_cover(&epub_path);

    let declarations = EpubDeclarations::acquire(&epub_path)
        .expect("complete EPUB declarations should be acquired");

    assert_eq!(declarations.identifier(), Some("declarations-test"));
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
}

#[test]
fn sparse_epub_declarations_are_a_successful_acquisition() {
    let temp_dir = temp_test_dir("epub-declarations", "sparse");
    let epub_path = temp_dir.join("book.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_sparse_epub(&epub_path);

    let declarations = EpubDeclarations::acquire(&epub_path)
        .expect("sparse EPUB declarations should still be acquired");

    assert_eq!(declarations.title(), None);
    assert_eq!(declarations.creator(), None);
    assert_eq!(declarations.cover_id(), None);
    assert!(declarations.resources().is_empty());
}

/// Pins the identifier the package names over any other identifier it declares.
///
/// Calibre writes a `calibre` UUID identifier alongside the one the package names, and
/// Document selection's duplicate key reads this value, so taking the first identifier
/// in document order would key a book on the wrong one.
#[test]
fn identifier_is_the_one_the_package_names_as_unique() {
    let temp_dir = temp_test_dir("epub-declarations", "unique-identifier");
    let epub_path = temp_dir.join("book.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    let package = br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="2.0" unique-identifier="uuid_id" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:identifier opf:scheme="ISBN">9780000000000</dc:identifier>
    <dc:identifier id="uuid_id" opf:scheme="uuid">urn:uuid:1b4e28ba-2fa1-11d2-883f-0016d3cca427</dc:identifier>
    <dc:title>Book</dc:title>
  </metadata>
  <manifest></manifest>
  <spine></spine>
</package>"#;
    write_epub_package(&epub_path, package, &[]);

    let declarations =
        EpubDeclarations::acquire(&epub_path).expect("the package declarations should be acquired");

    assert_eq!(
        declarations.identifier(),
        Some("urn:uuid:1b4e28ba-2fa1-11d2-883f-0016d3cca427")
    );
}

#[test]
fn retained_declarations_are_returned_without_reading_the_path() {
    let retained = EpubDeclarations {
        identifier: None,
        title: Some("Retained Title".to_string()),
        creator: None,
        cover_id: None,
        resources: Vec::new(),
    };
    // The path does not exist, so any read would fail: success proves none happened.
    let missing = Path::new("retained-declarations-missing.epub");

    let declarations = EpubDeclarations::retained_or_acquire(
        Some(retained.clone()),
        missing,
        &EpubFileDeclarations,
    )
    .expect("retained declarations should be returned as they are");

    assert_eq!(declarations, retained);
}

#[test]
fn absent_declarations_are_acquired_and_report_an_unreadable_path() {
    let temp_dir = temp_test_dir("epub-declarations", "retained-or-acquire");
    let epub_path = temp_dir.join("book.epub");
    let invalid_path = temp_dir.join("invalid.epub");
    fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
    write_epub_with_cover(&epub_path);
    fs::write(&invalid_path, b"not an epub").expect("invalid EPUB should be writable");

    let acquired = EpubDeclarations::retained_or_acquire(None, &epub_path, &EpubFileDeclarations)
        .expect("readable EPUB declarations should be acquired");
    let unreadable =
        EpubDeclarations::retained_or_acquire(None, &invalid_path, &EpubFileDeclarations);

    assert_eq!(acquired.title(), Some("Retained Title"));
    assert!(unreadable.is_err());
}
