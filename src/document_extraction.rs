//! Per-document extraction policy, dispatch, and outcomes.

#[path = "docx.rs"]
mod docx;
#[path = "epub.rs"]
mod epub;

use std::path::{Path, PathBuf};

use crate::image_write_pipeline::{ImageWritePipeline, ImageWriteResult};

/// Opaque handoff from Document selection to Document extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedDocument {
    kind: SelectedDocumentKind,
    path: PathBuf,
    output_dir: PathBuf,
    base_name: String,
    display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedDocumentKind {
    Docx,
    Epub,
}

impl SelectedDocument {
    /// Creates the extraction handoff for one selected DOCX document.
    pub(crate) fn docx(
        path: PathBuf,
        output_dir: PathBuf,
        base_name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        Self {
            kind: SelectedDocumentKind::Docx,
            path,
            output_dir,
            base_name: base_name.into(),
            display_name: display_name.into(),
        }
    }

    /// Creates the extraction handoff for one selected EPUB document.
    pub(crate) fn epub(
        path: PathBuf,
        output_dir: PathBuf,
        base_name: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Self {
        Self {
            kind: SelectedDocumentKind::Epub,
            path,
            output_dir,
            base_name: base_name.into(),
            display_name: display_name.into(),
        }
    }

    /// Returns the source path visible to the Extraction run.
    pub(crate) fn get_path(&self) -> &Path {
        &self.path
    }

    /// Returns the progress identity visible to the Extraction run.
    pub(crate) fn get_display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the output base name for Document selection interface tests.
    #[cfg(test)]
    pub(crate) fn get_base_name(&self) -> &str {
        &self.base_name
    }
}

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
    pub(crate) fn is_epub_cover_only(self) -> bool {
        matches!(self, Self::EpubCover { .. })
    }

    /// Returns whether failed EPUB cover discovery should fall back to normal images.
    fn is_epub_cover_fallback_enabled(self) -> bool {
        match self {
            Self::NormalImages => false,
            Self::EpubCover {
                fallback_to_normal_images,
            } => fallback_to_normal_images,
        }
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

    /// Returns the validated policy shared by selection and extraction.
    pub(crate) fn get_policy(&self) -> DocumentExtractionPolicy {
        self.policy
    }

    /// Extracts one selected document through its format adapter.
    ///
    /// Document-local errors are returned as failed outcomes so the Extraction
    /// run can continue processing later documents.
    pub(crate) fn extract(&self, document: &SelectedDocument) -> DocumentExtractionOutcome {
        let result = match document.kind {
            SelectedDocumentKind::Docx => docx::process_file(
                &document.path,
                &document.output_dir,
                &document.base_name,
                &self.image_write_pipeline,
            ),
            SelectedDocumentKind::Epub => epub::process_file(
                &document.path,
                &document.output_dir,
                &document.base_name,
                self.policy.is_epub_cover_only(),
                self.policy.is_epub_cover_fallback_enabled(),
                &self.image_write_pipeline,
            ),
        };

        match result {
            Ok(result) => DocumentExtractionOutcome::Completed(result),
            Err(failure) => DocumentExtractionOutcome::Failed {
                partial: failure.partial,
                error: failure.error,
            },
        }
    }
}

/// Terminal result of extracting one selected document.
pub(crate) enum DocumentExtractionOutcome {
    /// Extraction completed with its Image write facts.
    Completed(ImageWriteResult),
    /// Extraction failed after retaining any Image write facts already produced.
    Failed {
        /// Image write facts produced before the document-local failure.
        partial: ImageWriteResult,
        /// Contextual document-local error.
        error: anyhow::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_format::ImageFormat;
    use crate::image_write_pipeline::{ImageWritePipeline, ImageWritePolicy, ImageWriteWarning};
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
        let properties = properties
            .map(|value| format!(" properties=\"{value}\""))
            .unwrap_or_default();
        let opf = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">test-book</dc:identifier><dc:title>Test</dc:title>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="image" href="{image_href}" media-type="image/jpeg"{properties}/>
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
        zip.start_file(format!("OEBPS/{image_href}"), options)
            .expect("image entry should start");
        zip.write_all(data).expect("image should be writable");
        zip.finish().expect("test EPUB should finish");
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
        let document =
            SelectedDocument::docx(input_path, output_dir.clone(), "sample", "sample.docx");

        let outcome = extraction.extract(&document);

        let DocumentExtractionOutcome::Completed(result) = outcome else {
            panic!("valid DOCX extraction should complete");
        };
        assert_eq!(result.counts.extracted, 1);
        assert!(output_dir.join("sample.png").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn failed_extraction_retains_partial_image_write_facts() {
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
        let document =
            SelectedDocument::docx(input_path, output_dir.clone(), "sample", "sample.docx");

        let DocumentExtractionOutcome::Failed { partial, error } = extraction.extract(&document)
        else {
            panic!("blocked GIF destination should fail Document extraction");
        };

        assert_eq!(partial.counts.extracted, 1);
        assert!(partial.has_normal_image_output());
        assert_eq!(
            partial.warnings,
            vec![ImageWriteWarning::ExtensionFallback {
                source_name: "word/media/first.png".to_string(),
                format: ImageFormat::Png,
            }]
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
        let document = SelectedDocument::epub(input_path, output_dir.clone(), "sample", "Test");

        let DocumentExtractionOutcome::Completed(result) = extraction.extract(&document) else {
            panic!("valid EPUB cover extraction should complete");
        };

        assert_eq!(result.counts.extracted, 1);
        assert!(!result.has_normal_image_output());
        assert!(output_dir.join("sample.jpg").exists());

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
        let document = SelectedDocument::epub(input_path, output_dir.clone(), "sample", "Test");

        let DocumentExtractionOutcome::Completed(result) = extraction.extract(&document) else {
            panic!("EPUB cover fallback should complete");
        };

        assert_eq!(result.counts.extracted, 1);
        assert!(result.has_normal_image_output());
        assert!(output_dir.join("sample.jpg").exists());

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
        let document = SelectedDocument::epub(input_path, output_dir.clone(), "sample", "Test");

        let DocumentExtractionOutcome::Completed(result) = extraction.extract(&document) else {
            panic!("normal EPUB extraction should complete");
        };

        assert_eq!(result.counts.extracted, 1);
        assert!(result.has_normal_image_output());
        assert!(output_dir.join("sample.jpg").exists());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }
}
