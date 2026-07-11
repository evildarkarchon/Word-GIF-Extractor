//! EPUB resource archive resolution, identity, ordering, and scoped acquisition.

use anyhow::{Context, Result};
use percent_encoding::percent_decode;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::epub_declarations::EpubResourceDeclaration;

/// Stable opaque identity for one archive payload or unresolved reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ArchiveResourceIdentity(ResourceIdentityKind);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ResourceIdentityKind {
    Resolved(usize),
    Unresolved(String),
}

/// Owned document-facing descriptor for one EPUB manifest resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EpubResource {
    id: String,
    manifest_path: String,
    mime: String,
    identity: ArchiveResourceIdentity,
    resolution: ResourceResolution,
    sort_path: String,
}

impl EpubResource {
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
    pub(super) fn identity(&self) -> &ArchiveResourceIdentity {
        &self.identity
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

/// Ordered EPUB manifest resources backed by one independently opened ZIP archive.
pub(super) struct EpubResourceArchive {
    archive: ZipArchive<fs::File>,
    resources: Vec<EpubResource>,
}

impl EpubResourceArchive {
    /// Opens the EPUB ZIP archive and resolves supplied manifest resources.
    ///
    /// Failure to open or parse the ZIP archive is fatal. Individual unresolved
    /// resources remain in the catalog and fail only when acquired.
    pub(super) fn open(
        input_path: &Path,
        declared_resources: &[EpubResourceDeclaration],
    ) -> Result<Self> {
        let file = fs::File::open(input_path)
            .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
        let archive = ZipArchive::new(file)
            .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;
        let mut resources: Vec<_> = declared_resources
            .iter()
            .map(|resource| {
                let manifest_path = archive_path(resource.path());
                let resolution = match resolve_resource(&archive, &manifest_path) {
                    Ok(index) => ResourceResolution::Resolved(index),
                    Err(error) => ResourceResolution::Unresolved(error),
                };
                let identity = match &resolution {
                    ResourceResolution::Resolved(index) => {
                        ArchiveResourceIdentity(ResourceIdentityKind::Resolved(*index))
                    }
                    ResourceResolution::Unresolved(_) => ArchiveResourceIdentity(
                        ResourceIdentityKind::Unresolved(manifest_path.clone()),
                    ),
                };
                let sort_path = match &resolution {
                    ResourceResolution::Resolved(index) => archive
                        .name_for_index(*index)
                        .map(normalized_sort_path)
                        .unwrap_or_else(|| normalized_sort_path(&manifest_path)),
                    ResourceResolution::Unresolved(_) => normalized_sort_path(&manifest_path),
                };
                EpubResource {
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

        Ok(Self { archive, resources })
    }

    /// Returns the deterministically ordered resource catalog.
    pub(super) fn resources(&self) -> &[EpubResource] {
        &self.resources
    }

    /// Runs one reader operation while the selected ZIP entry borrow is scoped.
    ///
    /// Resource lookup and entry-open failures are returned as typed acquisition
    /// outcomes. Errors from the reader operation itself remain fatal to its caller.
    pub(super) fn with_reader<T>(
        &mut self,
        resource: &EpubResource,
        operation: impl FnOnce(&mut dyn Read) -> Result<T>,
    ) -> Result<std::result::Result<T, ResourceUnavailable>> {
        let index = match &resource.resolution {
            ResourceResolution::Resolved(index) => *index,
            ResourceResolution::Unresolved(error) => return Ok(Err(error.clone())),
        };
        let mut entry = match self.archive.by_index(index) {
            Ok(entry) => entry,
            Err(error) => {
                return Ok(Err(ResourceUnavailable::EntryOpen {
                    manifest_path: resource.manifest_path.clone(),
                    detail: error.to_string(),
                }));
            }
        };
        operation(&mut entry).map(Ok)
    }
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
        let mut archive = EpubResourceArchive::open(
            &path,
            &[EpubResourceDeclaration::new(
                "image",
                Path::new("OPS/image%20one.png"),
                "image/png",
            )],
        )
        .expect("resource archive should open");
        let resource = archive.resources()[0].clone();

        let acquired = archive
            .with_reader(&resource, |reader| {
                let mut data = Vec::new();
                reader.read_to_end(&mut data)?;
                Ok(data)
            })
            .expect("reader operation should succeed")
            .expect("resource should be available");

        assert_eq!(acquired, b"exact");

        fs::remove_file(path).expect("temporary EPUB should be removable");
    }

    #[test]
    fn percent_decoded_aliases_share_archive_resource_identity() {
        let path = temp_epub_path("decoded-alias-identity");
        write_archive(&path, &[("OPS/image one.png", b"shared")]);
        let mut archive = EpubResourceArchive::open(
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
        )
        .expect("resource archive should open");
        let encoded = archive.resources()[0].clone();
        let decoded = archive.resources()[1].clone();

        assert_eq!(encoded.identity, decoded.identity);
        let acquired = archive
            .with_reader(&encoded, |reader| {
                let mut data = Vec::new();
                reader.read_to_end(&mut data)?;
                Ok(data)
            })
            .expect("reader operation should succeed")
            .expect("encoded alias should resolve");
        assert_eq!(acquired, b"shared");

        fs::remove_file(path).expect("temporary EPUB should be removable");
    }

    #[test]
    fn resources_are_ordered_by_resolved_path_with_manifest_fallback() {
        let path = temp_epub_path("deterministic-order");
        write_archive(&path, &[("OPS/z.png", b"z"), ("OPS/a one.png", b"a")]);
        let archive = EpubResourceArchive::open(
            &path,
            &[
                EpubResourceDeclaration::new("z", Path::new("OPS/z.png"), "image/png"),
                EpubResourceDeclaration::new("a", Path::new("OPS/a%20one.png"), "image/png"),
                EpubResourceDeclaration::new("missing", Path::new("OPS/missing.png"), "image/png"),
            ],
        )
        .expect("resource archive should open");

        let ids: Vec<_> = archive
            .resources()
            .iter()
            .map(|resource| resource.id.as_str())
            .collect();
        assert_eq!(ids, ["a", "missing", "z"]);

        fs::remove_file(path).expect("temporary EPUB should be removable");
    }

    #[test]
    fn invalid_percent_encoded_path_is_retained_as_typed_acquisition_failure() {
        let path = temp_epub_path("invalid-percent-encoding");
        write_archive(&path, &[("OPS/other.png", b"other")]);
        let mut archive = EpubResourceArchive::open(
            &path,
            &[EpubResourceDeclaration::new(
                "invalid",
                Path::new("OPS/image%FF.png"),
                "image/png",
            )],
        )
        .expect("resource archive should open despite unresolved resources");
        let resource = archive.resources()[0].clone();

        let unavailable = archive
            .with_reader(&resource, |_| -> Result<()> {
                panic!("unresolved resource must not invoke the reader operation")
            })
            .expect("reader operation should not fail")
            .expect_err("invalid path should remain unavailable");

        assert!(matches!(
            unavailable,
            ResourceUnavailable::InvalidPercentEncoding { .. }
        ));

        fs::remove_file(path).expect("temporary EPUB should be removable");
    }

    #[test]
    fn malformed_percent_escape_is_retained_as_typed_acquisition_failure() {
        let path = temp_epub_path("malformed-percent-escape");
        write_archive(&path, &[("OPS/other.png", b"other")]);
        let mut archive = EpubResourceArchive::open(
            &path,
            &[EpubResourceDeclaration::new(
                "invalid",
                Path::new("OPS/image%ZZ.png"),
                "image/png",
            )],
        )
        .expect("resource archive should open despite unresolved resources");
        let resource = archive.resources()[0].clone();

        let unavailable = archive
            .with_reader(&resource, |_| -> Result<()> {
                panic!("unresolved resource must not invoke the reader operation")
            })
            .expect("reader operation should not fail")
            .expect_err("malformed escape should remain unavailable");

        assert!(matches!(
            unavailable,
            ResourceUnavailable::InvalidPercentEncoding { .. }
        ));

        fs::remove_file(path).expect("temporary EPUB should be removable");
    }
}
