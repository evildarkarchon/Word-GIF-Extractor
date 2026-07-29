//! Per-document extraction policy, dispatch, and outcomes.

#[path = "docx.rs"]
mod docx;
#[path = "epub.rs"]
mod epub;

use std::fmt;

use crate::document_selection::SelectedDocument;
use crate::image_write_pipeline::{ImageWritePipeline, ImageWriteResult, ImageWriteWarning};

/// Valid per-run choices for normal images versus EPUB cover extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentExtractionPolicy {
    /// Extract normal document images.
    NormalImages,
    /// Extract one required EPUB cover, optionally falling back to normal images.
    EpubCover {
        /// Whether an EPUB without a usable cover should emit normal images.
        fallback_to_normal_images: bool,
    },
}

impl DocumentExtractionPolicy {
    /// Returns whether EPUB documents should use required-cover extraction.
    fn is_epub_cover_only(self) -> bool {
        matches!(self, Self::EpubCover { .. })
    }
}

/// Immutable Document extraction module configured for one Extraction run.
pub(crate) struct DocumentExtraction {
    policy: DocumentExtractionPolicy,
    image_write_pipeline: ImageWritePipeline,
}

impl DocumentExtraction {
    /// Binds Document extraction and Image write policy for every selected document.
    pub(crate) fn new(
        policy: DocumentExtractionPolicy,
        image_write_pipeline: ImageWritePipeline,
    ) -> Self {
        Self {
            policy,
            image_write_pipeline,
        }
    }

    /// Returns whether this module is configured to extract EPUB covers.
    pub(crate) fn is_epub_cover_extraction_configured(&self) -> bool {
        self.policy.is_epub_cover_only()
    }

    /// Consumes one selected document and dispatches its authoritative variant.
    ///
    /// Document-local errors are returned as failed outcomes so the Extraction
    /// run can continue processing later documents.
    pub(crate) fn extract(&self, document: SelectedDocument) -> DocumentExtractionOutcome {
        let result = match document {
            SelectedDocument::Docx(document) => {
                let (path, output_dir, base_name) = document.into_extraction_parts();
                docx::process_file(&path, &output_dir, &base_name, &self.image_write_pipeline)
            }
            SelectedDocument::Epub(document) => {
                epub::extract(document, self.policy, &self.image_write_pipeline)
            }
        };

        match result {
            Ok(result) => DocumentExtractionOutcome::Completed(
                DocumentExtractionFacts::from_image_write_result(result),
            ),
            Err(failure) => DocumentExtractionOutcome::Failed {
                facts: DocumentExtractionFacts::from_image_write_result(failure.partial),
                error: DocumentExtractionError::from_source(failure.error),
            },
        }
    }
}

/// Opaque facts retained by one completed or failed Document extraction.
///
/// The value translates Image write pipeline accounting and warnings at the
/// Document extraction seam so callers do not depend on inner pipeline types.
#[derive(Debug)]
pub(crate) struct DocumentExtractionFacts {
    emitted_images: usize,
    gifs_routed: usize,
    converted_images: usize,
    skipped_conversions: usize,
    has_normal_image_output: bool,
    warnings: Vec<DocumentExtractionWarning>,
}

impl DocumentExtractionFacts {
    /// Translates inner Image write facts at the Document extraction seam.
    fn from_image_write_result(result: ImageWriteResult) -> Self {
        let has_normal_image_output = result.has_normal_image_output();
        Self {
            emitted_images: result.counts.extracted,
            gifs_routed: result.counts.gifs_routed,
            converted_images: result.counts.converted,
            skipped_conversions: result.counts.skipped,
            has_normal_image_output,
            warnings: result
                .warnings
                .into_iter()
                .map(DocumentExtractionWarning::from_image_write_warning)
                .collect(),
        }
    }

    /// Returns the number of images successfully emitted before the outcome ended.
    pub(crate) fn get_emitted_images(&self) -> usize {
        self.emitted_images
    }

    /// Returns the number of emitted GIFs routed to the configured destination.
    pub(crate) fn get_gifs_routed(&self) -> usize {
        self.gifs_routed
    }

    /// Returns the number of images successfully converted before emission.
    pub(crate) fn get_converted_images(&self) -> usize {
        self.converted_images
    }

    /// Returns the number of conversion attempts skipped while preserving source bytes.
    pub(crate) fn get_skipped_conversions(&self) -> usize {
        self.skipped_conversions
    }

    /// Returns whether any emitted file came from normal-image extraction.
    pub(crate) fn is_normal_image_output_present(&self) -> bool {
        self.has_normal_image_output
    }

    /// Returns ordered non-fatal warnings produced before the outcome ended.
    pub(crate) fn get_warnings(&self) -> &[DocumentExtractionWarning] {
        &self.warnings
    }
}

/// Opaque non-fatal warning exposed by Document extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentExtractionWarning {
    message: String,
}

impl DocumentExtractionWarning {
    /// Exhaustively translates one inner classification into stable Document extraction wording.
    fn from_image_write_warning(warning: ImageWriteWarning) -> Self {
        let message = match warning {
            ImageWriteWarning::ArchiveImageAcquisitionFailed {
                source_name,
                detail,
            } => format!("Could not read archive resource '{source_name}': {detail}"),
            ImageWriteWarning::ExtensionFallback {
                source_name,
                format,
            } => format!(
                "Magic detection failed for {source_name}; falling back to .{} extension",
                format.extension()
            ),
            ImageWriteWarning::CoverDefaultToJpeg { mime } => format!(
                "Cover image MIME '{mime}' could not be identified; defaulting to .jpg extension."
            ),
            ImageWriteWarning::UnsupportedCoverFormat { format } => format!(
                "Cover image format '{}' not in allowed formats, skipping.",
                format.extension()
            ),
            ImageWriteWarning::ConversionSkipped { base_name, format } => format!(
                "Skipping conversion for {base_name} ({} format not supported for conversion)",
                format.extension()
            ),
            ImageWriteWarning::CoverConversionSkipped { format } => format!(
                "Cover image format '{}' not supported for conversion, skipping cover.",
                format.extension()
            ),
            ImageWriteWarning::ConversionFailed { base_name, detail } => {
                format!("Conversion failed for image in {base_name}: {detail}")
            }
            ImageWriteWarning::CoverConversionFailed { detail } => {
                format!("Cover conversion failed: {detail}")
            }
        };
        Self { message }
    }

    /// Returns the stable user-visible wording for this warning fact.
    pub(crate) fn get_message(&self) -> &str {
        &self.message
    }
}

/// Opaque document-local failure exposed by Document extraction.
#[derive(Debug)]
pub(crate) struct DocumentExtractionError {
    source: anyhow::Error,
}

impl DocumentExtractionError {
    /// Preserves the contextual source chain while sealing its concrete type.
    fn from_source(source: anyhow::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for DocumentExtractionError {
    /// Formats the preserved document-local error context.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for DocumentExtractionError {
    /// Returns the preserved underlying error chain.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Terminal result of extracting one selected document.
pub(crate) enum DocumentExtractionOutcome {
    /// Extraction completed with its retained document-level facts.
    Completed(DocumentExtractionFacts),
    /// Extraction failed after retaining document-level facts already produced.
    Failed {
        /// Document extraction facts produced before the failure.
        facts: DocumentExtractionFacts,
        /// Opaque contextual document-local error.
        error: DocumentExtractionError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::{ConversionPolicy, ConversionRequest, ConversionTarget};
    use crate::document_selection::{
        DocumentSelectionDiagnostic, DocumentSelectionObserver, DocumentSelectionOptions,
        DocumentSelectionProgress, EpubFilter, select_documents,
    };
    use crate::image_format::ImageFormat;
    use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy};
    use std::collections::HashSet;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-document-extraction-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    /// Writes a DOCX fixture containing the supplied archive entries in order.
    fn write_docx(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("test DOCX should be creatable");
        let mut zip = zip::ZipWriter::new(file);
        for (entry_name, data) in entries {
            zip.start_file(*entry_name, SimpleFileOptions::default())
                .expect("ZIP entry should start");
            zip.write_all(data)
                .expect("ZIP entry payload should be writable");
        }
        zip.finish().expect("test DOCX should finish");
    }

    /// Writes an EPUB fixture with one declared image and optional cover property.
    fn write_epub(path: &Path, image_href: &str, properties: Option<&str>, data: &[u8]) {
        let archive_path = format!("OEBPS/{image_href}");
        write_epub_fixture(
            path,
            &[("image", image_href, "image/jpeg", properties)],
            &[(archive_path.as_str(), data)],
        );
    }

    /// Writes an EPUB fixture whose declaration and available payloads vary independently.
    fn write_epub_with_resources(
        path: &Path,
        image_href: &str,
        properties: Option<&str>,
        archive_resources: &[(&str, &[u8])],
    ) {
        write_epub_fixture(
            path,
            &[("image", image_href, "image/jpeg", properties)],
            archive_resources,
        );
    }

    /// Writes a real EPUB ZIP whose manifest declarations and payloads vary independently.
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
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
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
    <dc:identifier id="bookid">test-book</dc:identifier><dc:title>Test</dc:title>
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
        for (archive_path, data) in archive_resources {
            zip.start_file(*archive_path, options)
                .expect("image entry should start");
            zip.write_all(data).expect("image should be writable");
        }
        zip.finish().expect("test EPUB should finish");
    }

    /// Constructs a production Conversion policy for operation-level fixtures.
    fn conversion_policy(target: ConversionTarget) -> ConversionPolicy {
        ConversionPolicy::try_from(ConversionRequest {
            target,
            quality: None,
            lossless: false,
        })
        .expect("test conversion policy should be valid")
    }

    /// Returns stable warning bodies in their retained Document extraction order.
    fn warning_messages(facts: &DocumentExtractionFacts) -> Vec<&str> {
        facts
            .get_warnings()
            .iter()
            .map(DocumentExtractionWarning::get_message)
            .collect()
    }

    #[derive(Default)]
    struct SilentDocumentSelectionObserver;

    impl DocumentSelectionObserver for SilentDocumentSelectionObserver {
        /// Ignores progress facts that are outside this extraction-focused test seam.
        fn on_document_selection_progress(&mut self, _progress: DocumentSelectionProgress) {}

        /// Ignores diagnostics because the fixtures in these tests are readable.
        fn on_document_selection_diagnostic(&mut self, _diagnostic: DocumentSelectionDiagnostic) {}
    }

    /// Obtains one extraction handoff through the production Document selection operation.
    fn select_one_document(input_path: &Path, output_dir: &Path) -> SelectedDocument {
        let mut observer = SilentDocumentSelectionObserver;
        let input_path = input_path.to_path_buf();
        select_documents(
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
        .expect("document fixture should be selected")
    }

    #[test]
    fn docx_uses_normal_images_when_policy_requests_an_epub_cover() {
        let temp_dir = temp_test_dir("docx-normal-images");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.docx");
        let output_dir = temp_dir.join("output");
        write_docx(
            &input_path,
            &[("word/media/image.png", b"\x89PNG\r\n\x1A\n")],
        );

        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Png]),
                None,
                None,
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let outcome = extraction.extract(document);

        let DocumentExtractionOutcome::Completed(facts) = outcome else {
            panic!("valid DOCX extraction should complete");
        };
        assert_eq!(facts.get_emitted_images(), 1);
        assert!(facts.is_normal_image_output_present());
        assert!(facts.get_warnings().is_empty());
        assert!(output_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn failed_extraction_retains_document_extraction_facts() {
        let temp_dir = temp_test_dir("partial-failure");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.docx");
        let output_dir = temp_dir.join("output");
        let blocked_gif_output = temp_dir.join("blocked-gifs");
        fs::write(&blocked_gif_output, b"not a directory")
            .expect("blocked GIF destination should be creatable");
        write_docx(
            &input_path,
            &[
                ("word/media/first.png", b"not actually a png"),
                ("word/media/second.gif", b"GIF89a"),
            ],
        );

        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::NormalImages,
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Png, ImageFormat::Gif]),
                None,
                Some(blocked_gif_output),
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let DocumentExtractionOutcome::Failed { facts, error } = extraction.extract(document)
        else {
            panic!("blocked GIF destination should fail Document extraction");
        };

        assert_eq!(facts.get_emitted_images(), 1);
        assert!(facts.is_normal_image_output_present());
        assert_eq!(
            facts
                .get_warnings()
                .iter()
                .map(DocumentExtractionWarning::get_message)
                .collect::<Vec<_>>(),
            vec!["Magic detection failed for word/media/first.png; falling back to .png extension"]
        );
        assert!(
            error
                .to_string()
                .contains("Failed to create output directory")
        );
        assert!(output_dir.join("sample_1.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn docx_warning_bodies_keep_source_format_base_name_detail_multiplicity_and_phase_order() {
        let temp_dir = temp_test_dir("docx-warning-bodies");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.docx");
        let output_dir = temp_dir.join("output");
        let corrupt_png = b"\x89PNG\r\n\x1A\nnot an image at all";
        write_docx(
            &input_path,
            &[
                ("word/media/extension.png", b"extension-only payload"),
                ("word/media/corrupt-one.png", corrupt_png),
                ("word/media/corrupt-two.png", corrupt_png),
                (
                    "word/media/vector.svg",
                    b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
                ),
            ],
        );
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::NormalImages,
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Png, ImageFormat::Svg]),
                Some(conversion_policy(ConversionTarget::Jpg)),
                None,
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
            panic!("warning-producing DOCX extraction should complete");
        };

        assert_eq!(facts.get_emitted_images(), 4);
        assert_eq!(facts.get_converted_images(), 0);
        assert_eq!(facts.get_skipped_conversions(), 4);
        assert_eq!(
            warning_messages(&facts),
            vec![
                "Magic detection failed for word/media/extension.png; falling back to .png extension",
                "Skipping conversion for sample (png format not supported for conversion)",
                "Conversion failed for image in sample: Failed to decode image",
                "Conversion failed for image in sample: Failed to decode image",
                "Skipping conversion for sample (svg format not supported for conversion)",
            ]
        );

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn epub_cover_warning_bodies_keep_declared_mime_and_filtered_format() {
        let temp_dir = temp_test_dir("epub-cover-discovery-warning-bodies");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

        let unidentified_path = temp_dir.join("unidentified.epub");
        let unidentified_output = temp_dir.join("unidentified-output");
        write_epub_fixture(
            &unidentified_path,
            &[(
                "cover",
                "images/art.bin",
                "application/x-cover-art",
                Some("cover-image"),
            )],
            &[("OEBPS/images/art.bin", b"unidentified cover bytes")],
        );
        let unidentified_extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );
        let unidentified = select_one_document(&unidentified_path, &unidentified_output);
        let DocumentExtractionOutcome::Completed(unidentified_facts) =
            unidentified_extraction.extract(unidentified)
        else {
            panic!("unidentified required cover should default and complete");
        };
        assert_eq!(
            warning_messages(&unidentified_facts),
            vec![
                "Cover image MIME 'application/x-cover-art' could not be identified; defaulting to .jpg extension."
            ]
        );
        assert_eq!(unidentified_facts.get_emitted_images(), 1);

        let filtered_path = temp_dir.join("filtered.epub");
        let filtered_output = temp_dir.join("filtered-output");
        write_epub_fixture(
            &filtered_path,
            &[("cover", "images/art.png", "image/png", Some("cover-image"))],
            &[("OEBPS/images/art.png", b"\x89PNG\r\n\x1A\n")],
        );
        let filtered_extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );
        let filtered = select_one_document(&filtered_path, &filtered_output);
        let DocumentExtractionOutcome::Completed(filtered_facts) =
            filtered_extraction.extract(filtered)
        else {
            panic!("filtered required cover should complete without emission");
        };
        assert_eq!(
            warning_messages(&filtered_facts),
            vec!["Cover image format 'png' not in allowed formats, skipping."]
        );
        assert_eq!(filtered_facts.get_emitted_images(), 0);

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn epub_cover_conversion_warning_bodies_keep_format_and_lower_error_detail() {
        let temp_dir = temp_test_dir("epub-cover-conversion-warning-bodies");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");

        let unsupported_path = temp_dir.join("unsupported.epub");
        let unsupported_output = temp_dir.join("unsupported-output");
        write_epub_fixture(
            &unsupported_path,
            &[(
                "cover",
                "images/art.svg",
                "image/svg+xml",
                Some("cover-image"),
            )],
            &[(
                "OEBPS/images/art.svg",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
            )],
        );
        let unsupported_extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Svg]),
                Some(conversion_policy(ConversionTarget::Jpg)),
                None,
            )),
        );
        let unsupported = select_one_document(&unsupported_path, &unsupported_output);
        let DocumentExtractionOutcome::Completed(unsupported_facts) =
            unsupported_extraction.extract(unsupported)
        else {
            panic!("unsupported required-cover conversion should complete");
        };
        assert_eq!(
            warning_messages(&unsupported_facts),
            vec!["Cover image format 'svg' not supported for conversion, skipping cover."]
        );
        assert_eq!(unsupported_facts.get_emitted_images(), 0);

        let failed_path = temp_dir.join("failed.epub");
        let failed_output = temp_dir.join("failed-output");
        write_epub_fixture(
            &failed_path,
            &[("cover", "images/art.png", "image/png", Some("cover-image"))],
            &[(
                "OEBPS/images/art.png",
                b"\x89PNG\r\n\x1A\nnot an image at all",
            )],
        );
        let failed_extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Png]),
                Some(conversion_policy(ConversionTarget::Jpg)),
                None,
            )),
        );
        let failed = select_one_document(&failed_path, &failed_output);
        let DocumentExtractionOutcome::Completed(failed_facts) = failed_extraction.extract(failed)
        else {
            panic!("failed required-cover conversion should be non-fatal");
        };
        assert_eq!(
            warning_messages(&failed_facts),
            vec!["Cover conversion failed: Failed to decode image"]
        );
        assert_eq!(failed_facts.get_emitted_images(), 0);

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn epub_cover_retry_warning_bodies_precede_filename_retry_and_normal_fallback() {
        let temp_dir = temp_test_dir("epub-cover-retry-warning-bodies");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("output");
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
            &[("OEBPS/images/page.png", b"extension-only page")],
        );
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg, ImageFormat::Png]),
                None,
                None,
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let DocumentExtractionOutcome::Completed(facts) = extraction.extract(document) else {
            panic!("unreadable cover candidates should allow normal-image fallback");
        };

        assert_eq!(facts.get_emitted_images(), 1);
        assert!(facts.is_normal_image_output_present());
        assert_eq!(
            warning_messages(&facts),
            vec![
                "Could not read archive resource 'OEBPS/images/missing.png': EPUB resource not found: OEBPS/images/missing.png",
                "Could not read archive resource 'OEBPS/images/cover.jpg': EPUB resource not found: OEBPS/images/cover.jpg",
                "Magic detection failed for OEBPS/images/page.png; falling back to .png extension",
            ]
        );

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn epub_cover_output_is_not_classified_as_normal_images() {
        let temp_dir = temp_test_dir("epub-cover-purpose");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("output");
        write_epub(
            &input_path,
            "images/art.jpg",
            Some("cover-image"),
            b"\xFF\xD8\xFF",
        );
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: false,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
            panic!("valid EPUB cover extraction should complete");
        };

        assert_eq!(result.get_emitted_images(), 1);
        assert!(!result.is_normal_image_output_present());
        assert!(output_dir.join("Test.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn epub_cover_fallback_is_classified_as_normal_images() {
        let temp_dir = temp_test_dir("epub-fallback-purpose");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("output");
        write_epub(&input_path, "images/interior.jpg", None, b"\xFF\xD8\xFF");
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::EpubCover {
                fallback_to_normal_images: true,
            },
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
            panic!("EPUB cover fallback should complete");
        };

        assert_eq!(result.get_emitted_images(), 1);
        assert!(result.is_normal_image_output_present());
        assert!(output_dir.join("Test.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn normal_policy_extracts_epub_images_through_document_extraction() {
        let temp_dir = temp_test_dir("epub-normal-images");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("output");
        write_epub(
            &input_path,
            "images/interior.jpg",
            Some("cover-image"),
            b"\xFF\xD8\xFF",
        );
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::NormalImages,
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );
        let document = select_one_document(&input_path, &output_dir);

        let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
            panic!("normal EPUB extraction should complete");
        };

        assert_eq!(result.get_emitted_images(), 1);
        assert!(result.is_normal_image_output_present());
        assert!(output_dir.join("Test.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn retained_epub_declarations_are_authoritative_during_extraction() {
        let temp_dir = temp_test_dir("retained-epub-declarations");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("output");
        let selected_payload = b"\xFF\xD8\xFFselected";
        let replacement_payload = b"\xFF\xD8\xFFreplacement";
        let archive_resources = [
            ("OEBPS/images/selected.jpg", selected_payload.as_slice()),
            (
                "OEBPS/images/replacement.jpg",
                replacement_payload.as_slice(),
            ),
        ];
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        write_epub_with_resources(&input_path, "images/selected.jpg", None, &archive_resources);
        let mut observer = SilentDocumentSelectionObserver;
        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&input_path),
                recursive: false,
                output: Some(&output_dir),
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );
        assert_eq!(selected.len(), 1);

        write_epub_with_resources(
            &input_path,
            "images/replacement.jpg",
            None,
            &archive_resources,
        );
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::NormalImages,
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );

        let document = selected
            .into_iter()
            .next()
            .expect("EPUB fixture should be selected");
        let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
            panic!("retained EPUB declarations should support extraction");
        };

        assert_eq!(result.get_emitted_images(), 1);
        assert_eq!(
            fs::read(output_dir.join("Test.jpg")).expect("selected image should be readable"),
            selected_payload
        );

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn selection_declaration_failure_is_retried_without_revising_selected_identity() {
        let temp_dir = temp_test_dir("retry-epub-declarations");
        let input_path = temp_dir.join("sample.epub");
        let output_dir = temp_dir.join("output");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        fs::write(&input_path, b"not an EPUB").expect("invalid EPUB should be writable");
        let mut observer = SilentDocumentSelectionObserver;
        let selected = select_documents(
            DocumentSelectionOptions {
                inputs: std::slice::from_ref(&input_path),
                recursive: false,
                output: Some(&output_dir),
                epub_filter: &EpubFilter::default(),
            },
            &mut observer,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].get_display_name(), "sample.epub");

        write_epub(
            &input_path,
            "images/recovered.jpg",
            None,
            b"\xFF\xD8\xFFrecovered",
        );
        let extraction = DocumentExtraction::new(
            DocumentExtractionPolicy::NormalImages,
            ImageWritePipeline::new(ImageWritePolicy::new(
                HashSet::from([ImageFormat::Jpg]),
                None,
                None,
            )),
        );

        let document = selected
            .into_iter()
            .next()
            .expect("EPUB fixture should be selected");
        let DocumentExtractionOutcome::Completed(result) = extraction.extract(document) else {
            panic!("Document extraction should retry unavailable EPUB declarations");
        };

        assert_eq!(result.get_emitted_images(), 1);
        assert_eq!(
            fs::read(output_dir.join("sample.jpg")).expect("recovered image should be readable"),
            b"\xFF\xD8\xFFrecovered"
        );

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }
}
