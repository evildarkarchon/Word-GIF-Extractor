//! Tests for the Extraction run observation vocabulary and its outcome types.

use super::*;

#[test]
fn produced_outcome_rejects_inconsistent_semantic_totals() {
    let one = NonZeroUsize::new(1).expect("one should be nonzero");
    let two = NonZeroUsize::new(2).expect("two should be nonzero");

    assert!(
        ExtractionRunOutcome::try_produced(ExtractionOutputKind::Images, one, two, None, None,)
            .is_none()
    );
    assert!(
        ExtractionRunOutcome::try_produced(
            ExtractionOutputKind::Images,
            one,
            one,
            Some(ConversionFacts::new(1, 0)),
            Some(GifRoutingFacts::new(one, PathBuf::from("gifs"))),
        )
        .is_none()
    );
}
