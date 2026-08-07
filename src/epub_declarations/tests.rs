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
