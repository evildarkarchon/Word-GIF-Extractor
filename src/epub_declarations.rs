//! Shared acquisition of payload-free facts declared by EPUB documents.

use epub::doc::{DocError, EpubDoc};
use std::fmt;
use std::path::{Path, PathBuf};

/// Complete payload-free facts declared by one readable EPUB document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpubDeclarations {
    title: Option<String>,
    creator: Option<String>,
    cover_id: Option<String>,
    resources: Vec<EpubResourceDeclaration>,
}

impl EpubDeclarations {
    /// Acquires all declaration facts needed by Document selection and Document extraction.
    ///
    /// Resource payloads are not read; ADR-0001 reserves those reads for the
    /// independently opened EPUB resource archive.
    pub(crate) fn acquire(path: &Path) -> Result<Self, EpubDeclarationError> {
        let doc = EpubDoc::new(path).map_err(EpubDeclarationError::new)?;
        let title = doc.mdata("title").map(|metadata| metadata.value.clone());
        let creator = doc.mdata("creator").map(|metadata| metadata.value.clone());
        let cover_id = doc.get_cover_id();
        let resources = doc
            .resources
            .iter()
            .map(|(id, item)| EpubResourceDeclaration::new(id, &item.path, &item.mime))
            .collect();

        Ok(Self {
            title,
            creator,
            cover_id,
            resources,
        })
    }

    /// Returns the declared title when present.
    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the declared creator when present.
    pub(crate) fn creator(&self) -> Option<&str> {
        self.creator.as_deref()
    }

    /// Returns the declared cover resource identifier when present.
    pub(crate) fn cover_id(&self) -> Option<&str> {
        self.cover_id.as_deref()
    }

    /// Returns every declared resource without acquiring its archive payload.
    pub(crate) fn resources(&self) -> &[EpubResourceDeclaration] {
        &self.resources
    }
}

/// Payload-free declaration of one EPUB manifest resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpubResourceDeclaration {
    id: String,
    path: PathBuf,
    mime: String,
}

impl EpubResourceDeclaration {
    /// Captures one payload-free resource declaration from an EPUB manifest.
    pub(crate) fn new(
        id: impl Into<String>,
        path: impl Into<PathBuf>,
        mime: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            mime: mime.into(),
        }
    }

    /// Returns the manifest identifier used by cover declarations and spine references.
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Returns the manifest path before archive payload resolution.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the media type declared by the EPUB manifest.
    pub(crate) fn mime(&self) -> &str {
        &self.mime
    }
}

/// Failure to acquire declarations from an EPUB document.
#[derive(Debug)]
pub(crate) struct EpubDeclarationError {
    source: DocError,
}

impl EpubDeclarationError {
    /// Preserves the EPUB parser failure for workflow-specific translation.
    fn new(source: DocError) -> Self {
        Self { source }
    }
}

impl fmt::Display for EpubDeclarationError {
    /// Formats a presentation-free declaration acquisition failure.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Failed to open EPUB file: {}", self.source)
    }
}

impl std::error::Error for EpubDeclarationError {
    /// Returns the underlying EPUB parser error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    /// Returns an isolated temporary directory for one EPUB declaration test.
    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "word-image-extractor-epub-declarations-{test_name}-{}-{nanos}",
            std::process::id()
        ))
    }

    const CONTAINER_XML: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    /// Writes a minimal EPUB from the supplied package declarations and payload entries.
    fn write_epub(path: &Path, package: &[u8], resources: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("test EPUB should be creatable");
        let mut archive = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        archive
            .start_file("mimetype", options)
            .expect("mimetype entry should start");
        archive
            .write_all(b"application/epub+zip")
            .expect("mimetype should be writable");
        archive
            .start_file("META-INF/container.xml", options)
            .expect("container entry should start");
        archive
            .write_all(CONTAINER_XML)
            .expect("container should be writable");
        archive
            .start_file("OEBPS/content.opf", options)
            .expect("OPF entry should start");
        archive.write_all(package).expect("OPF should be writable");
        for (name, payload) in resources {
            archive
                .start_file(*name, options)
                .expect("resource entry should start");
            archive
                .write_all(payload)
                .expect("resource payload should be writable");
        }
        archive.finish().expect("test EPUB should finish");
    }

    /// Writes an EPUB with descriptive declarations and one declared cover resource.
    fn write_epub_with_cover(path: &Path) {
        write_epub(
            path,
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">declarations-test</dc:identifier>
    <dc:title>Retained Title</dc:title>
    <dc:creator>Retained Creator</dc:creator>
  </metadata>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
  </manifest>
  <spine></spine>
</package>"#,
            &[("OEBPS/cover.png", b"cover payload")],
        );
    }

    /// Writes a readable EPUB with no descriptive, cover, or resource declarations.
    fn write_sparse_epub(path: &Path) {
        write_epub(
            path,
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">sparse-test</dc:identifier>
  </metadata>
  <manifest></manifest>
  <spine></spine>
</package>"#,
            &[],
        );
    }

    #[test]
    fn acquires_complete_payload_free_epub_declarations() {
        let temp_dir = temp_test_dir("complete");
        let epub_path = temp_dir.join("book.epub");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        write_epub_with_cover(&epub_path);

        let declarations = EpubDeclarations::acquire(&epub_path)
            .expect("complete EPUB declarations should be acquired");

        assert_eq!(declarations.title(), Some("Retained Title"));
        assert_eq!(declarations.creator(), Some("Retained Creator"));
        assert_eq!(declarations.cover_id(), Some("cover"));
        let cover = declarations
            .resources()
            .iter()
            .find(|resource| resource.id() == "cover")
            .expect("cover declaration should be retained");
        assert_eq!(cover.path(), Path::new("OEBPS/cover.png"));
        assert_eq!(cover.mime(), "image/png");

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }

    #[test]
    fn sparse_epub_declarations_are_a_successful_acquisition() {
        let temp_dir = temp_test_dir("sparse");
        let epub_path = temp_dir.join("book.epub");
        fs::create_dir_all(&temp_dir).expect("temporary directory should be creatable");
        write_sparse_epub(&epub_path);

        let declarations = EpubDeclarations::acquire(&epub_path)
            .expect("sparse EPUB declarations should still be acquired");

        assert_eq!(declarations.title(), None);
        assert_eq!(declarations.creator(), None);
        assert_eq!(declarations.cover_id(), None);
        assert!(declarations.resources().is_empty());

        fs::remove_dir_all(temp_dir).expect("temporary directory should be removable");
    }
}
