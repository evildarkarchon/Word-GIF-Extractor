//! Office packages: which file extensions name an Office document, and which of
//! its archive entries are document images.
//!
//! An Office document is a ZIP-packaged Office Open XML or OpenDocument file. Each
//! suite lays its archive out its own way, but every one keeps the images a reader
//! sees in one media folder: `word/media/` in a Word document, `Pictures/` in an
//! OpenDocument file. Everything else in the archive is markup, metadata or a
//! rendering made for some other reader, such as the preview thumbnail a file
//! manager shows, and is not a document image (ADR-0029).
//!
//! Both facts live here, side by side, because they are one table: the extension
//! decides the package, and the package decides the folder. Document selection asks
//! the first question and Document extraction the second, so neither has to know
//! the other's half.

/// The packaging convention an Office document follows, as its extension names it.
///
/// The package decides which folder of the archive holds the document's images.
/// Macro-enabled, template and show variants of a format share its package, because
/// they share its layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OfficePackage {
    /// A Word document, template or macro-enabled variant (`.docx`, `.dotm`, …).
    Word,
    /// A PowerPoint presentation, template or show (`.pptx`, `.potx`, `.ppsx`, …).
    PowerPoint,
    /// An Excel workbook or template, the binary `.xlsb` workbook included.
    Excel,
    /// A Visio drawing, template or stencil (`.vsdx`, `.vstx`, `.vssx`, …).
    Visio,
    /// An OpenDocument text, presentation, spreadsheet or drawing, or a template of one.
    OpenDocument,
}

/// Every extension that names an Office document, lower-case, with its package.
///
/// Only ZIP-packaged formats belong here. The legacy binary formats (`.doc`, `.ppt`,
/// `.xls`) and the flat OpenDocument XML formats (`.fodt`, …) are not archives, so the
/// adapter that reads Office documents could not open them.
const OFFICE_EXTENSIONS: &[(&str, OfficePackage)] = &[
    ("docx", OfficePackage::Word),
    ("docm", OfficePackage::Word),
    ("dotx", OfficePackage::Word),
    ("dotm", OfficePackage::Word),
    ("pptx", OfficePackage::PowerPoint),
    ("pptm", OfficePackage::PowerPoint),
    ("potx", OfficePackage::PowerPoint),
    ("potm", OfficePackage::PowerPoint),
    ("ppsx", OfficePackage::PowerPoint),
    ("ppsm", OfficePackage::PowerPoint),
    ("xlsx", OfficePackage::Excel),
    ("xlsm", OfficePackage::Excel),
    ("xltx", OfficePackage::Excel),
    ("xltm", OfficePackage::Excel),
    ("xlsb", OfficePackage::Excel),
    ("vsdx", OfficePackage::Visio),
    ("vsdm", OfficePackage::Visio),
    ("vstx", OfficePackage::Visio),
    ("vstm", OfficePackage::Visio),
    ("vssx", OfficePackage::Visio),
    ("vssm", OfficePackage::Visio),
    ("odt", OfficePackage::OpenDocument),
    ("ott", OfficePackage::OpenDocument),
    ("odp", OfficePackage::OpenDocument),
    ("otp", OfficePackage::OpenDocument),
    ("ods", OfficePackage::OpenDocument),
    ("ots", OfficePackage::OpenDocument),
    ("odg", OfficePackage::OpenDocument),
    ("otg", OfficePackage::OpenDocument),
];

impl OfficePackage {
    /// Returns the package an extension names, ignoring ASCII case.
    ///
    /// The extension is given without its leading dot. Returns `None` for an
    /// extension that names no Office document, `epub` included.
    pub(crate) fn of_extension(extension: &str) -> Option<Self> {
        OFFICE_EXTENSIONS
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(extension))
            .map(|&(_, package)| package)
    }

    /// Returns the archive folder that holds this package's document images,
    /// with its trailing separator.
    pub(crate) fn media_folder(self) -> &'static str {
        match self {
            Self::Word => "word/media/",
            Self::PowerPoint => "ppt/media/",
            Self::Excel => "xl/media/",
            Self::Visio => "visio/media/",
            Self::OpenDocument => "Pictures/",
        }
    }

    /// Returns whether the archive entry named `entry_name` is a document image
    /// location for this package, that is, whether it lies inside the media folder.
    ///
    /// The comparison ignores ASCII case. Office Open XML part names are compared
    /// that way by definition, and no OpenDocument producer relies on two folders
    /// differing only in case, so one rule serves both. The name is compared as it
    /// is spelled in the archive and not resolved: an entry name is only format
    /// evidence and warning identity, never an output path (ADR-0020), so a `..`
    /// segment inside the media folder has nothing to escape to.
    ///
    /// Whether the entry's bytes are an image is not decided here. A video in
    /// `ppt/media/` lies inside the folder and is passed over later, by Archive
    /// image discovery, because its bytes identify no Image format.
    pub(crate) fn holds_document_image(self, entry_name: &str) -> bool {
        let folder = self.media_folder();
        // `get` rather than slicing: a byte offset can fall inside a multi-byte
        // character, and such a name is simply not inside the folder.
        entry_name
            .get(..folder.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(folder))
    }
}

#[cfg(test)]
mod tests;
