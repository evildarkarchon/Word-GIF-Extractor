//! EPUB file processing module

use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use percent_encoding::percent_decode;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use zip::ZipArchive;

use crate::image_write_pipeline::{
    ArchiveImageSource, ArchiveImageVisitor, ImageWritePipeline, ImageWriteRequest,
    ImageWriteResult,
};

/// Common JPEG file extensions for cover image fallback detection
const JPEG_EXTENSIONS: &[&str] = &["jpg", "jpeg", "jpe", "jfif"];

/// Gets the metadata (author, title) from an EPUB file.
/// Returns a tuple of (author, title) where either may be None if not present.
/// Used for deduplication and display purposes.
pub fn get_metadata(input_path: &Path) -> Result<(Option<String>, Option<String>)> {
    let doc =
        EpubDoc::new(input_path).map_err(|e| anyhow::anyhow!("Failed to open EPUB file: {}", e))?;

    let title = doc.mdata("title").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone());

    Ok((author, title))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EpubResourceCandidate {
    id: String,
    source_name: String,
    sort_name: String,
    mime: String,
}

/// Processes a single .epub file, extracting images accepted by the requested Image formats.
/// Uses the selected document base name for output files.
/// If cover_only is true, only extracts the cover image.
/// If cover_fallback is true and cover_only is true but no cover is found, extracts all images.
/// The configured Image write pipeline owns bounded source acquisition, image
/// acceptance, and output policy. The EPUB adapter retains manifest and cover
/// traversal while a second read-only ZIP handle lends scoped resource readers.
///
/// # Errors
///
/// Returns an error when EPUB metadata or either archive handle cannot be opened,
/// or when collision-safe output emission cannot create or complete a file.
pub fn process_file(
    input_path: &Path,
    output_base_dir: &Path,
    base_name: &str,
    cover_only: bool,
    cover_fallback: bool,
    pipeline: &ImageWritePipeline,
) -> Result<ImageWriteResult> {
    let doc =
        EpubDoc::new(input_path).map_err(|e| anyhow::anyhow!("Failed to open EPUB file: {}", e))?;
    let cover_id = doc.get_cover_id();
    let mut resources: Vec<EpubResourceCandidate> = doc
        .resources
        .iter()
        .map(|(id, item)| {
            let source_name = archive_path(&item.path);
            EpubResourceCandidate {
                id: id.clone(),
                sort_name: String::new(),
                source_name,
                mime: item.mime.clone(),
            }
        })
        .collect();
    // EpubDoc exposes the manifest facts we need but only offers eager payload reads.
    // Release it before opening the independent read-only ZIP handle recorded in ADR-0001.
    drop(doc);

    let file = fs::File::open(input_path)
        .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read zip archive: {}", input_path.display()))?;
    for candidate in &mut resources {
        // Missing resources keep their manifest spelling for a stable position; traversal
        // performs the same lookup again so its failure becomes an observable warning.
        let resolved_name = resource_index(&archive, &candidate.source_name)
            .ok()
            .and_then(|index| archive.name_for_index(index).map(str::to_owned))
            .unwrap_or_else(|| candidate.source_name.clone());
        candidate.sort_name = normalized_sort_path(&resolved_name);
    }
    resources
        .sort_by(|left, right| (&left.sort_name, &left.id).cmp(&(&right.sort_name, &right.id)));

    if cover_only {
        return extract_cover_only(
            &mut archive,
            &resources,
            cover_id.as_deref(),
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
    archive: &mut ZipArchive<fs::File>,
    resources: &[EpubResourceCandidate],
    excluded_sources: &HashSet<String>,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> Result<ImageWriteResult> {
    pipeline.write_from(
        ImageWriteRequest::normal_images(output_base_dir, base_name),
        |visitor| {
            for candidate in resources {
                if excluded_sources.contains(&candidate.source_name) {
                    continue;
                }
                let source = ArchiveImageSource::named(candidate.source_name.clone())
                    .with_mime(candidate.mime.clone());
                visit_resource(archive, candidate, source, visitor)?;
            }
            Ok(())
        },
    )
}

/// Searches for a cover image by filename when metadata-based detection fails.
/// Looks for files named "cover" (case-insensitive) with common JPEG extensions.
/// Returns the first matching deterministic manifest candidate.
fn find_cover_by_filename(resources: &[EpubResourceCandidate]) -> Option<&EpubResourceCandidate> {
    resources.iter().find(|candidate| {
        let path = Path::new(&candidate.source_name);
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
/// warnings and advance through metadata, filename, then optional batch fallback.
fn extract_cover_only(
    archive: &mut ZipArchive<fs::File>,
    resources: &[EpubResourceCandidate],
    cover_id: Option<&str>,
    output_base_dir: &Path,
    base_name: &str,
    cover_fallback: bool,
    pipeline: &ImageWritePipeline,
) -> Result<ImageWriteResult> {
    let metadata_cover = cover_id.and_then(|id| resources.iter().find(|item| item.id == id));
    let filename_cover = find_cover_by_filename(resources);
    let mut attempted_sources = HashSet::new();
    let mut aggregate = ImageWriteResult::default();

    for candidate in [metadata_cover, filename_cover].into_iter().flatten() {
        if !attempted_sources.insert(candidate.source_name.clone()) {
            continue;
        }

        let result = write_cover_image(archive, candidate, output_base_dir, base_name, pipeline)?;
        let acquisition_failed = result.is_archive_image_acquisition_failure();
        append_result(&mut aggregate, result);
        if !acquisition_failed {
            return Ok(aggregate);
        }
    }

    if cover_fallback {
        // Re-reading a deterministically unreadable cover would only duplicate its warning.
        let fallback = extract_all_images(
            archive,
            resources,
            &attempted_sources,
            output_base_dir,
            base_name,
            pipeline,
        )?;
        append_result(&mut aggregate, fallback);
    }

    Ok(aggregate)
}

/// Writes one cover candidate through cover-specific Image write purpose semantics.
///
/// # Errors
///
/// Returns an error when output emission fails. Candidate lookup and read failures
/// are returned as non-fatal warning facts in the result.
fn write_cover_image(
    archive: &mut ZipArchive<fs::File>,
    candidate: &EpubResourceCandidate,
    output_base_dir: &Path,
    base_name: &str,
    pipeline: &ImageWritePipeline,
) -> Result<ImageWriteResult> {
    pipeline.write_from(
        ImageWriteRequest::required_epub_cover(output_base_dir, base_name),
        |visitor| {
            let source = ArchiveImageSource::required_epub_cover(
                candidate.source_name.clone(),
                candidate.mime.clone(),
            );
            visit_resource(archive, candidate, source, visitor)
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
    archive: &mut ZipArchive<fs::File>,
    candidate: &EpubResourceCandidate,
    source: ArchiveImageSource,
    visitor: &mut ArchiveImageVisitor<'_, '_>,
) -> Result<()> {
    let index = match resource_index(archive, &candidate.source_name) {
        Ok(index) => index,
        Err(error) => {
            visitor.unreadable(source, error);
            return Ok(());
        }
    };

    match archive.by_index(index) {
        Ok(mut entry) => visitor.visit(source, &mut entry),
        Err(error) => {
            visitor.unreadable(source, error);
            Ok(())
        }
    }
}

/// Resolves a manifest path using the EPUB crate's exact-then-decoded behavior.
///
/// # Errors
///
/// Returns an error when percent-decoding is not valid UTF-8 or neither lookup
/// spelling names a ZIP entry.
fn resource_index(archive: &ZipArchive<fs::File>, source_name: &str) -> Result<usize> {
    if let Some(index) = archive.index_for_name(source_name) {
        return Ok(index);
    }

    let decoded = percent_decode(source_name.as_bytes())
        .decode_utf8()
        .with_context(|| {
            format!("Invalid UTF-8 percent encoding in EPUB resource {source_name}")
        })?;
    archive
        .index_for_name(&decoded)
        .ok_or_else(|| anyhow::anyhow!("EPUB resource not found: {source_name}"))
}

/// Converts an EPUB manifest path to the ZIP lookup spelling used by the adapter.
fn archive_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Builds a stable lexical sort key without changing the ZIP lookup path.
fn normalized_sort_path(source_name: &str) -> String {
    source_name
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Appends one fallback attempt while preserving counts and warning order.
fn append_result(aggregate: &mut ImageWriteResult, mut result: ImageWriteResult) {
    aggregate.counts.extracted += result.counts.extracted;
    aggregate.counts.gifs_routed += result.counts.gifs_routed;
    aggregate.counts.converted += result.counts.converted;
    aggregate.counts.skipped += result.counts.skipped;
    aggregate.warnings.append(&mut result.warnings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
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

    /// Builds a pipeline with a default Conversion policy for EPUB adapter tests.
    fn pipeline_with_conversion(
        allowed_formats: HashSet<ImageFormat>,
        target: ConversionTarget,
    ) -> ImageWritePipeline {
        let conversion = ConversionPolicy::try_from(ConversionRequest {
            target,
            quality: None,
            lossless: false,
        })
        .expect("test conversion request should be valid");
        ImageWritePipeline::new(ImageWritePolicy::new(
            allowed_formats,
            Some(conversion),
            None,
        ))
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

        let result = process_file(&input_path, &output_dir, "sample", false, false, &pipeline)
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

        let result = process_file(&input_path, &output_dir, "sample", false, false, &pipeline)
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

        let result = process_file(&input_path, &output_dir, "sample", false, false, &pipeline)
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

        let result = process_file(&input_path, &output_dir, "sample", false, false, &pipeline)
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

        let result = process_file(&input_path, &output_dir, "sample", false, false, &pipeline)
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

        let result = process_file(&input_path, &output_dir, "sample", true, false, &pipeline)
            .expect("an unreadable metadata cover should allow filename fallback");

        assert_eq!(result.counts.extracted, 1);
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

        let result = process_file(&input_path, &output_dir, "sample", true, true, &pipeline)
            .expect("unreadable cover candidates should allow batch fallback");

        assert_eq!(result.counts.extracted, 1);
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| matches!(
                    warning,
                    ImageWriteWarning::ArchiveImageAcquisitionFailed { .. }
                ))
                .count(),
            2
        );
        assert_eq!(
            fs::read(output_dir.join("sample.png")).unwrap(),
            MINIMAL_PNG
        );

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn filtered_cover_does_not_trigger_batch_fallback() {
        let temp_dir = temp_test_dir("filtered-cover-no-fallback");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "cover",
                    "images/cover.jpg",
                    "image/jpeg",
                    Some("cover-image"),
                ),
                ("page", "images/page.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/cover.jpg", b"\xFF\xD8\xFFcover"),
                ("OEBPS/images/page.png", MINIMAL_PNG),
            ],
        );
        let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
            HashSet::from([ImageFormat::Png]),
            None,
            None,
        ));

        let result = process_file(&input_path, &output_dir, "sample", true, true, &pipeline)
            .expect("filtered cover should be a normal cover-only outcome");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::UnsupportedCoverFormat {
                format: ImageFormat::Jpg,
            }]
        );
        assert!(!output_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }

    #[test]
    fn conversion_skipped_cover_does_not_trigger_batch_fallback() {
        let temp_dir = temp_test_dir("conversion-skipped-cover-no-fallback");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        write_epub_fixture(
            &input_path,
            &[
                (
                    "cover",
                    "images/cover.svg",
                    "image/svg+xml",
                    Some("cover-image"),
                ),
                ("page", "images/page.png", "image/png", None),
            ],
            &[
                ("OEBPS/images/cover.svg", b"<svg/>"),
                ("OEBPS/images/page.png", MINIMAL_PNG),
            ],
        );
        let pipeline = pipeline_with_conversion(
            HashSet::from([ImageFormat::Svg, ImageFormat::Png]),
            ConversionTarget::Png,
        );

        let result = process_file(&input_path, &output_dir, "sample", true, true, &pipeline)
            .expect("conversion-skipped cover should remain a cover-only outcome");

        assert_eq!(result.counts.extracted, 0);
        assert_eq!(
            result.warnings,
            vec![ImageWriteWarning::CoverConversionSkipped {
                format: ImageFormat::Svg,
            }]
        );
        assert!(!output_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary test directory should be removable");
    }
}
