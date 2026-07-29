//! EPUB file processing module

#[path = "epub/resource_archive.rs"]
mod resource_archive;

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use super::DocumentExtractionPolicy;
use crate::document_selection::SelectedEpub;
use crate::epub_declarations::EpubDeclarations;
use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWriteOutcome, ImageWritePipeline,
    ImageWriteRequest, ImageWriteResult, RequiredCoverWriteOutcome, RequiredCoverWriteRequest,
};

use self::resource_archive::{
    ArchiveResourceIdentity, EpubResourceArchive, EpubResourceArchiveSession, ResourceAcquisition,
    ResourceKey,
};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// Consumes one authoritative Selected EPUB and applies its Document extraction policy.
///
/// Retained declarations remain authoritative; when selection retained none, extraction
/// retries declaration acquisition without revising the selected output placement or base
/// name. Resource payloads are read through an independently opened archive with scoped
/// readers, and any partial Image write facts are preserved by the returned outcome.
///
/// # Errors
///
/// Returns an error when EPUB declarations cannot be acquired, the resource archive cannot
/// be opened, or collision-safe output emission cannot create or complete a file.
pub(super) fn extract(
    document: SelectedEpub,
    policy: DocumentExtractionPolicy,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let (input_path, output_dir, base_name, retained_declarations) =
        document.into_extraction_parts();
    let acquired_declarations;
    let declarations = match retained_declarations.as_ref() {
        Some(declarations) => declarations,
        None => {
            acquired_declarations =
                EpubDeclarations::acquire(&input_path).map_err(anyhow::Error::new)?;
            &acquired_declarations
        }
    };
    // ADR-0001 keeps payload acquisition on an independent direct ZIP handle,
    // even when declaration facts were retained earlier by Document selection.
    EpubResourceArchive::open(&input_path, declarations.resources(), |mut archive| {
        let plan = archive
            .resources()
            .iter()
            .map(EpubImagePlan::from_catalog)
            .collect::<Vec<_>>();
        match policy {
            DocumentExtractionPolicy::NormalImages => extract_all_images(
                &mut archive,
                &plan,
                &HashSet::new(),
                &output_dir,
                &base_name,
                pipeline,
            ),
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images,
            } => extract_required_cover(
                &mut archive,
                &plan,
                declarations.cover_id(),
                &output_dir,
                &base_name,
                fallback_to_normal_images,
                pipeline,
            ),
        }
    })?
}

/// Session-local extraction plan built only from payload-free catalog facts.
///
/// Keys and identities retain the archive session's invariant brand, while the
/// cloned strings end the catalog borrow before keyed acquisition mutably borrows
/// the session and retain the document-facing facts needed to construct Image sources.
struct EpubImagePlan<'session> {
    key: ResourceKey<'session>,
    identity: ArchiveResourceIdentity<'session>,
    id: String,
    manifest_path: String,
    mime: String,
}

impl<'session> EpubImagePlan<'session> {
    /// Copies one branded catalog entry into the session-local extraction plan.
    fn from_catalog(resource: &resource_archive::EpubResource<'session>) -> Self {
        Self {
            key: resource.key(),
            identity: resource.identity(),
            id: resource.id().to_string(),
            manifest_path: resource.manifest_path().to_string(),
            mime: resource.mime().to_string(),
        }
    }

    /// Builds the normal-image source facts used by shared archive traversal.
    fn normal_source(&self) -> ArchiveImageSource {
        ArchiveImageSource::named(&self.manifest_path).with_mime(&self.mime)
    }

    /// Builds the required-cover source facts used by the cover Image write purpose.
    fn required_cover_source(&self) -> ArchiveImageSource {
        ArchiveImageSource::required_cover(&self.manifest_path, &self.mime)
    }
}

/// Extracts every non-excluded planned resource in deterministic resolved-path order.
///
/// Weak labels are intentional inputs: byte-first discovery can recover images
/// from resources whose extension and MIME type do not identify them. Cover
/// fallback passes attempted Archive resource identities so aliases are not revisited.
///
/// # Errors
///
/// Returns an error only when output emission fails; per-resource lookup and read
/// failures become warning facts and traversal continues.
fn extract_all_images<'session>(
    archive: &mut EpubResourceArchiveSession<'session>,
    plan: &[EpubImagePlan<'session>],
    excluded_identities: &HashSet<ArchiveResourceIdentity<'session>>,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    pipeline.write_from(
        ImageWriteRequest::normal_images(output_base_dir, base_name),
        |visitor| {
            for candidate in plan {
                if excluded_identities.contains(&candidate.identity) {
                    continue;
                }
                visit_resource(archive, candidate, visitor)?;
            }
            Ok(())
        },
    )
}

/// Searches for a cover image by filename when the declared cover identity fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the first matching deterministic manifest candidate.
fn find_cover_by_filename<'resource, 'session>(
    resources: &'resource [EpubImagePlan<'session>],
) -> Option<&'resource EpubImagePlan<'session>> {
    resources.iter().find(|candidate| {
        let path = Path::new(&candidate.manifest_path);
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase());

        file_stem.as_deref() == Some("cover")
            && extension
                .as_deref()
                .is_some_and(|ext| JPEG_EXTENSIONS.contains(&ext))
    })
}

/// Extracts one required EPUB cover and optionally falls back to normal images.
///
/// The declared metadata cover precedes the first deterministic JPEG-family
/// filename candidate. Archive resource identities are recorded before attempts,
/// suppress aliases, and exclude every attempted payload from normal fallback.
/// Only retry dispositions advance; every completed Image write decision is terminal.
///
/// # Errors
///
/// Returns an error when cover or fallback emission fails. Facts from earlier cover
/// attempts and successful normal-image writes remain attached to the failure.
fn extract_required_cover<'session>(
    archive: &mut EpubResourceArchiveSession<'session>,
    resources: &[EpubImagePlan<'session>],
    cover_id: Option<&str>,
    output_base_dir: &Path,
    base_name: &str,
    fallback_to_normal_images: bool,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let metadata_cover = cover_id.and_then(|id| resources.iter().find(|item| item.id == id));
    let filename_cover = find_cover_by_filename(resources);
    let mut attempted_identities = HashSet::new();
    let mut aggregate = ImageWriteResult::default();

    for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
        if !attempted_identities.insert(candidate.identity) {
            continue;
        }

        let outcome = match pipeline.write_required_cover(
            RequiredCoverWriteRequest::new(output_base_dir, base_name),
            |visitor| {
                let source = candidate.required_cover_source();
                let acquisition = archive.acquire(candidate.key, |mut payload| {
                    visitor.visit(source.clone(), &mut payload)
                })?;
                if let ResourceAcquisition::Unavailable(error) = acquisition {
                    visitor.unreadable(source, error)?;
                }
                Ok(())
            },
        ) {
            Ok(outcome) => outcome,
            Err(mut failure) => {
                failure.prepend(aggregate);
                return Err(failure);
            }
        };

        match outcome {
            RequiredCoverWriteOutcome::Retry(result) => aggregate.append(result),
            RequiredCoverWriteOutcome::Completed(result) => {
                aggregate.append(result);
                return Ok(aggregate);
            }
        }
    }

    if fallback_to_normal_images {
        let fallback = match extract_all_images(
            archive,
            resources,
            &attempted_identities,
            output_base_dir,
            base_name,
            pipeline,
        ) {
            Ok(fallback) => fallback,
            Err(mut failure) => {
                failure.prepend(aggregate);
                return Err(failure);
            }
        };
        aggregate.append(fallback);
    }

    Ok(aggregate)
}

/// Visits one keyed normal-image resource while keeping its payload borrow scoped.
///
/// # Errors
///
/// Returns an error when the pipeline cannot emit an accepted image. Resource
/// lookup, open, and read failures are recorded on the visitor and return `Ok(())`.
fn visit_resource<'session>(
    archive: &mut EpubResourceArchiveSession<'session>,
    candidate: &EpubImagePlan<'session>,
    visitor: &mut ArchiveImageVisitor<'_, '_>,
) -> Result<()> {
    let source = candidate.normal_source();
    let acquisition = archive.acquire(candidate.key, |mut payload| {
        visitor.visit(source.clone(), &mut payload)
    })?;
    if let ResourceAcquisition::Unavailable(error) = acquisition {
        visitor.unreadable(source, error);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
    use crate::document_extraction::DocumentExtractionPolicy;
    use crate::document_selection::{
        DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionOptions,
        DocumentSelectionProgress, EpubFilter, SelectedDocument, SelectedEpub, select_documents,
    };
    use crate::image_format::ImageFormat;
    use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
    use std::collections::HashSet;
    use std::fs;
    use std::io::{Cursor, Write};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    const MINIMAL_PNG: &[u8] = b"\x89PNG\r\n\x1A\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1F\x15\xC4\x89";

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-epub-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[derive(Default)]
    struct SilentDocumentSelectionObserver;

    impl DocumentSelectionObserver for SilentDocumentSelectionObserver {
        /// Ignores progress facts that are outside this extraction-focused test seam.
        fn on_document_selection_progress(&mut self, _progress: DocumentSelectionProgress) {}

        /// Ignores diagnostics because the readable fixtures retain their declarations.
        fn on_document_selection_diagnostic(&mut self, _diagnostic: DocumentSelectionDiagnostic) {}
    }

    /// Obtains one owned EPUB handoff through the production Document selection operation.
    fn select_epub(input_path: &Path, output_dir: &Path) -> SelectedEpub {
        let mut observer = SilentDocumentSelectionObserver;
        let input_path = input_path.to_path_buf();
        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&input_path),
                recursive: false,
                output: Some(output_dir),
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        )
        .into_iter()
        .next()
        .expect("EPUB fixture should be selected");

        match selected {
            SelectedDocument::Epub(document) => document,
            SelectedDocument::Docx(_) => panic!("EPUB fixture should retain its selected kind"),
        }
    }

    /// Returns archive acquisition warning sources in their observed order.
    fn acquisition_failure_sources(warnings: &[ImageWriteWarning]) -> Vec<&str> {
        warnings
            .iter()
            .filter_map(|warning| match warning {
                ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, .. } => {
                    Some(source_name.as_str())
                }
                _ => None,
            })
            .collect()
    }

    /// Builds the production Image write pipeline with one validated Conversion policy.
    fn pipeline_with_conversion(
        allowed_formats: HashSet<ImageFormat>,
        target: ConversionTarget,
    ) -> ImageWritePipeline {
        let conversion = ConversionPolicy::try_from(ConversionRequest {
            target,
            quality: None,
            lossless: false,
        })
        .expect("test Conversion policy should be valid");
        ImageWritePipeline::new(ImageWritePolicy::new(
            allowed_formats,
            Some(conversion),
            None,
        ))
    }

    /// Encodes one valid pixel so conversion-focused EPUB tests use production decoders.
    fn encoded_test_png() -> Vec<u8> {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("test PNG should encode");
        encoded.into_inner()
    }

    fn write_minimal_epub(path: &Path, image_href: &str, image_mime: &str, image_data: &[u8]) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        zip.start_file("mimetype", options)
            .expect("mimetype entry should start");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype should be writable");

        zip.start_file("META-INF/container.xml", options)
            .expect("container entry should start");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .expect("container should be writable");

        zip.start_file("OEBPS/content.opf", options)
            .expect("OPF entry should start");
        let opf = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>Magic Test</dc:title>
    <dc:creator>Tester</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="img" href="{image_href}" media-type="{image_mime}"/>
  </manifest>
  <spine>
    <itemref idref="nav"/>
  </spine>
</package>"#
        );
        zip.write_all(opf.as_bytes())
            .expect("OPF should be writable");

        zip.start_file("OEBPS/nav.xhtml", options)
            .expect("nav entry should start");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body><nav></nav></body></html>"#,
        )
        .expect("nav should be writable");

        zip.start_file(format!("OEBPS/{image_href}"), options)
            .expect("image entry should start");
        zip.write_all(image_data)
            .expect("image data should be writable");
        zip.finish().expect("EPUB archive should finish");
    }

    /// Writes an EPUB fixture with independently controlled manifest and ZIP resources.
    fn write_epub_fixture(
        path: &Path,
        manifest_resources: &[(&str, &str, &str, Option<&str>)],
        archive_resources: &[(&str, &[u8])],
    ) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("mimetype", options)
            .expect("mimetype entry should start");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype should be writable");
        zip.start_file("META-INF/container.xml", options)
            .expect("container entry should start");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .expect("container should be writable");

        let resource_items = manifest_resources
            .iter()
            .map(|(id, href, mime, properties)| {
                let properties = properties
                    .map(|value| format!(" properties=\"{value}\""))
                    .unwrap_or_default();
                format!("    <item id=\"{id}\" href=\"{href}\" media-type=\"{mime}\"{properties}/>")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let opf = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier>
    <dc:title>Magic Test</dc:title>
    <dc:creator>Tester</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
{resource_items}
  </manifest>
  <spine><itemref idref="nav"/></spine>
</package>"#
        );
        zip.start_file("OEBPS/content.opf", options)
            .expect("OPF entry should start");
        zip.write_all(opf.as_bytes())
            .expect("OPF should be writable");
        zip.start_file("OEBPS/nav.xhtml", options)
            .expect("nav entry should start");
        zip.write_all(b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body/></html>")
            .expect("nav should be writable");

        for (name, data) in archive_resources {
            zip.start_file(name, options)
                .expect("resource entry should start");
            zip.write_all(data).expect("resource should be writable");
        }
        zip.finish().expect("EPUB archive should finish");
    }

    /// Corrupts one uniquely identifiable stored payload without changing ZIP metadata.
    fn corrupt_stored_payload(path: &Path, payload: &[u8]) {
        let mut archive_bytes = fs::read(path).expect("test EPUB should be readable");
        let payload_start = archive_bytes
            .windows(payload.len())
            .position(|window| window == payload)
            .expect("stored test payload should occur in the EPUB");
        assert_eq!(
            archive_bytes
                .windows(payload.len())
                .filter(|window| *window == payload)
                .count(),
            1,
            "test payload must uniquely identify one archive entry"
        );
        archive_bytes[payload_start] ^= 0xff;
        fs::write(path, archive_bytes).expect("corrupted test EPUB should be writable");
    }

    #[test]
    fn filename_cover_heuristics_are_observable_through_epub_extraction() {
        for (extension, should_match) in [
            ("jpg", true),
            ("jpeg", true),
            ("jpe", true),
            ("jfif", true),
            ("png", false),
            ("gif", false),
            ("webp", false),
            ("bmp", false),
        ] {
            let temp_dir = temp_test_dir(&format!("filename-cover-{extension}"));
            let input_path = temp_dir.join("sample.epub");
            let output_dir = temp_dir.join("out");
            fs::create_dir_all(&output_dir).expect("output directory should be creatable");
            let manifest_path = format!("images/CoVeR.{extension}");
            let archive_path = format!("OEBPS/{manifest_path}");
            write_epub_fixture(
                &input_path,
                &[(
                    "filename-candidate",
                    manifest_path.as_str(),
                    "application/octet-stream",
                    None,
                )],
                &[(archive_path.as_str(), b"\xFF\xD8\xFFcover")],
            );
            let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            ));
            let selected = select_epub(&input_path, &output_dir);

            let result = extract(
                selected,
                DocumentExtractionPolicy::EpubCover {
                    fallback_to_normal_images: false,
                },
                &pipeline,
            )
            .expect("filename cover discovery should complete normally");

            assert_eq!(
                result.counts.extracted,
                usize::from(should_match),
                "unexpected filename-cover decision for .{extension}"
            );
            assert_eq!(
                output_dir.join("Tester - Magic Test.jpg").exists(),
                should_match,
                "unexpected filename-cover output for .{extension}"
            );

            fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
        }
    }

    #[test]
    fn filename_cover_uses_first_deterministic_jpeg_family_candidate() {
        let temp_dir = temp_test_dir("deterministic-filename-cover");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let first_cover = b"\xFF\xD8\xFFfirst";
        let later_cover = b"\xFF\xD8\xFFlater";
        write_epub_fixture(
            &input_path,
            &[
                ("later", "z/cover.jpg", "image/jpeg", None),
                ("first", "a/cover.jpeg", "image/jpeg", None),
            ],
            &[
                ("OEBPS/z/cover.jpg", later_cover),
                ("OEBPS/a/cover.jpeg", first_cover),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            &pipeline,
        )
        .expect("the first deterministic filename candidate should be emitted");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.jpg")).unwrap(),
            first_cover
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn extracts_epub_resource_by_magic_before_declared_extension_and_mime() {
        let temp_dir = temp_test_dir("magic-before-labels");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");

        write_minimal_epub(
            &input_path,
            "images/mislabeled.jpg",
            "image/jpeg",
            MINIMAL_PNG,
        );

        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert!(output_dir.join("Tester - Magic Test.png").exists());
        assert!(!output_dir.join("Tester - Magic Test.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn extracts_epub_resource_by_magic_without_declared_image_hints() {
        let temp_dir = temp_test_dir("magic-without-hints");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");

        write_minimal_epub(
            &input_path,
            "images/mislabeled.bin",
            "application/octet-stream",
            MINIMAL_PNG,
        );

        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 1);
        assert!(output_dir.join("Tester - Magic Test.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn missing_manifest_resource_warns_and_later_image_is_extracted() {
        let temp_dir = temp_test_dir("missing-resource-continues");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                ("missing", "images/a.png", "image/png", None),
                ("valid", "images/b.png", "image/png", None),
            ],
            &[("OEBPS/images/b.png", MINIMAL_PNG)],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("a missing resource should not abort the EPUB");

        assert_eq!(result.counts.extracted, 1);
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name,
                detail,
            }] if source_name == "OEBPS/images/a.png"
                && detail == "EPUB resource not found: OEBPS/images/a.png"
        ));
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
            MINIMAL_PNG
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn epub_batch_output_uses_resolved_path_order() {
        let temp_dir = temp_test_dir("resolved-path-order");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let mut first_by_path = MINIMAL_PNG.to_vec();
        first_by_path.push(1);
        let mut second_by_path = MINIMAL_PNG.to_vec();
        second_by_path.push(2);
        write_epub_fixture(
            &input_path,
            &[
                ("z", "images/z.png", "image/png", None),
                ("a", "images/a.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/z.png", &second_by_path),
                ("OEBPS/images/a.png", &first_by_path),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 2);
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test_1.png")).unwrap(),
            first_by_path
        );
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test_2.png")).unwrap(),
            second_by_path
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn percent_decoded_resource_sorts_by_resolved_zip_path() {
        let temp_dir = temp_test_dir("percent-decoded-sort-order");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let mut first_by_resolved_path = MINIMAL_PNG.to_vec();
        first_by_resolved_path.push(1);
        let mut second_by_resolved_path = MINIMAL_PNG.to_vec();
        second_by_resolved_path.push(2);
        write_epub_fixture(
            &input_path,
            &[
                ("escaped-z", "images/%7A.png", "image/png", None),
                ("plain-a", "images/a.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/z.png", &second_by_resolved_path),
                ("OEBPS/images/a.png", &first_by_resolved_path),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 2);
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test_1.png")).unwrap(),
            first_by_resolved_path
        );
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test_2.png")).unwrap(),
            second_by_resolved_path
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn percent_decoded_manifest_path_falls_back_to_matching_zip_entry() {
        let temp_dir = temp_test_dir("percent-decoded-resource");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[("image", "images/cover%20art.png", "image/png", None)],
            &[("OEBPS/images/cover art.png", MINIMAL_PNG)],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("percent-decoded lookup should preserve EPUB crate behavior");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
            MINIMAL_PNG
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn exact_manifest_path_wins_before_percent_decoded_alias() {
        let temp_dir = temp_test_dir("exact-resource-wins");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let mut exact_payload = MINIMAL_PNG.to_vec();
        exact_payload.push(1);
        let mut decoded_payload = MINIMAL_PNG.to_vec();
        decoded_payload.push(2);
        write_epub_fixture(
            &input_path,
            &[("image", "images/cover%20art.png", "image/png", None)],
            &[
                ("OEBPS/images/cover%20art.png", &exact_payload),
                ("OEBPS/images/cover art.png", &decoded_payload),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect("exact ZIP lookup should take precedence");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
            exact_payload
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn archive_open_failure_after_selection_is_a_fatal_extraction_error() {
        let temp_dir = temp_test_dir("archive-open-failure");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_minimal_epub(&input_path, "images/image.png", "image/png", MINIMAL_PNG);
        let selected = select_epub(&input_path, &output_dir);
        fs::remove_file(&input_path).expect("selected EPUB should be removable");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let failure = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect_err("archive-open failure should be fatal");

        assert!(
            failure
                .error
                .to_string()
                .contains("Failed to open input file"),
            "unexpected failure: {}",
            failure.error
        );
        assert_eq!(failure.partial.counts.extracted, 0);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn archive_parse_failure_after_selection_is_a_fatal_extraction_error() {
        let temp_dir = temp_test_dir("archive-parse-failure");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_minimal_epub(&input_path, "images/image.png", "image/png", MINIMAL_PNG);
        let selected = select_epub(&input_path, &output_dir);
        fs::write(&input_path, b"not a ZIP archive").expect("selected EPUB should be replaceable");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let failure = extract(selected, DocumentExtractionPolicy::NormalImages, &pipeline)
            .expect_err("archive-parse failure should be fatal");

        assert!(
            failure
                .error
                .to_string()
                .contains("Failed to read zip archive"),
            "unexpected failure: {}",
            failure.error
        );
        assert_eq!(failure.partial.counts.extracted, 0);
        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn filtered_metadata_cover_is_terminal_before_filename_and_normal_fallback() {
        let temp_dir = temp_test_dir("filtered-cover-terminal");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/art.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/page.jpg", "image/jpeg", None),
            ],
            &[
                ("OEBPS/images/art.png", MINIMAL_PNG),
                ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
                ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect("a filtered cover should complete without another candidate or fallback");

        assert_eq!(result.counts.extracted, 0);
        assert!(!result.has_normal_image_output());
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::UnsupportedCoverFormat {
                format: ImageFormat::Png,
            }]
        );
        assert!(
            fs::read_dir(&output_dir)
                .expect("output directory should remain readable")
                .next()
                .is_none()
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn unsupported_cover_conversion_is_terminal_before_filename_and_normal_fallback() {
        let temp_dir = temp_test_dir("unsupported-cover-conversion-terminal");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/art.svg",
                    "image/svg+xml",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/page.jpg", "image/jpeg", None),
            ],
            &[
                ("OEBPS/images/art.svg", b"<svg/>"),
                ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
                ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
            ],
        );
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Svg, ImageFormat::Jpg]),
            ConversionTarget::Png,
        );
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect("an unsupported cover conversion should be a terminal decision");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert!(!result.has_normal_image_output());
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::CoverConversionSkipped {
                format: ImageFormat::Svg,
            }]
        );
        assert!(
            fs::read_dir(&output_dir)
                .expect("output directory should remain readable")
                .next()
                .is_none()
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn failed_cover_conversion_is_terminal_before_filename_and_normal_fallback() {
        let temp_dir = temp_test_dir("failed-cover-conversion-terminal");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/art.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/page.jpg", "image/jpeg", None),
            ],
            &[
                ("OEBPS/images/art.png", b"\x89PNG\r\n\x1A\ninvalid"),
                ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
                ("OEBPS/images/page.jpg", b"\xFF\xD8\xFFpage"),
            ],
        );
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Png, ImageFormat::Jpg]),
            ConversionTarget::Jpg,
        );
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect("a failed cover conversion should be a terminal decision");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert!(!result.has_normal_image_output());
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::CoverConversionFailed { detail }]
                if detail == "Failed to decode image"
        ));
        assert!(
            fs::read_dir(&output_dir)
                .expect("output directory should remain readable")
                .next()
                .is_none()
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn unreadable_metadata_cover_warns_then_filename_cover_succeeds() {
        let temp_dir = temp_test_dir("unreadable-cover-fallback");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let cover = b"\xFF\xD8\xFFcover";
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/missing.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
            ],
            &[("OEBPS/images/cover.jpg", cover)],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            &pipeline,
        )
        .expect("an unreadable metadata cover should allow filename fallback");

        assert_eq!(result.counts.extracted, 1);
        assert!(!result.has_normal_image_output());
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, .. }]
                if source_name == "OEBPS/images/missing.png"
        ));
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.jpg")).unwrap(),
            cover
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn cover_retry_warnings_precede_normal_fallback_warning() {
        let temp_dir = temp_test_dir("unreadable-covers-batch-fallback");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let extension_only_page = b"extension-only-page";
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/missing.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/page.png", "image/png", None),
            ],
            &[("OEBPS/images/page.png", extension_only_page)],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect("unreadable cover candidates should allow batch fallback");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.has_normal_image_output());
        assert!(matches!(
            &result.warnings[..],
            [
                ImageWriteWarning::ArchiveImageAcquisitionFailed {
                    source_name: metadata_source,
                    ..
                },
                ImageWriteWarning::ArchiveImageAcquisitionFailed {
                    source_name: filename_source,
                    ..
                },
                ImageWriteWarning::ExtensionFallback {
                    source_name: fallback_source,
                    format: ImageFormat::Png,
                },
            ] if metadata_source == "OEBPS/images/missing.png"
                && filename_source == "OEBPS/images/cover.jpg"
                && fallback_source == "OEBPS/images/page.png"
        ));
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
            extension_only_page
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn cover_retries_precede_partial_normal_fallback_facts() {
        let temp_dir = temp_test_dir("cover-retries-partial-batch-fallback");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        let blocked_gif_output = temp_dir.join("blocked-gifs");
        let convertible_png = encoded_test_png();
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        fs::write(&blocked_gif_output, b"not a directory")
            .expect("blocked GIF destination should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/missing.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/first.png", "image/png", None),
                ("animation", "images/second.gif", "image/gif", None),
            ],
            &[
                ("OEBPS/images/first.png", &convertible_png),
                ("OEBPS/images/second.gif", b"GIF89a"),
            ],
        );
        let conversion = ConversionPolicy::try_from(ConversionRequest {
            target: ConversionTarget::Jpg,
            quality: None,
            lossless: false,
        })
        .expect("test Conversion policy should be valid");
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png, ImageFormat::Gif]),
            Some(conversion),
            Some(blocked_gif_output),
        ));
        let selected = select_epub(&input_path, &output_dir);

        let failure = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect_err("fallback emission failure should retain earlier normal output");

        assert_eq!(failure.partial.counts.extracted, 1);
        assert_eq!(failure.partial.counts.gifs_routed, 0);
        assert_eq!(failure.partial.counts.converted, 1);
        assert_eq!(failure.partial.counts.skipped, 0);
        assert!(failure.partial.has_normal_image_output());
        assert_eq!(
            acquisition_failure_sources(&failure.partial.warnings),
            vec!["OEBPS/images/missing.png", "OEBPS/images/cover.jpg"]
        );
        let emitted = fs::read_dir(&output_dir)
            .expect("normal output directory should remain readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("normal output entries should remain readable");
        assert_eq!(emitted.len(), 1);
        assert_eq!(
            emitted[0]
                .path()
                .extension()
                .and_then(|value| value.to_str()),
            Some("jpg")
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn routed_gif_fact_precedes_later_normal_fallback_failure() {
        let temp_dir = temp_test_dir("routed-gif-before-normal-failure");
        let input_path = temp_dir.join("sample.epub");
        let blocked_output = temp_dir.join("blocked-normal-output");
        let gif_output = temp_dir.join("gifs");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::write(&blocked_output, b"not a directory")
            .expect("blocked normal destination should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/missing.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("animation", "images/a.gif", "image/gif", None),
                ("page", "images/b.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/a.gif", b"GIF89a"),
                ("OEBPS/images/b.png", MINIMAL_PNG),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png, ImageFormat::Gif]),
            None,
            Some(gif_output.clone()),
        ));
        let selected = select_epub(&input_path, &blocked_output);

        let failure = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect_err("later normal emission failure should retain the routed GIF");

        assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
        assert_eq!(failure.partial.counts.extracted, 1);
        assert_eq!(failure.partial.counts.gifs_routed, 1);
        assert_eq!(failure.partial.counts.converted, 0);
        assert!(failure.partial.has_normal_image_output());
        assert_eq!(
            acquisition_failure_sources(&failure.partial.warnings),
            vec!["OEBPS/images/missing.png", "OEBPS/images/cover.jpg"]
        );
        let routed = fs::read_dir(&gif_output)
            .expect("GIF output directory should remain readable")
            .collect::<Result<Vec<_>, _>>()
            .expect("routed GIF entries should remain readable");
        assert_eq!(routed.len(), 1);
        assert_eq!(fs::read(routed[0].path()).unwrap(), b"GIF89a");

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn resolved_archive_identity_prevents_duplicate_cover_attempts() {
        let temp_dir = temp_test_dir("resolved-cover-identity");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let corrupt_cover = b"uniquely corrupt cover payload";
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/cover%2Ejpg",
                    "image/jpeg",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("non-cover-alias", "images/cover%2ejpg", "image/jpeg", None),
                ("page", "images/page.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/cover.jpg", corrupt_cover),
                ("OEBPS/images/page.png", MINIMAL_PNG),
            ],
        );
        corrupt_stored_payload(&input_path, corrupt_cover);
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &output_dir);

        let result = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect("one failed resolved cover should allow batch fallback");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.has_normal_image_output());
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(
                    warning,
                    ImageWriteWarning::ArchiveImageAcquisitionFailed { .. }
                ))
                .count(),
            1
        );
        assert_eq!(
            acquisition_failure_sources(&result.warnings),
            vec!["OEBPS/images/cover%2Ejpg"]
        );
        assert_eq!(
            fs::read(output_dir.join("Tester - Magic Test.png")).unwrap(),
            MINIMAL_PNG
        );
        assert_eq!(
            fs::read_dir(&output_dir)
                .expect("normal output directory should remain readable")
                .count(),
            1
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn cover_emission_failure_aborts_the_document() {
        let temp_dir = temp_test_dir("cover-emission-failure");
        let input_path = temp_dir.join("sample.epub");
        let blocked_output = temp_dir.join("not-a-directory");
        fs::create_dir_all(&temp_dir).expect("temporary test directory should be creatable");
        fs::write(&blocked_output, b"occupied").expect("blocking file should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/missing.png",
                    "image/png",
                    Some("cover-image"),
                ),
                (
                    "filename-cover",
                    "images/cover.jpg",
                    "application/octet-stream",
                    None,
                ),
                ("page", "images/page.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/cover.jpg", b"unidentified-cover"),
                ("OEBPS/images/page.png", MINIMAL_PNG),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
            None,
            None,
        ));
        let selected = select_epub(&input_path, &blocked_output);

        let error = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect_err("Image file emission failure must abort cover extraction");

        assert!(format!("{:#}", error.error).contains("Failed to create output directory"));
        assert_eq!(error.partial.counts.extracted, 0);
        assert!(matches!(
            &error.partial.warnings[..],
            [
                ImageWriteWarning::ArchiveImageAcquisitionFailed { source_name, .. },
                ImageWriteWarning::CoverDefaultToJpeg { mime },
            ] if source_name == "OEBPS/images/missing.png"
                && mime == "application/octet-stream"
        ));

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn fatal_metadata_cover_emission_stops_filename_candidate_and_normal_fallback() {
        let temp_dir = temp_test_dir("fatal-cover-stops-later-work");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        let blocked_gif_output = temp_dir.join("blocked-gifs");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        fs::write(&blocked_gif_output, b"not a directory")
            .expect("blocked GIF destination should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "metadata-cover",
                    "images/art.gif",
                    "image/gif",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/page.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/art.gif", b"GIF89a"),
                ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfilename"),
                ("OEBPS/images/page.png", MINIMAL_PNG),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Gif, ImageFormat::Jpg, ImageFormat::Png]),
            None,
            Some(blocked_gif_output),
        ));
        let selected = select_epub(&input_path, &output_dir);

        let failure = extract(
            selected,
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            &pipeline,
        )
        .expect_err("fatal metadata cover emission should abort the EPUB");

        assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
        assert_eq!(failure.partial.counts.extracted, 0);
        assert_eq!(failure.partial.counts.gifs_routed, 0);
        assert!(!failure.partial.has_normal_image_output());
        assert!(failure.partial.warnings.is_empty());
        assert!(
            fs::read_dir(&output_dir)
                .expect("normal output directory should remain readable")
                .next()
                .is_none(),
            "neither the filename cover nor normal fallback may run after a fatal cover"
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
