//! Tests for Document identity.
//!
//! Every case here is built from plain strings and paths that are never opened, so
//! the identity rules are asserted without writing an EPUB to disk. Document
//! selection's own tests keep one on-disk case per phase to prove the wiring.

use super::*;

/// Builds an EPUB identity from string declarations at a fixed, unopened path.
fn epub(creator: Option<&str>, title: Option<&str>) -> DocumentIdentity {
    DocumentIdentity::of_epub(creator, title, Path::new("books/sample.epub"))
}

/// Builds case-folded filter terms from the requested title and author.
fn terms(title: Option<&str>, author: Option<&str>) -> EpubFilterTerms {
    EpubFilterTerms::new(&EpubFilter {
        title: title.map(str::to_string),
        author: author.map(str::to_string),
    })
}

#[test]
fn base_name_joins_creator_and_title() {
    assert_eq!(
        epub(Some("Stephen King"), Some("The Shining")).base_name(),
        "Stephen King - The Shining"
    );
}

#[test]
fn base_name_uses_title_alone_when_no_creator_is_declared() {
    assert_eq!(epub(None, Some("The Shining")).base_name(), "The Shining");
}

#[test]
fn base_name_uses_creator_alone_when_no_title_is_declared() {
    assert_eq!(epub(Some("Stephen King"), None).base_name(), "Stephen King");
}

#[test]
fn base_name_falls_back_to_the_file_stem_when_nothing_is_declared() {
    assert_eq!(epub(None, None).base_name(), "sample");
}

#[test]
fn base_name_treats_blank_declarations_as_undeclared() {
    assert_eq!(epub(Some("  "), Some("")).base_name(), "sample");
}

#[test]
fn base_name_trims_declarations_before_joining() {
    assert_eq!(
        epub(Some("  Stephen King "), Some(" The Shining  ")).base_name(),
        "Stephen King - The Shining"
    );
}

#[test]
fn base_name_sanitizes_declared_text() {
    assert_eq!(
        epub(Some("Author/Name"), Some("Title:Subtitle")).base_name(),
        "Author_Name - Title_Subtitle"
    );
}

/// Pins a difference that predates Document identity: an undeclared EPUB's stem is
/// sanitized, which trims it, while a DOCX's stem is used exactly as found.
#[test]
fn only_the_undeclared_epub_fallback_sanitizes_its_stem() {
    let docx = DocumentIdentity::of_path(Path::new(" spaced .docx"));
    let epub = DocumentIdentity::of_epub(None, None, Path::new(" spaced .epub"));

    assert_eq!(docx.base_name(), " spaced ");
    assert_eq!(epub.base_name(), "spaced");
}

#[test]
fn display_name_of_a_declared_identity_is_its_sanitized_base_name() {
    assert_eq!(
        epub(Some("Tester"), Some("Magic: Test")).display_name(),
        "Tester - Magic_ Test"
    );
}

#[test]
fn display_name_of_a_path_identity_keeps_the_extension() {
    assert_eq!(epub(None, Some(" ")).display_name(), "sample.epub");
    assert_eq!(
        DocumentIdentity::of_path(Path::new("docs/report.docx")).display_name(),
        "report.docx"
    );
}

#[test]
fn missing_declarations_give_the_same_identity_as_declaring_nothing() {
    let path = Path::new("books/sample.epub");

    assert_eq!(
        DocumentIdentity::of_epub_declarations(None, path),
        DocumentIdentity::of_epub(None, None, path)
    );
}

#[test]
fn dedupe_key_ignores_case_and_surrounding_whitespace() {
    let first = DocumentIdentity::of_epub(
        Some(" Shared Creator "),
        Some("SHARED TITLE"),
        Path::new("first.epub"),
    );
    let second = DocumentIdentity::of_epub(
        Some("shared creator"),
        Some("Shared Title"),
        Path::new("second.epub"),
    );

    assert_eq!(first.dedupe_key(), second.dedupe_key());
}

#[test]
fn dedupe_key_of_a_blank_title_comes_from_the_file_name() {
    let first = DocumentIdentity::of_epub(None, Some(""), Path::new("first.epub"));
    let second = DocumentIdentity::of_epub(None, Some(""), Path::new("second.epub"));

    assert_eq!(
        first.dedupe_key(),
        DedupeKey::Path {
            file_name: "first.epub".to_string()
        }
    );
    assert_ne!(first.dedupe_key(), second.dedupe_key());
}

#[test]
fn dedupe_key_of_undeclared_epubs_ignores_their_directory() {
    let first = DocumentIdentity::of_epub(None, None, Path::new("first/Book.epub"));
    let second = DocumentIdentity::of_epub(None, None, Path::new("second/book.epub"));

    assert_eq!(first.dedupe_key(), second.dedupe_key());
}

#[test]
fn declared_dedupe_key_never_equals_a_file_name_key() {
    let declared = DocumentIdentity::of_epub(None, Some("book.epub"), Path::new("other.epub"));
    let undeclared = DocumentIdentity::of_epub(None, None, Path::new("book.epub"));

    assert_ne!(declared.dedupe_key(), undeclared.dedupe_key());
}

#[test]
fn matches_title_and_author_terms_as_case_insensitive_substrings() {
    let identity = epub(Some("Test Author"), Some("Magic Book"));

    assert!(identity.matches(&terms(Some("MAGIC"), None)));
    assert!(identity.matches(&terms(None, Some("author"))));
    assert!(identity.matches(&terms(Some("book"), Some("test"))));
    assert!(!identity.matches(&terms(Some("magic"), Some("someone else"))));
}

#[test]
fn matches_requires_the_filtered_declaration_to_exist() {
    assert!(!epub(None, Some("Magic Book")).matches(&terms(None, Some("a"))));
    assert!(!epub(Some("Test Author"), None).matches(&terms(Some("a"), None)));
}

#[test]
fn a_path_identity_matches_no_requested_term_not_even_an_empty_one() {
    assert!(!epub(None, Some("   ")).matches(&terms(Some(""), None)));
}

#[test]
fn matches_keeps_filter_whitespace_but_trims_declarations() {
    let identity = epub(None, Some("  Magic Book  "));

    assert!(identity.matches(&terms(Some(" book"), None)));
    assert!(!identity.matches(&terms(Some("book "), None)));
}
