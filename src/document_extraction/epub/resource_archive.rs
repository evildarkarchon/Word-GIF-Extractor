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

    /// Returns the manifest path resolved against the archive root, used for diagnostics
    /// and format evidence; a reference escaping the root keeps the `epub` crate's spelling.
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
    /// The reference's `..` segments climb above the archive root. It keeps the
    /// spelling the `epub` crate produced, because there is no resolved spelling.
    OutsideArchive {
        manifest_path: String,
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
            ResourceUnavailable::OutsideArchive { manifest_path } => {
                write!(
                    formatter,
                    "EPUB resource outside the archive: {manifest_path}"
                )
            }
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
                let reference = archive_path(resource.path());
                let (manifest_path, resolution) = match resolve_reference(&reference) {
                    Some(resolved) => {
                        let resolution = match resolve_resource(&archive, &resolved) {
                            Ok(index) => ResourceResolution::Resolved(index),
                            Err(error) => ResourceResolution::Unresolved(error),
                        };
                        (resolved, resolution)
                    }
                    None => (
                        reference.clone(),
                        ResourceResolution::Unresolved(ResourceUnavailable::OutsideArchive {
                            manifest_path: reference,
                        }),
                    ),
                };
                let identity = match resolution {
                    ResourceResolution::Resolved(index) => ResourceIdentityKind::Resolved(index),
                    ResourceResolution::Unresolved(_) => {
                        ResourceIdentityKind::Unresolved(declaration_index)
                    }
                };
                // The resolved spelling already has no dot or empty segments, so it is
                // the sort key as it stands. A resolved reference sorts by the entry
                // name its lookup reached, which is that spelling or its percent-decoded
                // form, so percent-encoded aliases keep sorting where their payload is.
                let sort_path = match &resolution {
                    ResourceResolution::Resolved(index) => archive
                        .name_for_index(*index)
                        .map_or_else(|| manifest_path.clone(), str::to_string),
                    ResourceResolution::Unresolved(_) => manifest_path.clone(),
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

/// Normalises an EPUB manifest path's separators to `/` ahead of reference resolution.
fn archive_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Resolves a separator-normalised reference against the archive root.
///
/// Empty and `.` segments are dropped, so a leading `/` means the archive root, and
/// each `..` removes the segment before it. Returns `None` when a `..` would climb
/// above the root; such a reference is not clamped, because no archive entry is
/// the one it names.
fn resolve_reference(reference: &str) -> Option<String> {
    let mut segments = Vec::new();
    for segment in reference.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    Some(segments.join("/"))
}

/// Looks up a resolved manifest path using exact spelling before a percent-decoded alias.
///
/// `manifest_path` is the spelling [`resolve_reference`] produced; the raw, unresolved
/// spelling is never looked up. Returns the matching ZIP index. After exact lookup
/// fails, malformed escapes and decoded bytes that are not UTF-8 return
/// `InvalidPercentEncoding`; a valid decoded alias that does not exist returns `NotFound`.
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
mod tests;
