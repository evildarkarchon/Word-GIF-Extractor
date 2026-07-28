//! EPUB resource archive resolution, identity, ordering, and scoped acquisition.

use anyhow::{Context, Result};
use percent_encoding::percent_decode;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::marker::PhantomData;
use std::path::Path;
use zip::ZipArchive;

use crate::epub_declarations::EpubResourceDeclaration;

/// Invariant lifetime brand shared only by values from one archive session.
///
/// The higher-ranked session callback chooses the lifetime. Combining its
/// covariant return and contravariant argument positions makes the brand
/// invariant, so branded values from distinct callbacks cannot be mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SessionBrand<'session>(PhantomData<fn(&'session ()) -> &'session ()>);

impl<'session> SessionBrand<'session> {
    /// Creates the private marker copied into every value owned by one session.
    fn new() -> Self {
        Self(PhantomData)
    }
}

/// Opaque lookup key for one resource in its originating archive session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ResourceKey<'session> {
    index: usize,
    _brand: SessionBrand<'session>,
}

/// Stable opaque identity for one archive payload or unresolved declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ArchiveResourceIdentity<'session> {
    kind: ResourceIdentityKind,
    _brand: SessionBrand<'session>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ResourceIdentityKind {
    Resolved(usize),
    Unresolved(usize),
}

/// Document-facing catalog entry branded to one archive session.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct EpubResource<'session> {
    key: ResourceKey<'session>,
    id: String,
    manifest_path: String,
    mime: String,
    identity: ArchiveResourceIdentity<'session>,
}

impl<'session> EpubResource<'session> {
    /// Returns the opaque key used to lazily reacquire this session's resource.
    pub(super) fn key(&self) -> ResourceKey<'session> {
        self.key
    }

    /// Returns the EPUB manifest identifier used for cover metadata matching.
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    /// Returns the normalized manifest path used for diagnostics and format evidence.
    pub(super) fn manifest_path(&self) -> &str {
        &self.manifest_path
    }

    /// Returns the MIME type declared by the EPUB manifest.
    pub(super) fn mime(&self) -> &str {
        &self.mime
    }

    /// Returns the opaque identity shared by references to the same archive payload.
    pub(super) fn identity(&self) -> ArchiveResourceIdentity<'session> {
        self.identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceResolution {
    Resolved(usize),
    Unresolved(ResourceUnavailable),
}

/// Typed reason an EPUB resource cannot supply a scoped reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResourceUnavailable {
    NotFound {
        manifest_path: String,
    },
    InvalidPercentEncoding {
        manifest_path: String,
        detail: String,
    },
    EntryOpen {
        manifest_path: String,
        detail: String,
    },
}

impl fmt::Display for ResourceUnavailable {
    /// Formats an acquisition failure using the manifest path as diagnostic identity.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceUnavailable::NotFound { manifest_path } => {
                write!(formatter, "EPUB resource not found: {manifest_path}")
            }
            ResourceUnavailable::InvalidPercentEncoding {
                manifest_path,
                detail,
            } => write!(
                formatter,
                "Invalid UTF-8 percent encoding in EPUB resource {manifest_path}: {detail}"
            ),
            ResourceUnavailable::EntryOpen {
                manifest_path,
                detail,
            } => write!(
                formatter,
                "Could not open EPUB resource {manifest_path}: {detail}"
            ),
        }
    }
}

impl std::error::Error for ResourceUnavailable {}

/// Named result of attempting to acquire one catalog resource.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ResourceAcquisition<T> {
    Acquired(T),
    Unavailable(ResourceUnavailable),
}

/// Reader whose entry borrow and archive-session brand cannot escape its consumer.
pub(super) struct ResourcePayload<'reader, 'session> {
    reader: &'reader mut dyn Read,
    _brand: SessionBrand<'session>,
}

impl Read for ResourcePayload<'_, '_> {
    /// Reads from the currently acquired ZIP entry.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buffer)
    }
}

/// Concrete entry point for one independently reopened EPUB ZIP session.
pub(super) struct EpubResourceArchive;

/// Branded custody of one archive handle, its eager catalog, and lazy payload access.
pub(super) struct EpubResourceArchiveSession<'session> {
    archive: ZipArchive<fs::File>,
    resources: Vec<EpubResource<'session>>,
    // Resolution stays archive-private, so this vector is built in exact catalog
    // order and ResourceKey::index addresses both collections as one logical slot.
    resolutions: Vec<ResourceResolution>,
    brand: SessionBrand<'session>,
}

impl EpubResourceArchive {
    /// Opens one EPUB ZIP session and runs an operation under a fresh invariant brand.
    ///
    /// The complete deterministic catalog is built eagerly without reading payloads.
    /// The higher-ranked callback prevents keys, identities, catalog entries, and
    /// payload readers from escaping or being mixed with another session.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive file cannot be opened or parsed. Individual
    /// resource failures remain non-fatal catalog facts until lazy acquisition.
    pub(super) fn open<T>(
        input_path: &Path,
        declared_resources: &[EpubResourceDeclaration],
        operation: impl for<'session> FnOnce(EpubResourceArchiveSession<'session>) -> T,
    ) -> Result<T> {
        let file = fs::File::open(input_path)
            .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
        let archive = ZipArchive::new(file)
            .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;
        let mut resources: Vec<_> = declared_resources
            .iter()
            .enumerate()
            .map(|(declaration_index, resource)| {
                let manifest_path = archive_path(resource.path());
                let resolution = match resolve_resource(&archive, &manifest_path) {
                    Ok(index) => ResourceResolution::Resolved(index),
                    Err(error) => ResourceResolution::Unresolved(error),
                };
                let identity = match resolution {
                    ResourceResolution::Resolved(index) => ResourceIdentityKind::Resolved(index),
                    ResourceResolution::Unresolved(_) => {
                        ResourceIdentityKind::Unresolved(declaration_index)
                    }
                };
                let sort_path = match &resolution {
                    ResourceResolution::Resolved(index) => archive
                        .name_for_index(*index)
                        .map(normalized_sort_path)
                        .unwrap_or_else(|| normalized_sort_path(&manifest_path)),
                    ResourceResolution::Unresolved(_) => normalized_sort_path(&manifest_path),
                };
                CatalogSeed {
                    id: resource.id().to_string(),
                    manifest_path,
                    mime: resource.mime().to_string(),
                    identity,
                    resolution,
                    sort_path,
                }
            })
            .collect();
        resources
            .sort_by(|left, right| (&left.sort_path, &left.id).cmp(&(&right.sort_path, &right.id)));

        let brand = SessionBrand::new();
        let mut resolutions = Vec::with_capacity(resources.len());
        let resources = resources
            .into_iter()
            .enumerate()
            .map(|(index, resource)| {
                resolutions.push(resource.resolution);
                EpubResource {
                    key: ResourceKey {
                        index,
                        _brand: brand,
                    },
                    id: resource.id,
                    manifest_path: resource.manifest_path,
                    mime: resource.mime,
                    identity: ArchiveResourceIdentity {
                        kind: resource.identity,
                        _brand: brand,
                    },
                }
            })
            .collect();
        let session = EpubResourceArchiveSession {
            archive,
            resources,
            resolutions,
            brand,
        };

        Ok(operation(session))
    }
}

impl<'session> EpubResourceArchiveSession<'session> {
    /// Returns the deterministically ordered resource catalog.
    pub(super) fn resources(&self) -> &[EpubResource<'session>] {
        &self.resources
    }

    /// Lazily acquires one keyed resource and runs a scoped consumer operation.
    ///
    /// Acquisition is repeatable and sequential. Missing, malformed, or unopenable
    /// resources return `ResourceAcquisition::Unavailable` without invoking the
    /// consumer; consumer errors propagate unchanged through the outer result.
    ///
    /// # Errors
    ///
    /// Returns the consumer operation's error unchanged.
    pub(super) fn acquire<T>(
        &mut self,
        key: ResourceKey<'session>,
        operation: impl for<'reader> FnOnce(ResourcePayload<'reader, 'session>) -> Result<T>,
    ) -> Result<ResourceAcquisition<T>> {
        let resource = &self.resources[key.index];
        let index = match &self.resolutions[key.index] {
            ResourceResolution::Resolved(index) => *index,
            ResourceResolution::Unresolved(error) => {
                return Ok(ResourceAcquisition::Unavailable(error.clone()));
            }
        };
        let mut entry = match self.archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => {
                return Ok(ResourceAcquisition::Unavailable(
                    ResourceUnavailable::EntryOpen {
                        manifest_path: resource.manifest_path.clone(),
                        detail: error.to_string(),
                    },
                ));
            }
        };
        let payload = ResourcePayload {
            reader: &mut entry,
            _brand: self.brand,
        };
        operation(payload).map(ResourceAcquisition::Acquired)
    }
}

struct CatalogSeed {
    id: String,
    manifest_path: String,
    mime: String,
    identity: ResourceIdentityKind,
    resolution: ResourceResolution,
    sort_path: String,
}

/// Converts an EPUB manifest path to the ZIP lookup spelling used by the resource archive.
fn archive_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Builds a stable lexical sort key without changing the ZIP lookup path.
fn normalized_sort_path(path: &str) -> String {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Resolves a manifest path using exact spelling before a percent-decoded alias.
///
/// Returns the matching ZIP index. After exact lookup fails, malformed escapes
/// and decoded bytes that are not UTF-8 return `InvalidPercentEncoding`; a
/// valid decoded alias that does not exist returns `NotFound`.
fn resolve_resource(
    archive: &ZipArchive<fs::File>,
    manifest_path: &str,
) -> std::result::Result<usize, ResourceUnavailable> {
    if let Some(index) = archive.index_for_name(manifest_path) {
        return Ok(index);
    }

    validate_percent_escapes(manifest_path)?;
    let decoded = percent_decode(manifest_path.as_bytes())
        .decode_utf8()
        .map_err(|error| ResourceUnavailable::InvalidPercentEncoding {
            manifest_path: manifest_path.to_string(),
            detail: error.to_string(),
        })?;
    archive
        .index_for_name(&decoded)
        .ok_or_else(|| ResourceUnavailable::NotFound {
            manifest_path: manifest_path.to_string(),
        })
}

/// Validates that every percent marker starts one complete hexadecimal triplet.
///
/// Returns `InvalidPercentEncoding` with the malformed byte position. Literal
/// percent signs are accepted only when exact ZIP lookup already succeeded.
fn validate_percent_escapes(manifest_path: &str) -> std::result::Result<(), ResourceUnavailable> {
    let bytes = manifest_path.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'%' {
            continue;
        }
        let valid = bytes
            .get(index + 1..index + 3)
            .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit));
        if !valid {
            return Err(ResourceUnavailable::InvalidPercentEncoding {
                manifest_path: manifest_path.to_string(),
                detail: format!(
                    "percent escape at byte {index} must contain two hexadecimal digits"
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    /// Returns a unique local filesystem path for one resource archive test.
    fn temp_epub_path(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-resource-archive-{test_name}-{}-{nanos}.epub",
            std::process::id()
        ))
    }

    /// Writes a minimal ZIP payload containing the supplied resource entries.
    fn write_archive(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut archive = zip::ZipWriter::new(file);
        for (name, data) in entries {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .expect("test resource entry should start");
            archive
                .write_all(data)
                .expect("test resource payload should be writable");
        }
        archive.finish().expect("test EPUB should finish");
    }

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
        let path = temp_epub_path("exact-before-decoded");
        write_archive(
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
        let path = temp_epub_path("decoded-alias-identity");
        write_archive(&path, &[("OPS/image one.png", b"shared")]);
        EpubResourceArchive::open(
            &path,
            &[
                EpubResourceDeclaration::new(
                    "encoded",
                    Path::new("OPS/image%20one.png"),
                    "image/png",
                ),
                EpubResourceDeclaration::new(
                    "decoded",
                    Path::new("OPS/image one.png"),
                    "image/png",
                ),
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
        let path = temp_epub_path("unresolved-identity");
        write_archive(&path, &[("OPS/other.png", b"other")]);
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
        let path = temp_epub_path("deterministic-order");
        write_archive(&path, &[("OPS/z.png", b"z"), ("OPS/a one.png", b"a")]);
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
        let path = temp_epub_path("deterministic-ties");
        write_archive(&path, &[("OPS/resolved.png", b"resolved")]);
        EpubResourceArchive::open(
            &path,
            &[
                EpubResourceDeclaration::new(
                    "resolved-b",
                    Path::new("OPS/resolved.png"),
                    "resolved-b",
                ),
                EpubResourceDeclaration::new(
                    "unresolved-b",
                    Path::new("OPS/missing.png"),
                    "unresolved-b",
                ),
                EpubResourceDeclaration::new("equal", Path::new("OPS/missing.png"), "equal-first"),
                EpubResourceDeclaration::new(
                    "resolved-a",
                    Path::new("OPS/resolved.png"),
                    "resolved-a",
                ),
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
        let path = temp_epub_path("invalid-percent-encoding");
        write_archive(&path, &[("OPS/other.png", b"other")]);
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
        let path = temp_epub_path("malformed-percent-escape");
        write_archive(&path, &[("OPS/other.png", b"other")]);
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
        let path = temp_epub_path("unopenable-entry");
        write_archive(&path, &[("OPS/image.png", b"payload")]);
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
        let path = temp_epub_path("consumer-failure");
        write_archive(&path, &[("OPS/image.png", b"payload")]);

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
        let path = temp_epub_path("lazy-repeatable");
        write_archive(&path, &[("OPS/image.png", b"payload")]);
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
}
