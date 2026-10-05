//! Tests for the Extraction run observation vocabulary and its outcome types.
//!
//! Outcome classification is tested at the outcome accumulator, where it lives:
//! tallies and failures go in and an Extraction run outcome comes out, with no
//! Document extraction, search surface or EPUB declarations involved (ADR-0017).
//! What the run hands the fold stays tested by the run's own tests.

use super::*;
use crate::emitted_image_tally::TallyRole;

#[test]
fn produced_outcome_rejects_inconsistent_semantic_totals() {
    let one = NonZeroUsize::new(1).expect("one should be nonzero");
    let two = NonZeroUsize::new(2).expect("two should be nonzero");

    assert!(
        ExtractionRunOutcome::try_produced(
            ExtractionOutputKind::Images,
            one,
            two,
            None,
            None,
            None
        )
        .is_none()
    );
    assert!(
        ExtractionRunOutcome::try_produced(
            ExtractionOutputKind::Images,
            one,
            one,
            Some(ConversionFacts::new(1, 0)),
            Some(GifRoutingFacts::new(one, PathBuf::from("gifs"))),
            None,
        )
        .is_none()
    );
}

/// Returns the tally of a document that wrote `count` normal images as extracted.
fn normal_images(count: usize) -> EmittedImageTally {
    normal_images_in(&vec![TallyRole::Preserved; count])
}

/// Returns the tally of a document that wrote one normal image per role, in order.
fn normal_images_in(roles: &[TallyRole]) -> EmittedImageTally {
    let mut tally = EmittedImageTally::default();
    for &role in roles {
        tally.record_normal_image(role);
    }
    tally
}

/// Returns the tally of an EPUB that wrote its one required cover as extracted.
fn one_cover() -> EmittedImageTally {
    let mut tally = EmittedImageTally::default();
    tally.record_cover(TallyRole::Preserved);
    tally
}

/// Returns the seed of a run with neither conversion nor GIF routing configured.
fn no_optional_facts() -> ApplicableOutcomeFacts {
    ApplicableOutcomeFacts::fabricated(false, None)
}

/// One document as the accumulator receives it: its tally, and whether it failed.
struct FoldedDocument {
    tally: EmittedImageTally,
    failed: bool,
}

/// A completed document that emitted what `tally` records.
fn completed(tally: EmittedImageTally) -> FoldedDocument {
    FoldedDocument {
        tally,
        failed: false,
    }
}

/// A failed document whose partial output is what `tally` records.
fn failed(tally: EmittedImageTally) -> FoldedDocument {
    FoldedDocument {
        tally,
        failed: true,
    }
}

/// Seeds an accumulator, folds each document in order, and finishes with the cover intent.
///
/// A failed document's facts fold exactly like a completed one's, and its failure
/// is recorded beside them, which is the order the run itself uses.
fn accumulate(
    applicable: ApplicableOutcomeFacts,
    documents: Vec<FoldedDocument>,
    cover_only: bool,
) -> ExtractionRunOutcome {
    let mut accumulator = ExtractionRunOutcomeAccumulator::new(applicable);
    for document in documents {
        accumulator.fold(&DocumentExtractionFacts::fabricated(
            document.tally,
            Vec::new(),
        ));
        if document.failed {
            accumulator.record_failed_document();
        }
    }
    accumulator.finish(cover_only)
}

/// Borrows produced-output facts from one outcome, panicking with `case` otherwise.
fn produced<'outcome>(
    outcome: &'outcome ExtractionRunOutcome,
    case: &str,
) -> &'outcome ProducedOutput {
    match outcome {
        ExtractionRunOutcome::ProducedOutput(output) => output,
        other => panic!("{case}: expected produced output, got {other:?}"),
    }
}

/// Verifies every cover intent and failure combination over a document that emitted nothing.
///
/// With nothing emitted, only the run's cover intent decides the kind sought,
/// and a failure is the one fact that tells "nothing found" from "nothing could
/// be read".
#[test]
fn document_without_output_classifies_as_no_output_for_every_intent_and_failure() {
    let cases = [
        (
            "document without images: image no-output",
            false,
            completed(EmittedImageTally::default()),
            ExtractionOutputKind::Images,
            None,
        ),
        (
            "failed document without output: counted in image no-output",
            false,
            failed(EmittedImageTally::default()),
            ExtractionOutputKind::Images,
            NonZeroUsize::new(1),
        ),
        (
            "cover-only EPUB without a cover: cover no-output",
            true,
            completed(EmittedImageTally::default()),
            ExtractionOutputKind::Covers,
            None,
        ),
        (
            "failed EPUB in a cover-only run: counted in cover no-output",
            true,
            failed(EmittedImageTally::default()),
            ExtractionOutputKind::Covers,
            NonZeroUsize::new(1),
        ),
    ];

    for (case, cover_only, document, output_kind, failed_documents) in cases {
        assert_eq!(
            accumulate(no_optional_facts(), vec![document], cover_only),
            ExtractionRunOutcome::NoOutput {
                output_kind,
                failed_documents,
            },
            "{case}"
        );
    }
}

/// Verifies one emitted normal image classifies as produced images with no optional facts.
#[test]
fn normal_document_output_produces_images() {
    let outcome = accumulate(
        no_optional_facts(),
        vec![completed(normal_images(1))],
        false,
    );
    let output = produced(&outcome, "one normal image");

    assert_eq!(output.output_kind(), ExtractionOutputKind::Images);
    assert_eq!(output.emitted_images(), 1);
    assert_eq!(output.documents_with_output(), 1);
    assert!(output.conversion().is_none());
    assert!(output.gif_routing().is_none());
    assert_eq!(outcome.failed_documents(), None);
}

/// Verifies a routed GIF keeps its count and the seeded destination in the outcome.
#[test]
fn routed_gif_retains_its_count_and_destination() {
    let gif_destination = PathBuf::from("gifs");

    let outcome = accumulate(
        ApplicableOutcomeFacts::fabricated(false, Some(gif_destination.clone())),
        vec![completed(normal_images_in(&[TallyRole::RoutedGif]))],
        false,
    );
    let output = produced(&outcome, "one routed GIF");
    let gif_routing = output
        .gif_routing()
        .expect("GIF routing facts should apply");

    assert!(output.conversion().is_none());
    assert_eq!(gif_routing.routed_gifs(), 1);
    assert_eq!(gif_routing.destination(), gif_destination);
}

/// Verifies GIF routing that applies but routed nothing leaves no routing facts.
///
/// Routing facts need a positive routed count, so a configured destination alone
/// is not enough for the outcome to carry them.
#[test]
fn applicable_gif_routing_with_nothing_routed_carries_no_routing_facts() {
    let outcome = accumulate(
        ApplicableOutcomeFacts::fabricated(false, Some(PathBuf::from("gifs"))),
        vec![completed(normal_images(2))],
        false,
    );
    let output = produced(&outcome, "two preserved images with routing configured");

    assert_eq!(output.emitted_images(), 2);
    assert!(output.gif_routing().is_none());
}

/// Verifies conversion and GIF-routing totals are summed across documents and survive together.
///
/// In the multi-document case the middle document emits nothing, so it adds to
/// no total and is not a document with output. The single-document case is the
/// same fold over one tally.
#[test]
fn produced_outcome_sums_conversion_and_gif_routing_totals_across_documents() {
    let gif_destination = PathBuf::from("gifs");
    // Each document records its own normal images, so its converted, skipped and
    // routed totals stay within its own emitted total.
    let cases = [
        (
            "one document: converted, conversion-skipped and routed facts together",
            vec![completed(normal_images_in(&[
                TallyRole::Converted,
                TallyRole::ConversionSkipped,
                TallyRole::RoutedGif,
            ]))],
            3,
            1,
            ConversionFacts::new(1, 1),
            1,
        ),
        (
            "three documents, the middle one empty: totals summed",
            vec![
                completed(normal_images_in(&[
                    TallyRole::RoutedGif,
                    TallyRole::Converted,
                ])),
                completed(EmittedImageTally::default()),
                completed(normal_images_in(&[
                    TallyRole::RoutedGif,
                    TallyRole::Converted,
                    TallyRole::ConversionSkipped,
                ])),
            ],
            5,
            2,
            ConversionFacts::new(2, 1),
            2,
        ),
    ];

    for (case, documents, emitted_images, documents_with_output, conversion, routed_gifs) in cases {
        let outcome = accumulate(
            ApplicableOutcomeFacts::fabricated(true, Some(gif_destination.clone())),
            documents,
            false,
        );
        let output = produced(&outcome, case);
        let gif_routing = output
            .gif_routing()
            .unwrap_or_else(|| panic!("{case}: routed GIF facts should be present"));

        assert_eq!(output.output_kind(), ExtractionOutputKind::Images, "{case}");
        assert_eq!(output.emitted_images(), emitted_images, "{case}");
        assert_eq!(
            output.documents_with_output(),
            documents_with_output,
            "{case}"
        );
        assert_eq!(output.conversion(), Some(&conversion), "{case}");
        assert_eq!(gif_routing.routed_gifs(), routed_gifs, "{case}");
        assert_eq!(gif_routing.destination(), gif_destination, "{case}");
        assert_eq!(outcome.failed_documents(), None, "{case}");
    }
}

/// Verifies a cover-only run that wrote one cover and nothing else produces a Covers outcome.
///
/// The whole outcome is pinned: a Covers outcome carries no conversion, GIF
/// routing or failure facts here, and the EPUB that emitted nothing is not a
/// document with output. Which of the two documents comes last does not change
/// the kind, because a cover-only run is Covers unless normal images were
/// included, and neither document included any.
#[test]
fn cover_only_run_with_a_written_cover_and_an_empty_epub_produces_covers() {
    let outcome = accumulate(
        no_optional_facts(),
        vec![
            completed(one_cover()),
            completed(EmittedImageTally::default()),
        ],
        true,
    );
    let output = produced(&outcome, "one cover and an empty EPUB");

    assert_eq!(output.output_kind(), ExtractionOutputKind::Covers);
    assert_eq!(output.emitted_images(), 1);
    assert_eq!(output.documents_with_output(), 1);
    assert!(output.conversion().is_none());
    assert!(output.gif_routing().is_none());
    assert_eq!(outcome.failed_documents(), None);
}

/// Verifies any normal image makes a cover run's output Images, alone or merged with covers.
///
/// `--cover-only --cover-fallback` binds `CoverThenNormalImages`, so an EPUB
/// without a cover falls back to its interior images, alone or beside another
/// EPUB that wrote its cover. Tallies combine by addition, so one merge order
/// proves the rule; the reversed order stays only to document that fold order
/// does not matter.
#[test]
fn cover_run_with_fallback_images_classifies_as_images() {
    let cases = [
        (
            "EPUB normal fallback alone",
            vec![completed(normal_images(1))],
            1,
            1,
        ),
        (
            "covers then fallback",
            vec![completed(one_cover()), completed(normal_images(1))],
            2,
            2,
        ),
        (
            "fallback then covers",
            vec![completed(normal_images(1)), completed(one_cover())],
            2,
            2,
        ),
    ];

    for (case, documents, emitted_images, documents_with_output) in cases {
        let outcome = accumulate(no_optional_facts(), documents, true);
        let output = produced(&outcome, case);

        assert_eq!(output.output_kind(), ExtractionOutputKind::Images, "{case}");
        assert_eq!(output.emitted_images(), emitted_images, "{case}");
        assert_eq!(
            output.documents_with_output(),
            documents_with_output,
            "{case}"
        );
        assert!(output.conversion().is_none(), "{case}");
        assert!(output.gif_routing().is_none(), "{case}");
        assert_eq!(outcome.failed_documents(), None, "{case}");
    }
}
