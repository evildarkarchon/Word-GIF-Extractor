//! Tests for EPUB cover extraction's ordering, exclusion, retry, and fallback decisions.
//!
//! Every test opens a real EPUB resource archive session from a temporary ZIP with
//! hand-built resource declarations, runs a real Image write pipeline, and writes into
//! a temporary output directory. Which candidate was attempted is observed through
//! what a user could see: the warnings, the Emitted image tally, the facts retained on
//! a failure, and the files on disk (ADR-0021). Failure paths use real inputs too: a
//! missing archive entry is an unreadable cover, a corrupted stored payload fails its
//! read, and a file standing where a directory should be fails emission.

use super::*;
use crate::conversion::ConversionTarget;
use crate::epub_declarations::EpubResourceDeclaration;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::{ImageWritePolicy, ImageWriteWarning};
use crate::test_support::{
    MINIMAL_PNG, TempTestPath, acquisition_failure_sources, corrupt_stored_payload, file_names_in,
    pipeline_accepting, pipeline_converting, temp_test_dir, valid_png, write_stored_epub_fixture,
};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use super::super::resource_archive::EpubResourceArchive;

/// A JPEG payload whose magic bytes identify it and whose tail makes it unique.
fn jpeg(tag: &str) -> Vec<u8> {
    let mut payload = b"\xFF\xD8\xFF".to_vec();
    payload.extend_from_slice(tag.as_bytes());
    payload
}

/// One hand-built manifest declaration, addressed from the archive root.
fn declared(id: &str, path: &str, mime: &str) -> EpubResourceDeclaration {
    EpubResourceDeclaration::new(id, path, mime)
}

/// A temporary EPUB, an output directory that already exists, and their parent.
///
/// The output directory is created up front so that an empty listing is evidence
/// that nothing was written, rather than a property of a path nobody created.
struct CoverFixture {
    epub: PathBuf,
    output_dir: PathBuf,
    root: TempTestPath,
}

impl CoverFixture {
    /// Writes a stored-payload ZIP holding `entries` and creates the output directory.
    ///
    /// The stored-EPUB builder is used only for its uncompressed ZIP: declarations
    /// are hand-built by each test, so the fixture's own manifest is left empty.
    fn new(test_name: &str, entries: &[(&str, &[u8])]) -> Self {
        let root = temp_test_dir("cover-extraction", test_name);
        let output_dir = root.join("out");
        fs::create_dir_all(&output_dir).expect("output directory should be creatable");
        let epub = root.join("sample.epub");
        write_stored_epub_fixture(&epub, &[], entries);
        Self {
            epub,
            output_dir,
            root,
        }
    }

    /// Places output under this fixture's output directory with the base name `Book`.
    fn placement(&self) -> OutputPlacement {
        OutputPlacement::new(&self.output_dir, "Book")
    }

    /// Returns a placement whose output directory is a file, so emission there fails.
    fn blocked_placement(&self) -> OutputPlacement {
        let blocked = self.root.join("blocked-output");
        fs::write(&blocked, b"not a directory").expect("blocking file should be creatable");
        OutputPlacement::new(blocked, "Book")
    }

    /// Returns a GIF destination path occupied by a file, so routing a GIF there fails.
    fn blocked_gif_destination(&self) -> PathBuf {
        let blocked = self.root.join("blocked-gifs");
        fs::write(&blocked, b"not a directory").expect("blocking file should be creatable");
        blocked
    }

    /// Opens the fixture as a real EPUB resource archive and runs cover extraction on it.
    fn extract(
        &self,
        declarations: &[EpubResourceDeclaration],
        declared_cover_id: Option<&str>,
        cover_policy: EpubCoverPolicy,
        pipeline: &ImageWritePipeline,
        placement: &OutputPlacement,
    ) -> ImageWriteOutcome {
        EpubResourceArchive::open(&self.epub, declarations, |catalog, mut payloads| {
            extract_required_cover(
                catalog,
                &mut payloads,
                pipeline,
                placement,
                declared_cover_id,
                cover_policy,
            )
        })
        .expect("test EPUB resource archive should open")
    }

    /// Returns the names of the files written to the output directory, sorted.
    fn written_files(&self) -> Vec<String> {
        file_names_in(&self.output_dir)
    }
}

/// The non-fatal warning an unreadable cover records for a declared but absent entry.
fn not_found(source_name: &str) -> ImageWriteWarning {
    ImageWriteWarning::archive_image_acquisition_failed(
        source_name,
        format!("EPUB resource not found: {source_name}"),
    )
}

#[test]
fn the_declared_cover_is_attempted_before_the_filename_candidate() {
    // The filename candidate deliberately sorts earlier in the catalog, so an attempt
    // order that merely followed catalog order would attempt it first.
    let fixture = CoverFixture::new("declared-before-filename", &[]);
    let declarations = [
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
        declared("metadata-cover", "OEBPS/images/z-art.png", "image/png"),
    ];

    let result = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverOnly,
            &pipeline_accepting([ImageFormat::Jpg, ImageFormat::Png]),
            &fixture.placement(),
        )
        .expect("retried cover candidates should complete without fallback");

    assert_eq!(
        result.warnings,
        vec![
            not_found("OEBPS/images/z-art.png"),
            not_found("OEBPS/images/cover.jpg"),
        ]
    );
    assert_eq!(result.tally.emitted(), 0);
}

#[test]
fn the_filename_cover_is_the_first_deterministic_jpeg_family_candidate() {
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
        let entry = format!("OEBPS/images/CoVeR.{extension}");
        let payload = jpeg("filename-candidate");
        let fixture = CoverFixture::new(
            &format!("filename-extension-{extension}"),
            &[(entry.as_str(), &payload)],
        );

        let result = fixture
            .extract(
                &[declared("filename-candidate", &entry, "image/jpeg")],
                None,
                EpubCoverPolicy::CoverOnly,
                &pipeline_accepting([ImageFormat::Jpg]),
                &fixture.placement(),
            )
            .expect("a filename-cover decision should complete normally");

        // An attempted candidate is a readable JPEG, so attempting it writes it; a
        // candidate the heuristic rejects is never read and leaves no trace.
        let expected_files = if should_match {
            vec!["Book.jpg".to_string()]
        } else {
            Vec::new()
        };
        assert_eq!(
            fixture.written_files(),
            expected_files,
            "unexpected filename-cover decision for .{extension}"
        );
        assert_eq!(result.tally.covers(), usize::from(should_match));
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    // Two JPEG-family covers: the earlier catalog entry wins, so the decision is fixed
    // by the catalog's deterministic ordering rather than by which one is found first.
    let first = jpeg("first-cover");
    let later = jpeg("later-cover");
    let fixture = CoverFixture::new(
        "filename-first-deterministic",
        &[("a/cover.jpeg", &first), ("z/cover.jpg", &later)],
    );

    let result = fixture
        .extract(
            &[
                declared("later", "z/cover.jpg", "image/jpeg"),
                declared("first", "a/cover.jpeg", "image/jpeg"),
            ],
            None,
            EpubCoverPolicy::CoverOnly,
            &pipeline_accepting([ImageFormat::Jpg]),
            &fixture.placement(),
        )
        .expect("the first deterministic filename candidate should complete the decision");

    assert_eq!(result.tally.covers(), 1);
    assert_eq!(fixture.written_files(), vec!["Book.jpg".to_string()]);
    assert_eq!(
        fs::read(fixture.output_dir.join("Book.jpg")).unwrap(),
        first
    );
}

#[test]
fn a_completed_cover_is_terminal_before_the_filename_candidate_and_fallback() {
    // A filtered cover, an unsupported cover conversion and a failed cover conversion
    // are three Image write policy decisions that reach this module as one disposition.
    // The module must treat each as final, whatever fact the attempt carried back.
    let cases = [
        (
            "filtered",
            "OEBPS/images/art.png",
            "image/png",
            MINIMAL_PNG.to_vec(),
            pipeline_accepting([ImageFormat::Jpg]),
            ImageWriteWarning::UnsupportedCoverFormat {
                format: ImageFormat::Png,
            },
        ),
        (
            "conversion-skipped",
            "OEBPS/images/art.svg",
            "image/svg+xml",
            b"<svg/>".to_vec(),
            pipeline_converting(
                [ImageFormat::Svg, ImageFormat::Jpg],
                ConversionTarget::Png,
                None,
            ),
            ImageWriteWarning::CoverConversionSkipped {
                format: ImageFormat::Svg,
            },
        ),
        (
            "conversion-failed",
            "OEBPS/images/art.png",
            "image/png",
            b"\x89PNG\r\n\x1A\ninvalid".to_vec(),
            pipeline_converting(
                [ImageFormat::Png, ImageFormat::Jpg],
                ConversionTarget::Jpg,
                None,
            ),
            ImageWriteWarning::CoverConversionFailed {
                detail: "Failed to decode image".to_string(),
            },
        ),
    ];
    for (case, metadata_cover_path, cover_mime, cover_payload, pipeline, warning) in cases {
        let filename_cover = jpeg("filename-cover");
        let page = jpeg("page");
        let fixture = CoverFixture::new(
            &format!("completed-is-terminal-{case}"),
            &[
                (metadata_cover_path, &cover_payload),
                ("OEBPS/images/cover.jpg", &filename_cover),
                ("OEBPS/images/page.jpg", &page),
            ],
        );
        let declarations = [
            declared("metadata-cover", metadata_cover_path, cover_mime),
            declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
            declared("page", "OEBPS/images/page.jpg", "image/jpeg"),
        ];

        let result = fixture
            .extract(
                &declarations,
                Some("metadata-cover"),
                EpubCoverPolicy::CoverThenNormalImages,
                &pipeline,
                &fixture.placement(),
            )
            .expect("a completed cover attempt should end the decision normally");

        // Both the filename candidate and the fallback page are readable JPEGs the
        // pipeline accepts, so reaching either would have written a file.
        assert!(fixture.written_files().is_empty(), "{case}");
        assert_eq!(result.warnings, vec![warning], "{case}");
        assert_eq!(result.tally.emitted(), 0, "{case}");
        assert_eq!(result.tally.conversion_skipped(), 0, "{case}");
        assert_eq!(result.tally.normal_images(), 0, "{case}");
    }
}

#[test]
fn an_unreadable_declared_cover_retries_the_filename_candidate() {
    let unreadable_cover = jpeg("uniquely corrupt declared cover");
    let filename_cover = jpeg("filename-cover");
    let fixture = CoverFixture::new(
        "unreadable-declared-retries",
        &[
            ("OEBPS/images/art.jpg", &unreadable_cover),
            ("OEBPS/images/cover.jpg", &filename_cover),
        ],
    );
    corrupt_stored_payload(&fixture.epub, &unreadable_cover);
    let declarations = [
        declared("metadata-cover", "OEBPS/images/art.jpg", "image/jpeg"),
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
    ];

    let result = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverOnly,
            &pipeline_accepting([ImageFormat::Jpg]),
            &fixture.placement(),
        )
        .expect("an unreadable declared cover should allow the filename candidate");

    assert_eq!(result.tally.emitted(), 1);
    assert_eq!(result.tally.normal_images(), 0);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert_eq!(
        acquisition_failure_sources(&result.warnings),
        vec!["OEBPS/images/art.jpg"]
    );
    assert_eq!(
        fs::read(fixture.output_dir.join("Book.jpg")).unwrap(),
        filename_cover
    );
}

#[test]
fn cover_retry_warnings_precede_the_normal_fallback_warning() {
    let fixture = CoverFixture::new(
        "retry-warnings-before-fallback",
        &[("OEBPS/images/page.png", b"not actually a png")],
    );
    let declarations = [
        declared("metadata-cover", "OEBPS/images/missing.png", "image/png"),
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
        declared("page", "OEBPS/images/page.png", "image/png"),
    ];

    let result = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverThenNormalImages,
            &pipeline_accepting([ImageFormat::Png]),
            &fixture.placement(),
        )
        .expect("unreadable cover candidates should allow normal fallback");

    // Each unreadable candidate warns once, so fallback did not revisit either one.
    assert_eq!(
        result.warnings,
        vec![
            not_found("OEBPS/images/missing.png"),
            not_found("OEBPS/images/cover.jpg"),
            ImageWriteWarning::ExtensionFallback {
                source_name: "OEBPS/images/page.png".to_string(),
                format: ImageFormat::Png,
            },
        ]
    );
    assert_eq!(result.tally.emitted(), 1);
    assert!(result.tally.normal_images() > 0);
    assert_eq!(fixture.written_files(), vec!["Book.png".to_string()]);
}

#[test]
fn cover_retries_precede_partial_normal_fallback_facts() {
    // The singular-versus-numbered lookahead emits the converted PNG only once the GIF
    // arrives, and then fails routing the GIF into a destination a file occupies.
    // Before either, fallback visits a declared page with no archive entry, so the
    // partial facts carry a fallback warning whose place after the cover retries'
    // warnings shows the order the scope folded them in.
    let first = valid_png();
    let fixture = CoverFixture::new(
        "retries-before-partial-fallback",
        &[
            ("OEBPS/images/first.png", &first),
            ("OEBPS/images/second.gif", b"GIF89a"),
        ],
    );
    let pipeline = pipeline_converting(
        [ImageFormat::Png, ImageFormat::Gif, ImageFormat::Jpg],
        ConversionTarget::Jpg,
        Some(fixture.blocked_gif_destination()),
    );
    let declarations = [
        declared("metadata-cover", "OEBPS/images/missing.png", "image/png"),
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
        declared("absent-page", "OEBPS/images/absent.png", "image/png"),
        declared("first", "OEBPS/images/first.png", "image/png"),
        declared("second", "OEBPS/images/second.gif", "image/gif"),
    ];

    let failure = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverThenNormalImages,
            &pipeline,
            &fixture.placement(),
        )
        .expect_err("a fallback emission failure should retain earlier normal output");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.tally.emitted(), 1);
    assert_eq!(failure.partial.tally.converted(), 1);
    assert_eq!(failure.partial.tally.gifs_routed(), 0);
    assert_eq!(failure.partial.tally.conversion_skipped(), 0);
    assert!(failure.partial.tally.normal_images() > 0);
    assert_eq!(
        failure.partial.warnings,
        vec![
            not_found("OEBPS/images/missing.png"),
            not_found("OEBPS/images/cover.jpg"),
            not_found("OEBPS/images/absent.png"),
        ]
    );
    assert_eq!(fixture.written_files(), vec!["Book_1.jpg".to_string()]);
}

#[test]
fn a_routed_gif_fact_precedes_a_later_normal_fallback_failure() {
    // The GIF waits on the lookahead until the PNG arrives; it is then routed into its
    // own destination before the PNG fails against an output directory a file occupies.
    let fixture = CoverFixture::new(
        "routed-gif-before-fallback-failure",
        &[
            ("OEBPS/images/a.gif", b"GIF89a"),
            ("OEBPS/images/b.png", MINIMAL_PNG),
        ],
    );
    let gif_dir = fixture.root.join("gifs");
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Gif, ImageFormat::Png]),
        None,
        Some(gif_dir.clone()),
    ));
    let declarations = [
        declared("metadata-cover", "OEBPS/images/missing.png", "image/png"),
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
        declared("animation", "OEBPS/images/a.gif", "image/gif"),
        declared("page", "OEBPS/images/b.png", "image/png"),
    ];

    let failure = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverThenNormalImages,
            &pipeline,
            &fixture.blocked_placement(),
        )
        .expect_err("a later normal emission failure should retain the routed GIF");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.tally.emitted(), 1);
    assert_eq!(failure.partial.tally.gifs_routed(), 1);
    assert_eq!(failure.partial.tally.converted(), 0);
    assert!(failure.partial.tally.normal_images() > 0);
    // The cover warnings come first: the routed GIF is a fallback fact, and fallback
    // ran only after both cover candidates had been tried.
    assert_eq!(
        failure.partial.warnings,
        vec![
            not_found("OEBPS/images/missing.png"),
            not_found("OEBPS/images/cover.jpg"),
        ]
    );
    assert_eq!(fs::read(gif_dir.join("Book_1.gif")).unwrap(), b"GIF89a");
}

#[test]
fn an_already_attempted_identity_is_not_retried() {
    // Three manifest spellings share one payload identity. The declared cover is
    // attempted and fails its read; the filename candidate is another spelling of the
    // same payload, and the third spelling is an ordinary resource for fallback.
    let corrupt_cover = jpeg("uniquely corrupt aliased cover");
    let fixture = CoverFixture::new(
        "attempted-identity-not-retried",
        &[
            ("OEBPS/images/cover.jpg", &corrupt_cover),
            ("OEBPS/images/page.png", MINIMAL_PNG),
        ],
    );
    corrupt_stored_payload(&fixture.epub, &corrupt_cover);
    let declarations = [
        declared("metadata-cover", "OEBPS/images/cover%2Ejpg", "image/jpeg"),
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
        declared("non-cover-alias", "OEBPS/images/cover%2ejpg", "image/jpeg"),
        declared("page", "OEBPS/images/page.png", "image/png"),
    ];

    let result = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverThenNormalImages,
            &pipeline_accepting([ImageFormat::Jpg, ImageFormat::Png]),
            &fixture.placement(),
        )
        .expect("one failed resolved cover should allow normal fallback");

    // One acquisition warning for three spellings is the observable form of one
    // identity: the aliases were neither attempted again nor revisited by fallback,
    // each of which would have warned about the same unreadable payload again.
    //
    // That the aliased payload was not also written as a normal image holds here
    // trivially, because it cannot be read. No readable case exists through the real
    // pipeline: a readable cover completes its attempt, and a completed attempt ends
    // the sequence before fallback, so only an unreadable identity ever reaches
    // fallback as an attempted one. The warning count is what shows the exclusion.
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert_eq!(
        acquisition_failure_sources(&result.warnings),
        vec!["OEBPS/images/cover%2Ejpg"]
    );
    assert_eq!(result.tally.emitted(), 1);
    assert!(result.tally.normal_images() > 0);
    assert_eq!(fixture.written_files(), vec!["Book.png".to_string()]);
    assert_eq!(
        fs::read(fixture.output_dir.join("Book.png")).unwrap(),
        MINIMAL_PNG
    );
}

#[test]
fn a_fatal_cover_emission_stops_the_filename_candidate_and_normal_fallback() {
    let filename_cover = jpeg("filename-cover");
    let fixture = CoverFixture::new(
        "fatal-cover-emission-stops",
        &[
            ("OEBPS/images/art.gif", b"GIF89a"),
            ("OEBPS/images/cover.jpg", &filename_cover),
            ("OEBPS/images/page.png", MINIMAL_PNG),
        ],
    );
    let pipeline = ImageWritePipeline::new(ImageWritePolicy::new(
        HashSet::from([ImageFormat::Gif, ImageFormat::Jpg, ImageFormat::Png]),
        None,
        Some(fixture.blocked_gif_destination()),
    ));
    let declarations = [
        declared("metadata-cover", "OEBPS/images/art.gif", "image/gif"),
        declared("filename-cover", "OEBPS/images/cover.jpg", "image/jpeg"),
        declared("page", "OEBPS/images/page.png", "image/png"),
    ];

    let failure = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverThenNormalImages,
            &pipeline,
            &fixture.placement(),
        )
        .expect_err("a fatal cover emission should abort the decision");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.tally.emitted(), 0);
    assert_eq!(failure.partial.tally.gifs_routed(), 0);
    assert_eq!(failure.partial.tally.normal_images(), 0);
    assert!(failure.partial.warnings.is_empty());
    // The filename candidate and the fallback page both write into the output
    // directory, which stayed empty, so neither was reached.
    assert!(fixture.written_files().is_empty());
}

#[test]
fn a_cover_emission_error_propagates_with_its_retained_facts() {
    let fixture = CoverFixture::new(
        "cover-emission-error-retains-facts",
        &[
            ("OEBPS/images/cover.jpg", &[0x5a; 64]),
            ("OEBPS/images/page.png", b"not actually a png"),
        ],
    );
    let declarations = [
        declared("metadata-cover", "OEBPS/images/missing.png", "image/png"),
        declared(
            "filename-cover",
            "OEBPS/images/cover.jpg",
            "application/octet-stream",
        ),
        declared("page", "OEBPS/images/page.png", "image/png"),
    ];

    let failure = fixture
        .extract(
            &declarations,
            Some("metadata-cover"),
            EpubCoverPolicy::CoverThenNormalImages,
            &pipeline_accepting([ImageFormat::Jpg, ImageFormat::Png]),
            &fixture.blocked_placement(),
        )
        .expect_err("an emission failure must abort cover extraction");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.tally.emitted(), 0);
    // Fallback would have added the page's extension-fallback warning, so its absence
    // shows the failed cover emission ended the sequence.
    assert_eq!(
        failure.partial.warnings,
        vec![
            not_found("OEBPS/images/missing.png"),
            ImageWriteWarning::CoverDefaultToJpeg {
                mime: "application/octet-stream".to_string(),
            },
        ]
    );
}
