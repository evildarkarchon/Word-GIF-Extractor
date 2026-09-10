//! Conformance tests for the real Document search surface.
//!
//! Their subject is that [`FilesystemSearchSurface`] reports what `std::fs` and
//! `walkdir` report, not that Document selection selects correctly — selection's
//! own behaviour is asserted against a declared surface with no disk behind it.
//! ADR-0008 keeps the two link kinds, the two link positions, and the absent path
//! here. ADR-0012 adds direct-listing scope, encounter order, and the failure when
//! a successfully inspected directory is replaced before immediate-child search.
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

/// Guards direct listing's failure when the inspected root becomes a regular file.
#[test]
fn immediate_search_reports_root_replaced_after_inspection_then_exhausts() {
    let temp_dir = temp_test_dir("document-search-surface", "replaced-root");
    let root = temp_dir.join("root");
    fs::create_dir_all(&root).expect("root should be creatable");
    assert_eq!(
        FilesystemSearchSurface.inspect(&root).unwrap(),
        InspectedKind::Directory
    );
    fs::remove_dir(&root).expect("empty root should be removable");
    fs::write(&root, []).expect("root should be replaceable by a file");

    let mut search = FilesystemSearchSurface.search(&root, SearchScope::ImmediateChildren);
    let failure = search
        .next_entry()
        .expect("opening a file must fail")
        .err()
        .expect("the search must report a failure, not an entry");
    assert_eq!(failure.path(), Some(root.as_path()));
    assert_eq!(failure.depth(), 0);
    assert!(search.next_entry().is_none());
    drop(search);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

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
    assert_eq!(
        immediate_paths(&FilesystemSearchSurface, &requested_link),
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
    assert!(!entry.may_descend());
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
        immediate_paths(&FilesystemSearchSurface, &requested_directory),
        vec![linked_document.clone()]
    );

    remove_file_symlink(&linked_document);
    fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
}

/// Collects direct children while checking that acquisition never schedules descent.
fn immediate_paths(surface: &FilesystemSearchSurface, root: &Path) -> Vec<PathBuf> {
    let mut search = surface.search(root, SearchScope::ImmediateChildren);
    let mut paths = Vec::new();
    while let Some(entry) = search.next_entry() {
        let entry =
            entry.unwrap_or_else(|failure| panic!("listing should succeed: {}", failure.error()));
        assert_eq!(entry.depth(), 1);
        assert!(!entry.may_descend());
        paths.push(entry.into_path());
    }
    paths
}

/// Checks direct-child scope and OS encounter order in an unchanged fixture.
#[test]
fn immediate_search_retains_listing_order_and_excludes_grandchildren() {
    let root = temp_test_dir("document-search-surface", "immediate-scope-order");
    fs::create_dir_all(root.join("nested")).expect("nested directory should be creatable");
    fs::write(root.join("z.docx"), []).unwrap();
    fs::write(root.join("a.epub"), []).unwrap();
    fs::write(root.join("nested/grandchild.docx"), []).unwrap();

    // Compare with the OS listing in this unchanged fixture only. Independent
    // filesystem searches do not promise a stable order across executions.
    let listed = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    let paths = immediate_paths(&FilesystemSearchSurface, &root);
    assert_eq!(paths, listed);
    assert_eq!(paths.len(), 3);
    assert!(paths.contains(&root.join("nested")));
    assert!(paths.contains(&root.join("z.docx")));
    assert!(paths.contains(&root.join("a.epub")));
    assert!(!paths.contains(&root.join("nested/grandchild.docx")));

    fs::remove_dir_all(root).expect("temporary test directory should be removable");
}
