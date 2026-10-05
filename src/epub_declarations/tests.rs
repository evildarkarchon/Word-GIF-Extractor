//! Tests for EPUB declaration acquisition.

use super::*;
use crate::test_support::{temp_test_dir, write_epub_with_cover, write_sparse_epub};
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

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}

#[test]
fn retained_declarations_are_returned_without_reading_the_path() {
    let retained = EpubDeclarations {
        title: Some("Retained Title".to_string()),
        creator: None,
        cover_id: None,
        resources: Vec::new(),
    };
    // The path does not exist, so any read would fail: success proves none happened.
    let missing = Path::new("retained-declarations-missing.epub");

    let declarations = EpubDeclarations::retained_or_acquire(Some(retained.clone()), missing)
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

    let acquired = EpubDeclarations::retained_or_acquire(None, &epub_path)
        .expect("readable EPUB declarations should be acquired");
    let unreadable = EpubDeclarations::retained_or_acquire(None, &invalid_path);

    assert_eq!(acquired.title(), Some("Retained Title"));
    assert!(unreadable.is_err());

    fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
}
