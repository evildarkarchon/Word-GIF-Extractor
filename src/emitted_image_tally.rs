//! Emitted image tally: the one record of what an Image write pipeline emitted.
//!
//! A leaf module that imports nothing from the crate. The Image write pipeline,
//! Document extraction and the Extraction run observation module all import it, so
//! every dependency edge points into it and none can close a cycle (ADR-0017).

use std::ops::AddAssign;

/// The destination-free role one emitted image is recorded under.
///
/// This mirrors the Image write pipeline's Emitted image role without the routed
/// GIF destination: the pipeline keeps that destination bound to its routing
/// decision (ADR-0007), and the tally needs only to know that the image was routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TallyRole {
    /// A GIF the Image write policy sent to its own destination, unconverted.
    RoutedGif,
    /// An image the Conversion policy re-encoded to its target.
    Converted,
    /// An image conversion could not take, emitted in its original bytes.
    ConversionSkipped,
    /// An image emitted as extracted, counted only toward the emitted total.
    Preserved,
}

/// Every emitted image, entered exactly once under its Image write purpose and role.
///
/// The empty tally ([`Default`]) is the starting value, and recording an image or
/// adding another tally are the only ways to change one. Because every image adds
/// one to exactly one purpose total and at most one role total, the converted,
/// conversion-skipped and GIF-routed totals can never together exceed the emitted
/// total, at any level tallies are combined.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct EmittedImageTally {
    normal_images: usize,
    covers: usize,
    converted: usize,
    conversion_skipped: usize,
    gifs_routed: usize,
}

impl EmittedImageTally {
    /// Records one emitted normal image under `role`.
    pub(crate) fn record_normal_image(&mut self, role: TallyRole) {
        self.normal_images += 1;
        self.record_role(role);
    }

    /// Records one emitted required cover under `role`.
    ///
    /// Any role is accepted. The pipeline never emits a conversion-skipped cover,
    /// because a cover conversion fallback completes without emitting, but the
    /// tally records what it is given rather than policing that rule.
    pub(crate) fn record_cover(&mut self, role: TallyRole) {
        self.covers += 1;
        self.record_role(role);
    }

    /// Adds one image's role to the matching role total, if the role is counted.
    fn record_role(&mut self, role: TallyRole) {
        match role {
            TallyRole::RoutedGif => self.gifs_routed += 1,
            TallyRole::Converted => self.converted += 1,
            TallyRole::ConversionSkipped => self.conversion_skipped += 1,
            TallyRole::Preserved => {}
        }
    }

    /// Returns the number of normal images recorded.
    pub(crate) fn normal_images(self) -> usize {
        self.normal_images
    }

    /// Returns the number of required covers recorded.
    pub(crate) fn covers(self) -> usize {
        self.covers
    }

    /// Returns every image recorded, normal images plus covers.
    ///
    /// Derived rather than stored, so it cannot disagree with the two purpose totals.
    pub(crate) fn emitted(self) -> usize {
        self.normal_images() + self.covers()
    }

    /// Returns the number of images recorded as converted.
    pub(crate) fn converted(self) -> usize {
        self.converted
    }

    /// Returns the number of images recorded as conversion-skipped.
    pub(crate) fn conversion_skipped(self) -> usize {
        self.conversion_skipped
    }

    /// Returns the number of images recorded as routed GIFs, covers included.
    pub(crate) fn gifs_routed(self) -> usize {
        self.gifs_routed
    }
}

impl AddAssign for EmittedImageTally {
    /// Adds every total of `later` into this tally.
    fn add_assign(&mut self, later: Self) {
        self.normal_images += later.normal_images;
        self.covers += later.covers;
        self.converted += later.converted;
        self.conversion_skipped += later.conversion_skipped;
        self.gifs_routed += later.gifs_routed;
    }
}
