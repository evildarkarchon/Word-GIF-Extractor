//! Tests for EPUB cover extraction's ordering, exclusion, retry, and fallback decisions.
//!
//! Every decision this module makes is a pure function of declared candidate facts
//! and the dispositions the injected seam returns, so these tests build candidates by
//! hand and drive a canned adapter. Nothing here opens an EPUB, a ZIP, or a file:
//! Image write pipeline behaviour, output naming, and temporary-directory management
//! are not under test and are covered where they live.
//!
//! One assertion is knowingly weaker than the on-disk test it replaces. That a cover
//! emission failure aborts the document is proven here only as far as the module
//! propagating the error out of the attempt operation; that a *real* emission failure
//! produces an error belongs to the Image write pipeline's own tests.

use super::*;
use crate::image_format::ImageFormat;
use crate::image_write_pipeline::RequiredCoverWriteOutcome::{Completed, Retry};
use crate::image_write_pipeline::{
    ImageWriteCounts, ImageWriteFailure, ImageWriteWarning, NormalImageOutput,
};
use anyhow::anyhow;
use std::collections::VecDeque;

/// Stands in for the branded Archive resource identity, which needs a real archive.
///
/// ADR-0005 keeps the module generic over the identity precisely so these tests can
/// use a plain integer: the production identity is constructible only by opening a file.
type TestIdentity = u32;

/// One attempt the module made through the injected seam, in the order it made it.
#[derive(Debug, PartialEq, Eq)]
enum RecordedAttempt {
    /// A cover candidate attempt, named by the candidate's position.
    Cover(usize),
    /// The normal-image fallback, carrying the excluded identities in sorted order.
    Fallback(Vec<TestIdentity>),
}

/// Test adapter returning prepared dispositions and recording the attempt sequence.
///
/// Modelled on the recording run observer in the crate's shared test support: the
/// double's value is the ordered sequence of facts it saw. An attempt with no prepared
/// disposition panics, which is how tests assert that later work never ran.
struct CannedAttempts {
    cover_dispositions: VecDeque<ImageWriteOutcome<RequiredCoverWriteOutcome>>,
    fallback_disposition: Option<ImageWriteOutcome>,
    recorded: Vec<RecordedAttempt>,
}

impl CannedAttempts {
    /// Prepares the cover dispositions returned in attempt order, with no fallback.
    fn covers(dispositions: Vec<ImageWriteOutcome<RequiredCoverWriteOutcome>>) -> Self {
        Self {
            cover_dispositions: dispositions.into(),
            fallback_disposition: None,
            recorded: Vec::new(),
        }
    }

    /// Prepares the disposition the normal-image fallback returns.
    fn with_fallback(mut self, disposition: ImageWriteOutcome) -> Self {
        self.fallback_disposition = Some(disposition);
        self
    }
}

impl CoverAttempts<TestIdentity> for CannedAttempts {
    /// Records the attempted candidate's position and returns the next disposition.
    fn attempt(
        &mut self,
        candidate: &CoverCandidate<TestIdentity>,
    ) -> ImageWriteOutcome<RequiredCoverWriteOutcome> {
        self.recorded
            .push(RecordedAttempt::Cover(candidate.position()));
        self.cover_dispositions
            .pop_front()
            .expect("an unprepared cover attempt means the module attempted too many candidates")
    }

    /// Records the fallback and the identities it was told to exclude.
    fn fallback(&mut self, excluded_identities: &HashSet<TestIdentity>) -> ImageWriteOutcome {
        // Set iteration order is not part of what the module decides, so the recorded
        // exclusions are sorted to keep the assertion on their content, not their order.
        let mut excluded = excluded_identities.iter().copied().collect::<Vec<_>>();
        excluded.sort_unstable();
        self.recorded.push(RecordedAttempt::Fallback(excluded));
        self.fallback_disposition
            .take()
            .expect("an unprepared fallback means the module fell back when it should not have")
    }
}

/// Builds one cover candidate with a plain integer identity.
fn candidate(
    position: usize,
    declared_id: &str,
    manifest_path: &str,
    identity: TestIdentity,
) -> CoverCandidate<TestIdentity> {
    CoverCandidate::new(position, declared_id, manifest_path, identity)
}

/// Image write facts carrying only warnings, as a non-emitting attempt produces.
fn warned(warnings: Vec<ImageWriteWarning>) -> ImageWriteResult {
    ImageWriteResult::new(
        ImageWriteCounts::default(),
        warnings,
        NormalImageOutput::Absent,
    )
}

/// Image write facts for one emitted required cover, which is not normal image output.
fn emitted_cover() -> ImageWriteResult {
    ImageWriteResult::new(
        ImageWriteCounts {
            extracted: 1,
            ..ImageWriteCounts::default()
        },
        Vec::new(),
        NormalImageOutput::Absent,
    )
}

/// Image write facts for a normal-images invocation that emitted at least one file.
fn normal_output(counts: ImageWriteCounts, warnings: Vec<ImageWriteWarning>) -> ImageWriteResult {
    ImageWriteResult::new(counts, warnings, NormalImageOutput::Present)
}

/// One non-fatal acquisition warning fact for the named archive source.
fn acquisition_failed(source_name: &str) -> ImageWriteWarning {
    ImageWriteWarning::archive_image_acquisition_failed(
        source_name,
        format!("EPUB resource not found: {source_name}"),
    )
}

#[test]
fn the_declared_cover_is_attempted_before_the_filename_candidate() {
    // The filename candidate deliberately sits earlier in the slice, so an attempt
    // order that merely followed candidate order would attempt it first.
    let candidates = [
        candidate(0, "filename-cover", "images/cover.jpg", 1),
        candidate(1, "metadata-cover", "images/art.png", 2),
    ];
    let mut attempts = CannedAttempts::covers(vec![
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/art.png",
        )]))),
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/cover.jpg",
        )]))),
    ]);

    let result = extract_required_cover(&candidates, Some("metadata-cover"), false, &mut attempts)
        .expect("retried cover candidates should complete without fallback");

    assert_eq!(
        attempts.recorded,
        vec![RecordedAttempt::Cover(1), RecordedAttempt::Cover(0)]
    );
    assert_eq!(
        result.warnings,
        vec![
            acquisition_failed("OEBPS/images/art.png"),
            acquisition_failed("OEBPS/images/cover.jpg"),
        ]
    );
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
        let manifest_path = format!("images/CoVeR.{extension}");
        let candidates = [candidate(0, "filename-candidate", &manifest_path, 1)];
        let mut attempts = CannedAttempts::covers(vec![Ok(Completed(emitted_cover()))]);

        extract_required_cover(&candidates, None, false, &mut attempts)
            .expect("a filename-cover decision should complete normally");

        let expected = if should_match {
            vec![RecordedAttempt::Cover(0)]
        } else {
            Vec::new()
        };
        assert_eq!(
            attempts.recorded, expected,
            "unexpected filename-cover decision for .{extension}"
        );
    }

    // Two JPEG-family covers: the earlier candidate wins, so the decision is fixed by
    // the caller's deterministic ordering rather than by which one is found first.
    let candidates = [
        candidate(0, "first", "a/cover.jpeg", 1),
        candidate(1, "later", "z/cover.jpg", 2),
    ];
    let mut attempts = CannedAttempts::covers(vec![Ok(Completed(emitted_cover()))]);

    extract_required_cover(&candidates, None, false, &mut attempts)
        .expect("the first deterministic filename candidate should complete the decision");

    assert_eq!(attempts.recorded, vec![RecordedAttempt::Cover(0)]);
}

#[test]
fn a_completed_cover_is_terminal_before_the_filename_candidate_and_fallback() {
    // A filtered cover, an unsupported cover conversion and a failed cover conversion
    // are three Image write policy decisions that reach this module as one disposition.
    // The module must treat each as final, whatever fact the attempt carried back.
    for warning in [
        ImageWriteWarning::UnsupportedCoverFormat {
            format: ImageFormat::Png,
        },
        ImageWriteWarning::CoverConversionSkipped {
            format: ImageFormat::Svg,
        },
        ImageWriteWarning::CoverConversionFailed {
            detail: "Failed to decode image".to_string(),
        },
    ] {
        let candidates = [
            candidate(0, "metadata-cover", "images/art.png", 1),
            candidate(1, "filename-cover", "images/cover.jpg", 2),
            candidate(2, "page", "images/page.jpg", 3),
        ];
        // No fallback disposition is prepared, so falling back would panic.
        let mut attempts =
            CannedAttempts::covers(vec![Ok(Completed(warned(vec![warning.clone()])))]);

        let result =
            extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
                .expect("a completed cover attempt should end the decision normally");

        assert_eq!(attempts.recorded, vec![RecordedAttempt::Cover(0)]);
        assert_eq!(result.warnings, vec![warning]);
        assert_eq!(result.counts.extracted, 0);
        assert_eq!(result.counts.skipped, 0);
        assert!(!result.has_normal_image_output());
    }
}

#[test]
fn an_unreadable_declared_cover_retries_the_filename_candidate() {
    let candidates = [
        candidate(0, "metadata-cover", "images/missing.png", 1),
        candidate(1, "filename-cover", "images/cover.jpg", 2),
    ];
    let mut attempts = CannedAttempts::covers(vec![
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/missing.png",
        )]))),
        Ok(Completed(emitted_cover())),
    ]);

    let result = extract_required_cover(&candidates, Some("metadata-cover"), false, &mut attempts)
        .expect("an unreadable declared cover should allow the filename candidate");

    assert_eq!(
        attempts.recorded,
        vec![RecordedAttempt::Cover(0), RecordedAttempt::Cover(1)]
    );
    assert_eq!(result.counts.extracted, 1);
    assert!(!result.has_normal_image_output());
    assert_eq!(
        result.warnings,
        vec![acquisition_failed("OEBPS/images/missing.png")]
    );
}

#[test]
fn cover_retry_warnings_precede_the_normal_fallback_warning() {
    let candidates = [
        candidate(0, "metadata-cover", "images/missing.png", 1),
        candidate(1, "filename-cover", "images/cover.jpg", 2),
        candidate(2, "page", "images/page.png", 3),
    ];
    let fallback_warning = ImageWriteWarning::ExtensionFallback {
        source_name: "OEBPS/images/page.png".to_string(),
        format: ImageFormat::Png,
    };
    let mut attempts = CannedAttempts::covers(vec![
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/missing.png",
        )]))),
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/cover.jpg",
        )]))),
    ])
    .with_fallback(Ok(normal_output(
        ImageWriteCounts {
            extracted: 1,
            ..ImageWriteCounts::default()
        },
        vec![fallback_warning.clone()],
    )));

    let result = extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
        .expect("unreadable cover candidates should allow normal fallback");

    assert_eq!(
        attempts.recorded,
        vec![
            RecordedAttempt::Cover(0),
            RecordedAttempt::Cover(1),
            RecordedAttempt::Fallback(vec![1, 2]),
        ]
    );
    assert_eq!(
        result.warnings,
        vec![
            acquisition_failed("OEBPS/images/missing.png"),
            acquisition_failed("OEBPS/images/cover.jpg"),
            fallback_warning,
        ]
    );
    assert_eq!(result.counts.extracted, 1);
    assert!(result.has_normal_image_output());
}

#[test]
fn cover_retries_precede_partial_normal_fallback_facts() {
    let candidates = [
        candidate(0, "metadata-cover", "images/missing.png", 1),
        candidate(1, "filename-cover", "images/cover.jpg", 2),
        candidate(2, "page", "images/first.png", 3),
    ];
    let mut attempts = CannedAttempts::covers(vec![
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/missing.png",
        )]))),
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/cover.jpg",
        )]))),
    ])
    .with_fallback(Err(ImageWriteFailure {
        partial: normal_output(
            ImageWriteCounts {
                extracted: 1,
                converted: 1,
                ..ImageWriteCounts::default()
            },
            Vec::new(),
        ),
        error: anyhow!("Failed to create output directory"),
    }));

    let failure = extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
        .expect_err("a fallback emission failure should retain earlier normal output");

    assert_eq!(failure.partial.counts.extracted, 1);
    assert_eq!(failure.partial.counts.converted, 1);
    assert_eq!(failure.partial.counts.gifs_routed, 0);
    assert_eq!(failure.partial.counts.skipped, 0);
    assert!(failure.partial.has_normal_image_output());
    assert_eq!(
        failure.partial.warnings,
        vec![
            acquisition_failed("OEBPS/images/missing.png"),
            acquisition_failed("OEBPS/images/cover.jpg"),
        ]
    );
    assert_eq!(
        attempts.recorded,
        vec![
            RecordedAttempt::Cover(0),
            RecordedAttempt::Cover(1),
            RecordedAttempt::Fallback(vec![1, 2]),
        ]
    );
}

#[test]
fn a_routed_gif_fact_precedes_a_later_normal_fallback_failure() {
    let candidates = [
        candidate(0, "metadata-cover", "images/missing.png", 1),
        candidate(1, "filename-cover", "images/cover.jpg", 2),
        candidate(2, "animation", "images/a.gif", 3),
        candidate(3, "page", "images/b.png", 4),
    ];
    let mut attempts = CannedAttempts::covers(vec![
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/missing.png",
        )]))),
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/cover.jpg",
        )]))),
    ])
    .with_fallback(Err(ImageWriteFailure {
        partial: normal_output(
            ImageWriteCounts {
                extracted: 1,
                gifs_routed: 1,
                ..ImageWriteCounts::default()
            },
            Vec::new(),
        ),
        error: anyhow!("Failed to create output directory"),
    }));

    let failure = extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
        .expect_err("a later normal emission failure should retain the routed GIF");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.counts.extracted, 1);
    assert_eq!(failure.partial.counts.gifs_routed, 1);
    assert_eq!(failure.partial.counts.converted, 0);
    assert!(failure.partial.has_normal_image_output());
    assert_eq!(
        failure.partial.warnings,
        vec![
            acquisition_failed("OEBPS/images/missing.png"),
            acquisition_failed("OEBPS/images/cover.jpg"),
        ]
    );
    // The routed GIF is a fallback fact, so "precedes" is a claim about when fallback
    // ran relative to the cover attempts, not only about which counts survived.
    assert_eq!(
        attempts.recorded,
        vec![
            RecordedAttempt::Cover(0),
            RecordedAttempt::Cover(1),
            RecordedAttempt::Fallback(vec![1, 2]),
        ]
    );
}

#[test]
fn an_already_attempted_identity_is_not_retried() {
    // Three manifest spellings share one payload identity. That aliasing spellings
    // really do resolve to a single Archive resource identity is grounded by the
    // EPUB adapter's aliasing test; what this asserts is the policy over it.
    let candidates = [
        candidate(0, "metadata-cover", "images/cover%2Ejpg", 7),
        candidate(1, "filename-cover", "images/cover.jpg", 7),
        candidate(2, "non-cover-alias", "images/cover%2ejpg", 7),
        candidate(3, "page", "images/page.png", 9),
    ];
    let mut attempts = CannedAttempts::covers(vec![Ok(Retry(warned(vec![acquisition_failed(
        "OEBPS/images/cover%2Ejpg",
    )])))])
    .with_fallback(Ok(normal_output(
        ImageWriteCounts {
            extracted: 1,
            ..ImageWriteCounts::default()
        },
        Vec::new(),
    )));

    let result = extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
        .expect("one failed resolved cover should allow normal fallback");

    assert_eq!(
        attempts.recorded,
        vec![
            RecordedAttempt::Cover(0),
            RecordedAttempt::Fallback(vec![7]),
        ]
    );
    assert_eq!(
        result.warnings,
        vec![acquisition_failed("OEBPS/images/cover%2Ejpg")]
    );
    assert_eq!(result.counts.extracted, 1);
    assert!(result.has_normal_image_output());
}

#[test]
fn a_fatal_cover_emission_stops_the_filename_candidate_and_normal_fallback() {
    let candidates = [
        candidate(0, "metadata-cover", "images/art.gif", 1),
        candidate(1, "filename-cover", "images/cover.jpg", 2),
        candidate(2, "page", "images/page.png", 3),
    ];
    // Neither a second cover disposition nor a fallback is prepared, so reaching
    // either would panic rather than quietly returning a passing outcome.
    let mut attempts = CannedAttempts::covers(vec![Err(ImageWriteFailure::empty(anyhow!(
        "Failed to create output directory"
    )))]);

    let failure = extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
        .expect_err("a fatal cover emission should abort the decision");

    assert_eq!(attempts.recorded, vec![RecordedAttempt::Cover(0)]);
    assert_eq!(failure.partial.counts.extracted, 0);
    assert_eq!(failure.partial.counts.gifs_routed, 0);
    assert!(!failure.partial.has_normal_image_output());
    assert!(failure.partial.warnings.is_empty());
}

#[test]
fn a_cover_emission_error_propagates_with_its_retained_facts() {
    let candidates = [
        candidate(0, "metadata-cover", "images/missing.png", 1),
        candidate(1, "filename-cover", "images/cover.jpg", 2),
        candidate(2, "page", "images/page.png", 3),
    ];
    let default_to_jpeg = ImageWriteWarning::CoverDefaultToJpeg {
        mime: "application/octet-stream".to_string(),
    };
    let mut attempts = CannedAttempts::covers(vec![
        Ok(Retry(warned(vec![acquisition_failed(
            "OEBPS/images/missing.png",
        )]))),
        Err(ImageWriteFailure {
            partial: warned(vec![default_to_jpeg.clone()]),
            error: anyhow!("Failed to create output directory"),
        }),
    ]);

    let failure = extract_required_cover(&candidates, Some("metadata-cover"), true, &mut attempts)
        .expect_err("an emission failure must abort cover extraction");

    assert!(format!("{:#}", failure.error).contains("Failed to create output directory"));
    assert_eq!(failure.partial.counts.extracted, 0);
    assert_eq!(
        failure.partial.warnings,
        vec![
            acquisition_failed("OEBPS/images/missing.png"),
            default_to_jpeg,
        ]
    );
    assert_eq!(
        attempts.recorded,
        vec![RecordedAttempt::Cover(0), RecordedAttempt::Cover(1)]
    );
}
