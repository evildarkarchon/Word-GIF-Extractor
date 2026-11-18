//! Tests for Office package classification and the media-folder rule.

use super::*;

/// Pins one representative extension of every package, plus the variants a
/// reader would most likely guess wrong.
#[test]
fn classifies_each_suite_by_extension() {
    let cases = [
        ("docx", OfficePackage::Word),
        ("docm", OfficePackage::Word),
        ("dotx", OfficePackage::Word),
        ("pptx", OfficePackage::PowerPoint),
        ("ppsx", OfficePackage::PowerPoint),
        ("potm", OfficePackage::PowerPoint),
        ("xlsx", OfficePackage::Excel),
        ("xlsb", OfficePackage::Excel),
        ("xltx", OfficePackage::Excel),
        ("vsdx", OfficePackage::Visio),
        ("vssx", OfficePackage::Visio),
        ("odt", OfficePackage::OpenDocument),
        ("ods", OfficePackage::OpenDocument),
        ("odp", OfficePackage::OpenDocument),
        ("otg", OfficePackage::OpenDocument),
    ];
    for (extension, package) in cases {
        assert_eq!(
            OfficePackage::of_extension(extension),
            Some(package),
            "{extension}"
        );
    }
}

#[test]
fn classifies_extensions_whatever_their_case() {
    assert_eq!(
        OfficePackage::of_extension("DOCX"),
        Some(OfficePackage::Word)
    );
    assert_eq!(
        OfficePackage::of_extension("Odp"),
        Some(OfficePackage::OpenDocument)
    );
}

/// The binary and flat-XML formats are not archives, and EPUB is its own kind.
#[test]
fn rejects_formats_that_are_not_zip_packaged_office_documents() {
    for extension in [
        "doc", "ppt", "xls", "fodt", "epub", "zip", "pdf", "", "docx ",
    ] {
        assert_eq!(
            OfficePackage::of_extension(extension),
            None,
            "{extension:?}"
        );
    }
}

/// Every table row is lower-case, which `of_extension` relies on only for the
/// tidiness of the table: matching ignores case either way.
#[test]
fn lists_each_extension_once_in_lower_case() {
    let mut seen = std::collections::HashSet::new();
    for (extension, _) in OFFICE_EXTENSIONS {
        assert_eq!(*extension, extension.to_ascii_lowercase());
        assert!(seen.insert(*extension), "{extension} is listed twice");
    }
}

#[test]
fn holds_images_inside_each_packages_media_folder() {
    let cases = [
        (OfficePackage::Word, "word/media/image1.png"),
        (OfficePackage::PowerPoint, "ppt/media/image3.jpeg"),
        (OfficePackage::Excel, "xl/media/image1.emf"),
        (OfficePackage::Visio, "visio/media/image2.png"),
        (
            OfficePackage::OpenDocument,
            "Pictures/100000000000012C0000012C8E2D4F1B.png",
        ),
    ];
    for (package, entry) in cases {
        assert!(package.holds_document_image(entry), "{package:?} {entry}");
    }
}

/// The audit's M5 case: Word's preview thumbnail is not a document image.
#[test]
fn passes_over_preview_thumbnails() {
    assert!(!OfficePackage::Word.holds_document_image("docProps/thumbnail.jpeg"));
    assert!(!OfficePackage::Excel.holds_document_image("docProps/thumbnail.wmf"));
    assert!(!OfficePackage::OpenDocument.holds_document_image("Thumbnails/thumbnail.png"));
}

/// Markup, embedded objects and another suite's media folder are all outside.
#[test]
fn passes_over_entries_outside_the_media_folder() {
    let outside = [
        "word/document.xml",
        "word/embeddings/oleObject1.bin",
        "ppt/media/image1.png",
        "media/image1.png",
        "word/mediafiles/image1.png",
        "word/media",
        "xword/media/image1.png",
    ];
    for entry in outside {
        assert!(!OfficePackage::Word.holds_document_image(entry), "{entry}");
    }
    assert!(!OfficePackage::OpenDocument.holds_document_image("ObjectReplacements/Object 1"));
}

#[test]
fn compares_the_media_folder_ignoring_case() {
    assert!(OfficePackage::Word.holds_document_image("Word/Media/image1.png"));
    assert!(OfficePackage::OpenDocument.holds_document_image("pictures/image.png"));
}

/// A nested folder under the media folder is still inside it.
#[test]
fn holds_images_in_folders_nested_under_the_media_folder() {
    assert!(OfficePackage::PowerPoint.holds_document_image("ppt/media/nested/image1.png"));
}

/// A name whose folder-length prefix ends inside a multi-byte character must be
/// answered, not panic on a char-boundary slice.
#[test]
fn answers_for_a_name_whose_prefix_splits_a_character() {
    // `word/media` is ten bytes and `é` takes two, so the eleven-byte prefix the
    // folder needs ends halfway through it.
    assert!(!OfficePackage::Word.holds_document_image("word/media\u{e9}.png"));
    assert!(!OfficePackage::Word.holds_document_image("\u{e9}"));
}
