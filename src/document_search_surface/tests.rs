//! Conformance tests for the real Document search surface.
//!
//! Their subject is that [`FilesystemSearchSurface`] reports what `std::fs` and
//! `walkdir` report, not that Document selection selects correctly — selection's
//! own behaviour is asserted against a declared surface with no disk behind it.
//! ADR-0008 keeps exactly the cases only an operating system can settle here:
//! the two link kinds, the two link positions, and the absent path.
//!
//! One case is deliberately absent. An inspectable object that is neither a file
//! nor a directory — a fifo or a socket — cannot be staged portably on Windows,
//! so `InspectedKind::Other` is covered against the declared surface only.

use super::*;
use crate::test_support::{
    create_directory_link, create_file_symlink, remove_directory_link, remove_file_symlink,
    temp_test_dir,
};
use std::fs;

/// Collects one whole traversal, so a test can assert on what it did not yield.
fn traversed_paths(surface: &FilesystemSearchSurface, root: &Path) -> Vec<PathBuf> {
    let mut traversal = surface.traverse(root);
    let mut paths = Vec::new();
    while let Some(entry) = traversal.next_entry() {
        paths.push(
            entry
                .unwrap_or_else(|failure| {
                    panic!("traversal should not fail: {:?}", failure.error())
                })
                .into_path(),
        );
    }
    paths
}

#[test]
fn absent_path_is_not_found_by_either_inspection() {
    let absent = temp_test_dir("document-search-surface", "absent").join("missing.docx");

    assert_eq!(
        FilesystemSearchSurface
            .inspect(&absent)
            .expect_err("an absent path should not inspect")
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        FilesystemSearchSurface
            .inspect_without_following(&absent)
            .expect_err("an absent path should not inspect without following")
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn link_whose_target_is_gone_inspects_only_without_following() {
    let temp_dir = temp_test_dir("document-search-surface", "broken-link");
    let removed_target = temp_dir.join("removed-target");
    let broken_link = temp_dir.join("broken-link");
    fs::create_dir_all(&removed_target).expect("link target should be creatable");
    create_directory_link(&removed_target, &broken_link);
    fs::remove_dir(&removed_target).expect("link target should be removable");

    // The pair of answers is the whole point: not-found followed, present
    // unfollowed, which is how a broken link stays distinct from an absent path.
    assert_eq!(
        FilesystemSearchSurface
            .inspect(&broken_link)
            .expect_err("a link whose target is gone should not inspect")
            .kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        FilesystemSearchSurface
            .inspect_without_following(&broken_link)
            .expect("a link whose target is gone should still be inspectable unfollowed"),
        InspectedKind::Other
    );

    remove_directory_link(&broken_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn requested_directory_link_is_followed_by_inspection_and_traversal() {
    let temp_dir = temp_test_dir("document-search-surface", "requested-directory-link");
    let target = temp_dir.join("target");
    let requested_link = temp_dir.join("requested-link");
    fs::create_dir_all(&target).expect("link target should be creatable");
    fs::write(target.join("linked.docx"), []).expect("linked DOCX should be writable");
    create_directory_link(&target, &requested_link);

    assert_eq!(
        FilesystemSearchSurface
            .inspect(&requested_link)
            .expect("a requested directory link should inspect"),
        InspectedKind::Directory
    );
    // Entries are named under the link the caller asked about, not under its target.
    assert_eq!(
        traversed_paths(&FilesystemSearchSurface, &requested_link),
        vec![requested_link.join("linked.docx")]
    );

    remove_directory_link(&requested_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn nested_directory_link_is_enumerated_but_not_descended_into() {
    let temp_dir = temp_test_dir("document-search-surface", "nested-directory-link");
    let requested_directory = temp_dir.join("requested");
    let target_directory = temp_dir.join("target");
    let nested_link = requested_directory.join("nested-link");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&target_directory).expect("link target should be creatable");
    fs::write(target_directory.join("outside.docx"), [])
        .expect("linked-directory DOCX should be writable");
    create_directory_link(&target_directory, &nested_link);

    // The link is yielded, its contents are not, and it does not enumerate as a
    // directory — which is what stops a traversal widening its scope through one.
    let mut traversal = FilesystemSearchSurface.traverse(&requested_directory);
    let entry = traversal
        .next_entry()
        .expect("the nested link should be enumerated")
        .unwrap_or_else(|failure| panic!("traversal should not fail: {:?}", failure.error()));
    assert_eq!(entry.path(), nested_link);
    assert!(!entry.is_directory());
    assert!(traversal.next_entry().is_none());
    assert_eq!(
        FilesystemSearchSurface
            .inspect(&nested_link)
            .expect("a nested directory link should still inspect"),
        InspectedKind::Directory
    );

    remove_directory_link(&nested_link);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

#[test]
fn nested_file_link_inspects_as_a_file() {
    let temp_dir = temp_test_dir("document-search-surface", "nested-file-link");
    let requested_directory = temp_dir.join("requested");
    let target_directory = temp_dir.join("targets");
    let target = target_directory.join("target.docx");
    let linked_document = requested_directory.join("linked.docx");
    fs::create_dir_all(&requested_directory).expect("requested directory should be creatable");
    fs::create_dir_all(&target_directory).expect("target directory should be creatable");
    fs::write(&target, []).expect("linked DOCX target should be writable");
    if !create_file_symlink(&target, &linked_document) {
        eprintln!("skipping file-link inspection: Windows denied symlink creation");
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
        return;
    }

    assert_eq!(
        FilesystemSearchSurface
            .inspect(&linked_document)
            .expect("a nested file link should inspect"),
        InspectedKind::File
    );
    assert_eq!(
        FilesystemSearchSurface
            .inspect_without_following(&linked_document)
            .expect("a nested file link should inspect without following"),
        InspectedKind::Other
    );
    assert_eq!(
        FilesystemSearchSurface
            .read_directory(&requested_directory)
            .expect("the requested directory should list")
            .collect::<io::Result<Vec<_>>>()
            .expect("every entry should read"),
        vec![linked_document.clone()]
    );

    remove_file_symlink(&linked_document);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}
