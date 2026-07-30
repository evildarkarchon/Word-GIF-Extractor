//! Tests for the EPUB resource archive session.

use super::*;
use crate::test_support::{temp_epub_path, write_zip_archive};
use std::cell::Cell;
use std::fs;
use std::path::Path;

/// Marks the first central-directory entry encrypted so ZIP parsing still succeeds.
fn mark_first_entry_encrypted(path: &Path) {
    let mut archive = fs::read(path).expect("test archive should be readable");
    let central_header = archive
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .expect("fixture should contain a central-directory header");
    archive[central_header + 8] |= 1;
    fs::write(path, archive).expect("test archive corruption should be writable");
}

/// Sentinel used to verify consumer errors retain their concrete identity.
#[derive(Debug)]
struct ConsumerFailure;

impl fmt::Display for ConsumerFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sentinel consumer failure")
    }
}

impl std::error::Error for ConsumerFailure {}

#[test]
fn exact_manifest_path_wins_before_percent_decoded_alias() {
    let path = temp_epub_path("resource-archive", "exact-before-decoded");
    write_zip_archive(
        &path,
        &[
            ("OPS/image%20one.png", b"exact"),
            ("OPS/image one.png", b"decoded"),
        ],
    );
    EpubResourceArchive::open(
        &path,
        &[EpubResourceDeclaration::new(
            "image",
            Path::new("OPS/image%20one.png"),
            "image/png",
        )],
        |mut archive| {
            let key = archive.resources()[0].key();
            let acquired = archive
                .acquire(key, |mut payload| {
                    let mut data = Vec::new();
                    payload.read_to_end(&mut data)?;
                    Ok(data)
                })
                .expect("reader operation should succeed");
            let ResourceAcquisition::Acquired(data) = acquired else {
                panic!("resource should be available");
            };

            assert_eq!(data, b"exact");
        },
    )
    .expect("resource archive should open");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn percent_decoded_aliases_share_archive_resource_identity() {
    let path = temp_epub_path("resource-archive", "decoded-alias-identity");
    write_zip_archive(&path, &[("OPS/image one.png", b"shared")]);
    EpubResourceArchive::open(
        &path,
        &[
            EpubResourceDeclaration::new("encoded", Path::new("OPS/image%20one.png"), "image/png"),
            EpubResourceDeclaration::new("decoded", Path::new("OPS/image one.png"), "image/png"),
        ],
        |mut archive| {
            let encoded = &archive.resources()[0];
            let decoded = &archive.resources()[1];
            assert_eq!(encoded.identity(), decoded.identity());
            let encoded_key = encoded.key();
            let acquired = archive
                .acquire(encoded_key, |mut payload| {
                    let mut data = Vec::new();
                    payload.read_to_end(&mut data)?;
                    Ok(data)
                })
                .expect("reader operation should succeed");
            let ResourceAcquisition::Acquired(data) = acquired else {
                panic!("encoded alias should resolve");
            };
            assert_eq!(data, b"shared");
        },
    )
    .expect("resource archive should open");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn unresolved_duplicate_manifest_paths_have_distinct_archive_resource_identities() {
    let path = temp_epub_path("resource-archive", "unresolved-identity");
    write_zip_archive(&path, &[("OPS/other.png", b"other")]);
    EpubResourceArchive::open(
        &path,
        &[
            EpubResourceDeclaration::new("first", Path::new("OPS/missing.png"), "image/png"),
            EpubResourceDeclaration::new("second", Path::new("OPS/missing.png"), "image/png"),
        ],
        |archive| {
            assert_ne!(
                archive.resources()[0].identity(),
                archive.resources()[1].identity()
            );
        },
    )
    .expect("resource archive should open despite unresolved resources");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn resources_are_ordered_by_resolved_path_with_manifest_fallback() {
    let path = temp_epub_path("resource-archive", "deterministic-order");
    write_zip_archive(&path, &[("OPS/z.png", b"z"), ("OPS/a one.png", b"a")]);
    EpubResourceArchive::open(
        &path,
        &[
            EpubResourceDeclaration::new("z", Path::new("OPS/z.png"), "image/png"),
            EpubResourceDeclaration::new("a", Path::new("OPS/a%20one.png"), "image/png"),
            EpubResourceDeclaration::new("missing", Path::new("OPS/missing.png"), "image/png"),
        ],
        |archive| {
            let ids: Vec<_> = archive.resources().iter().map(EpubResource::id).collect();
            assert_eq!(ids, ["a", "missing", "z"]);
        },
    )
    .expect("resource archive should open");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn resource_order_uses_manifest_id_ties_and_retains_equal_declaration_order() {
    let path = temp_epub_path("resource-archive", "deterministic-ties");
    write_zip_archive(&path, &[("OPS/resolved.png", b"resolved")]);
    EpubResourceArchive::open(
        &path,
        &[
            EpubResourceDeclaration::new("resolved-b", Path::new("OPS/resolved.png"), "resolved-b"),
            EpubResourceDeclaration::new(
                "unresolved-b",
                Path::new("OPS/missing.png"),
                "unresolved-b",
            ),
            EpubResourceDeclaration::new("equal", Path::new("OPS/missing.png"), "equal-first"),
            EpubResourceDeclaration::new("resolved-a", Path::new("OPS/resolved.png"), "resolved-a"),
            EpubResourceDeclaration::new("equal", Path::new("OPS/missing.png"), "equal-second"),
            EpubResourceDeclaration::new(
                "unresolved-a",
                Path::new("OPS/missing.png"),
                "unresolved-a",
            ),
        ],
        |archive| {
            let ordered: Vec<_> = archive
                .resources()
                .iter()
                .map(|resource| (resource.id(), resource.mime()))
                .collect();
            assert_eq!(
                ordered,
                [
                    ("equal", "equal-first"),
                    ("equal", "equal-second"),
                    ("unresolved-a", "unresolved-a"),
                    ("unresolved-b", "unresolved-b"),
                    ("resolved-a", "resolved-a"),
                    ("resolved-b", "resolved-b"),
                ]
            );
        },
    )
    .expect("resource archive should open");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn invalid_percent_encoded_path_is_retained_as_typed_acquisition_failure() {
    let path = temp_epub_path("resource-archive", "invalid-percent-encoding");
    write_zip_archive(&path, &[("OPS/other.png", b"other")]);
    EpubResourceArchive::open(
        &path,
        &[EpubResourceDeclaration::new(
            "invalid",
            Path::new("OPS/image%FF.png"),
            "image/png",
        )],
        |mut archive| {
            let key = archive.resources()[0].key();
            let unavailable = archive
                .acquire(key, |_| -> Result<()> {
                    panic!("unresolved resource must not invoke the reader operation")
                })
                .expect("reader operation should not fail");

            assert!(matches!(
                unavailable,
                ResourceAcquisition::Unavailable(
                    ResourceUnavailable::InvalidPercentEncoding { .. }
                )
            ));
        },
    )
    .expect("resource archive should open despite unresolved resources");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn malformed_percent_escape_is_retained_as_typed_acquisition_failure() {
    let path = temp_epub_path("resource-archive", "malformed-percent-escape");
    write_zip_archive(&path, &[("OPS/other.png", b"other")]);
    EpubResourceArchive::open(
        &path,
        &[EpubResourceDeclaration::new(
            "invalid",
            Path::new("OPS/image%ZZ.png"),
            "image/png",
        )],
        |mut archive| {
            let key = archive.resources()[0].key();
            let unavailable = archive
                .acquire(key, |_| -> Result<()> {
                    panic!("unresolved resource must not invoke the reader operation")
                })
                .expect("reader operation should not fail");

            assert!(matches!(
                unavailable,
                ResourceAcquisition::Unavailable(
                    ResourceUnavailable::InvalidPercentEncoding { .. }
                )
            ));
        },
    )
    .expect("resource archive should open despite unresolved resources");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn unopenable_resource_is_nonfatal_and_does_not_invoke_its_consumer() {
    let path = temp_epub_path("resource-archive", "unopenable-entry");
    write_zip_archive(&path, &[("OPS/image.png", b"payload")]);
    mark_first_entry_encrypted(&path);

    EpubResourceArchive::open(
        &path,
        &[EpubResourceDeclaration::new(
            "image",
            Path::new("OPS/image.png"),
            "image/png",
        )],
        |mut archive| {
            let key = archive.resources()[0].key();
            let acquisition = archive
                .acquire(key, |_| -> Result<()> {
                    panic!("unopenable resource must not invoke the consumer")
                })
                .expect("entry-open failure should remain non-fatal");
            assert!(matches!(
                acquisition,
                ResourceAcquisition::Unavailable(ResourceUnavailable::EntryOpen { .. })
            ));
        },
    )
    .expect("catalog construction should remain payload-free");

    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn consumer_failure_propagates_with_its_concrete_error_identity() {
    let path = temp_epub_path("resource-archive", "consumer-failure");
    write_zip_archive(&path, &[("OPS/image.png", b"payload")]);

    let error = EpubResourceArchive::open(
        &path,
        &[EpubResourceDeclaration::new(
            "image",
            Path::new("OPS/image.png"),
            "image/png",
        )],
        |mut archive| {
            let key = archive.resources()[0].key();
            archive
                .acquire(key, |_| -> Result<()> { Err(ConsumerFailure.into()) })
                .expect_err("consumer failure should remain fatal")
        },
    )
    .expect("resource archive should open");

    assert!(error.downcast_ref::<ConsumerFailure>().is_some());
    fs::remove_file(path).expect("temporary EPUB should be removable");
}

#[test]
fn catalog_acquisition_is_lazy_repeatable_and_keyed_to_its_session() {
    let path = temp_epub_path("resource-archive", "lazy-repeatable");
    write_zip_archive(&path, &[("OPS/image.png", b"payload")]);
    let consumer_calls = Cell::new(0);

    EpubResourceArchive::open(
        &path,
        &[EpubResourceDeclaration::new(
            "image",
            Path::new("OPS/image.png"),
            "image/png",
        )],
        |mut session| {
            let resource = &session.resources()[0];
            let key = resource.key();
            assert_eq!(consumer_calls.get(), 0);

            for _ in 0..2 {
                let acquisition = session
                    .acquire(key, |mut payload| {
                        consumer_calls.set(consumer_calls.get() + 1);
                        let mut data = Vec::new();
                        payload.read_to_end(&mut data)?;
                        Ok(data)
                    })
                    .expect("consumer operation should succeed");
                let ResourceAcquisition::Acquired(data) = acquisition else {
                    panic!("resolved resource should be acquired");
                };
                assert_eq!(data, b"payload");
            }
        },
    )
    .expect("resource archive session should open");

    assert_eq!(consumer_calls.get(), 2);
    fs::remove_file(path).expect("temporary EPUB should be removable");
}
