//! EPUB file processing module

#[path = "epub/cover_extraction.rs"]
mod cover_extraction;
#[path = "epub/resource_archive.rs"]
mod resource_archive;

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::epub_declarations::EpubDeclarations;
use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWriteOutcome, ImageWritePipeline,
    ImageWriteRequest,
};

use self::cover_extraction::{EpubCoverRequest, extract_required_cover};
use self::resource_archive::{ArchiveResourceIdentity, EpubResource, EpubResourceArchive};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// Processes a single .epub file, extracting images accepted by the requested Image formats.
/// Uses the selected document base name for output files.
/// If cover_only is true, only extracts the cover image.
/// If cover_fallback is true and cover_only is true but no cover is found, extracts all images.
/// The configured Image write pipeline owns bounded source acquisition, image
/// acceptance, and output policy. Retained EPUB declarations drive manifest and
/// cover traversal while a separate read-only ZIP handle lends scoped readers.
///
/// # Errors
///
/// Returns an error when EPUB declarations cannot be acquired, an archive cannot be opened,
/// or when collision-safe output emission cannot create or complete a file.
pub(super) fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    base_name: &str,
    retained_declarations: Option<&EpubDeclarations>,
    cover_only: bool,
    cover_fallback: bool,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let acquired_declarations;
    let declarations = match retained_declarations {
        Some(declarations) => declarations,
        None => {
            acquired_declarations =
                EpubDeclarations::acquire(input_path).map_err(anyhow::Error::new)?;
            &acquired_declarations
        }
    };
    // ADR-0001 keeps payload acquisition on an independent direct ZIP handle,
    // even when declaration facts were retained earlier by Document selection.
    let mut archive = EpubResourceArchive::open(input_path, declarations.resources())?;
    let resources = archive.resources().to_vec();

    if cover_only {
        return extract_cover_only(
            &mut archive,
            &resources,
            declarations.cover_id(),
            output_base_dir,
            base_name,
            cover_fallback,
            pipeline,
        );
    }

    extract_all_images(
        &mut archive,
        &resources,
        &HashSet::new(),
        output_base_dir,
        base_name,
        pipeline,
    )
}

/// Extracts every non-excluded manifest resource in deterministic resolved-path order.
///
/// Weak labels are intentional inputs: byte-first discovery can recover images
/// from resources whose extension and MIME type do not identify them.
///
/// # Errors
///
/// Returns an error only when output emission fails; per-resource lookup and read
/// failures become warning facts and traversal continues.
fn extract_all_images(
    archive: &mut EpubResourceArchive,
    resources: &[EpubResource],
    excluded_identities: &HashSet<ArchiveResourceIdentity>,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    pipeline.write_from(
        ImageWriteRequest::normal_images(output_base_dir, base_name),
        |visitor| {
            for candidate in resources {
                if excluded_identities.contains(candidate.identity()) {
                    continue;
                }
                let source = ArchiveImageSource::named(candidate.manifest_path())
                    .with_mime(candidate.mime());
                visit_resource(archive, candidate, source, visitor)?;
            }
            Ok(())
        },
    )
}

/// Searches for a cover image by filename when the declared cover identity fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the first matching deterministic manifest candidate.
fn find_cover_by_filename(resources: &[EpubResource]) -> Option<&EpubResource> {
    resources.iter().find(|candidate| {
        let path = Path::new(candidate.manifest_path());
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

/// Extracts only the cover image from an EPUB file
/// If cover_fallback is true and no cover is found, extracts all images instead
///
/// # Errors
///
/// Returns an error when output emission fails. Unreadable cover resources become
/// warnings and advance through declared identity, filename, then optional batch fallback.
fn extract_cover_only(
    archive: &mut EpubResourceArchive,
    resources: &[EpubResource],
    cover_id: Option<&str>,
    output_base_dir: &Path,
    base_name: &str,
    cover_fallback: bool,
    pipeline: &ImageWritePipeline,
) -> ImageWriteOutcome {
    let metadata_cover = cover_id.and_then(|id| resources.iter().find(|item| item.id() == id));
    let filename_cover = find_cover_by_filename(resources);
    extract_required_cover(
        archive,
        EpubCoverRequest {
            resources,
            metadata_cover,
            filename_cover,
            cover_fallback,
            output_dir: output_base_dir,
            base_name,
            pipeline,
        },
    )
}

/// Visits one manifest resource while keeping its `ZipFile` borrow scoped.
///
/// # Errors
///
/// Returns an error when the pipeline cannot emit an accepted image. Resource
/// lookup, open, and read failures are recorded on the visitor and return `Ok(())`.
fn visit_resource(
    archive: &mut EpubResourceArchive,
    candidate: &EpubResource,
    source: ArchiveImageSource,
    visitor: &mut ArchiveImageVisitor<'_, '_>,
) -> Result<()> {
    let acquisition =
        archive.with_reader(candidate, |reader| visitor.visit(source.clone(), reader))?;
    if let Err(error) = acquisition {
        visitor.unreadable(source, error);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_format::ImageFormat;
    use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
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
    fn test_jpeg_extensions_contains_common_extensions() {
        assert!(JPEG_EXTENSIONS.contains(&"jpg"));
        assert!(JPEG_EXTENSIONS.contains(&"jpeg"));
        assert!(JPEG_EXTENSIONS.contains(&"jpe"));
        assert!(JPEG_EXTENSIONS.contains(&"jfif"));
    }

    #[test]
    fn test_jpeg_extensions_does_not_contain_other_formats() {
        assert!(!JPEG_EXTENSIONS.contains(&"png"));
        assert!(!JPEG_EXTENSIONS.contains(&"gif"));
        assert!(!JPEG_EXTENSIONS.contains(&"webp"));
        assert!(!JPEG_EXTENSIONS.contains(&"bmp"));
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

        let result = process_file(
            &input_path,
            &output_dir,
            "Tester - Magic Test",
            None,
            false,
            false,
            &pipeline,
        )
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

        let result = process_file(
            &input_path,
            &output_dir,
            "Tester - Magic Test",
            None,
            false,
            false,
            &pipeline,
        )
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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            false,
            false,
            &pipeline,
        )
        .expect("a missing resource should not abort the EPUB");

        assert_eq!(result.counts.extracted, 1);
        assert!(matches!(
            &result.warnings[..],
            [ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name,
                message,
            }] if source_name == "OEBPS/images/a.png"
                && message.contains("EPUB resource not found")
        ));
        assert_eq!(
            fs::read(output_dir.join("sample.png")).unwrap(),
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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            false,
            false,
            &pipeline,
        )
        .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 2);
        assert_eq!(
            fs::read(output_dir.join("sample_1.png")).unwrap(),
            first_by_path
        );
        assert_eq!(
            fs::read(output_dir.join("sample_2.png")).unwrap(),
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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            false,
            false,
            &pipeline,
        )
        .expect("EPUB extraction should succeed");

        assert_eq!(result.counts.extracted, 2);
        assert_eq!(
            fs::read(output_dir.join("sample_1.png")).unwrap(),
            first_by_resolved_path
        );
        assert_eq!(
            fs::read(output_dir.join("sample_2.png")).unwrap(),
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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            false,
            false,
            &pipeline,
        )
        .expect("percent-decoded lookup should preserve EPUB crate behavior");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            fs::read(output_dir.join("sample.png")).unwrap(),
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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            false,
            false,
            &pipeline,
        )
        .expect("exact ZIP lookup should take precedence");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            fs::read(output_dir.join("sample.png")).unwrap(),
            exact_payload
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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            true,
            false,
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
        assert_eq!(fs::read(output_dir.join("sample.jpg")).unwrap(), cover);

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn unreadable_cover_candidates_fall_back_to_batch_images() {
        let temp_dir = temp_test_dir("unreadable-covers-batch-fallback");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
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
            &[("OEBPS/images/page.png", MINIMAL_PNG)],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
            None,
            None,
        ));

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            true,
            true,
            &pipeline,
        )
        .expect("unreadable cover candidates should allow batch fallback");

        assert_eq!(result.counts.extracted, 1);
        assert!(result.has_normal_image_output());
        assert_eq!(
            acquisition_failure_sources(&result.warnings),
            vec!["OEBPS/images/missing.png", "OEBPS/images/cover.jpg"]
        );
        assert_eq!(
            fs::read(output_dir.join("sample.png")).unwrap(),
            MINIMAL_PNG
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn cover_retries_precede_partial_normal_fallback_facts() {
        let temp_dir = temp_test_dir("cover-retries-partial-batch-fallback");
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
                    "images/missing.png",
                    "image/png",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/first.png", "image/png", None),
                ("animation", "images/second.gif", "image/gif", None),
            ],
            &[
                ("OEBPS/images/first.png", MINIMAL_PNG),
                ("OEBPS/images/second.gif", b"GIF89a"),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png, ImageFormat::Gif]),
            None,
            Some(blocked_gif_output),
        ));

        let failure = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            true,
            true,
            &pipeline,
        )
        .expect_err("fallback emission failure should retain earlier normal output");

        assert_eq!(failure.partial.counts.extracted, 1);
        assert_eq!(failure.partial.counts.gifs_routed, 0);
        assert!(failure.partial.has_normal_image_output());
        assert_eq!(
            acquisition_failure_sources(&failure.partial.warnings),
            vec!["OEBPS/images/missing.png", "OEBPS/images/cover.jpg"]
        );
        assert_eq!(
            fs::read(output_dir.join("sample_1.png")).unwrap(),
            MINIMAL_PNG
        );

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

        let result = process_file(
            &input_path,
            &output_dir,
            "sample",
            None,
            true,
            true,
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
            fs::read(output_dir.join("sample.png")).unwrap(),
            MINIMAL_PNG
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
                    "images/art.jpg",
                    "application/octet-stream",
                    Some("cover-image"),
                ),
                ("filename-cover", "images/cover.jpg", "image/jpeg", None),
                ("page", "images/page.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/art.jpg", b"unidentified-cover"),
                ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFfallback"),
                ("OEBPS/images/page.png", MINIMAL_PNG),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
            None,
            None,
        ));

        let error = process_file(
            &input_path,
            &blocked_output,
            "sample",
            None,
            true,
            true,
            &pipeline,
        )
        .expect_err("Image file emission failure must abort cover extraction");

        assert!(format!("{:#}", error.error).contains("Failed to create output directory"));
        assert_eq!(error.partial.counts.extracted, 0);
        assert_eq!(
            error.partial.warnings,
            vec![ImageWriteWarning::CoverDefaultToJpeg {
                mime: "application/octet-stream".to_string(),
            }]
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
