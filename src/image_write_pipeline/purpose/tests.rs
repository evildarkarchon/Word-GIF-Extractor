//! Tests for the archive path safety rule behind Image write purpose.
//!
//! The scope here is deliberately narrow: the path safety rule, and the source
//! eligibility that is built from it. `is_safe_archive_path` is the one decision
//! in this module that can be wrong — it has several rejection conditions and it
//! guards against traversal and alternate-data-stream syntax, so a branch lost
//! there is a real defect rather than a style regression — and
//! `source_eligibility` is the trait method through which discovery observes it,
//! so the two are asserted together. Required-cover eligibility is asserted only
//! as that rule's counterpart, because a required cover's path is diagnostic
//! identity rather than evidence.
//!
//! Every other method on the Image write purpose trait returns an unconditional
//! literal, so asserting one would mirror the source line above it. Those hooks
//! are exercised where they are consumed instead — the unidentified-format and
//! filtered-format hooks by Archive image discovery's tests, and the two
//! conversion hooks by the Image write pipeline's tests, which is the only place
//! both purposes' conversion paths run and the contrast between them is
//! observable.
//!
//! Nothing here needs a comparison derive: the safety rule returns a boolean and
//! source eligibility is pattern-matched.

use super::*;

/// One unsafe archive path naming the rejection condition it exercises.
struct UnsafePathCase {
    condition: &'static str,
    name: &'static str,
}

/// Returns one representative unsafe archive path per rejection condition.
///
/// Each fixture trips exactly one condition, so a lost branch fails the case
/// named for it rather than being masked by another condition in the same path.
fn unsafe_path_cases() -> Vec<UnsafePathCase> {
    vec![
        UnsafePathCase {
            condition: "parent-directory traversal",
            name: "word/media/../../secret.png",
        },
        UnsafePathCase {
            condition: "leading forward slash",
            name: "/word/media/image1.png",
        },
        UnsafePathCase {
            condition: "leading backslash",
            name: "\\word\\media\\image1.png",
        },
        UnsafePathCase {
            condition: "alternate data stream syntax",
            name: "word/media/image1.png:hidden",
        },
        UnsafePathCase {
            condition: "embedded NUL byte",
            name: "word/media/image\u{0}1.png",
        },
    ]
}

#[test]
fn every_rejection_condition_makes_an_archive_path_unsafe() {
    // Collected rather than asserted in place: a guard with this many branches
    // can lose more than one at a time, and a loop that stops at the first
    // failure would report only the first.
    let wrongly_accepted: Vec<&str> = unsafe_path_cases()
        .into_iter()
        .filter(|case| is_safe_archive_path(case.name))
        .map(|case| case.condition)
        .collect();

    assert!(
        wrongly_accepted.is_empty(),
        "is_safe_archive_path accepted paths carrying: {wrongly_accepted:?}"
    );
}

#[test]
fn representative_archive_resource_paths_are_safe() {
    // Ordinary office and EPUB media paths, so that hardening the rule cannot
    // silently start rejecting the resources discovery exists to accept.
    assert!(is_safe_archive_path("word/media/image1.png"));
    assert!(is_safe_archive_path("OEBPS/images/cover.jpg"));
    // EPUB manifest paths stay percent-encoded until the ZIP lookup decodes
    // them, so an encoded name reaches this rule as an ordinary resource.
    assert!(is_safe_archive_path("OEBPS/images/cover%20art.jpg"));
}

#[test]
fn normal_source_safety_follows_the_presence_of_path_evidence() {
    // Both sources carry the same name, so the only thing separating them is
    // whether that name is path evidence or diagnostic identity. A required
    // cover deliberately omits the evidence, and can therefore never be safe
    // for normal traversal.
    let cover = ArchiveImageSource::required_cover("OEBPS/images/cover.jpg", "image/jpeg");
    assert!(!is_normal_source_safe(&cover));

    let normal = ArchiveImageSource::named("OEBPS/images/cover.jpg");
    assert!(is_normal_source_safe(&normal));
}

#[test]
fn normal_image_eligibility_rejects_every_unsafe_path() {
    let wrongly_inspected: Vec<&str> = unsafe_path_cases()
        .into_iter()
        .filter(|case| {
            matches!(
                NormalImages.source_eligibility(&ArchiveImageSource::named(case.name)),
                SourceEligibility::Inspect
            )
        })
        .map(|case| case.condition)
        .collect();

    assert!(
        wrongly_inspected.is_empty(),
        "NormalImages::source_eligibility inspected sources carrying: {wrongly_inspected:?}"
    );
}

#[test]
fn normal_image_eligibility_inspects_a_safe_source() {
    let source = ArchiveImageSource::named("word/media/image1.png");
    assert!(matches!(
        NormalImages.source_eligibility(&source),
        SourceEligibility::Inspect
    ));
}

#[test]
fn required_cover_eligibility_inspects_regardless_of_path_syntax() {
    let cover = ArchiveImageSource::required_cover("OEBPS/images/cover.jpg", "image/jpeg");
    assert!(matches!(
        RequiredCover.source_eligibility(&cover),
        SourceEligibility::Inspect
    ));

    // The EPUB adapter supplies the reader, so a path that normal traversal
    // would reject withholds nothing here. One such path is enough: the
    // decision ignores its source, so repeating it per rejection condition
    // would assert the same literal five times.
    let traversal = ArchiveImageSource::required_cover("../../secret.jpg", "image/jpeg");
    assert!(matches!(
        RequiredCover.source_eligibility(&traversal),
        SourceEligibility::Inspect
    ));
}
