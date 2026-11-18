//! Tests for the Image size limit's value, spelling and wording.
//!
//! Byte counts are written as literals rather than derived from the module's unit
//! constants, so each test pins the documented binary meaning of a suffix.

use super::*;

/// Parses `text`, panicking with the spelling if it is rejected.
fn parsed(text: &str) -> ImageSizeLimit {
    text.parse()
        .unwrap_or_else(|error| panic!("'{text}' should parse: {error}"))
}

#[test]
fn default_is_256_mebibytes() {
    assert_eq!(ImageSizeLimit::DEFAULT.bytes(), 268_435_456);
    assert_eq!(ImageSizeLimit::default(), ImageSizeLimit::DEFAULT);
}

#[test]
fn default_spelling_parses_to_the_default() {
    assert_eq!(
        parsed(ImageSizeLimit::DEFAULT_SPELLING),
        ImageSizeLimit::DEFAULT
    );
}

#[test]
fn bare_counts_and_byte_units_are_bytes() {
    for text in ["1500", "1500B", "1500 bytes", "1500b", " 1500 "] {
        assert_eq!(parsed(text).bytes(), 1500, "{text}");
    }
    assert_eq!(parsed("1 byte").bytes(), 1);
}

#[test]
fn suffixes_are_binary_units_in_any_case() {
    assert_eq!(parsed("4K").bytes(), 4096);
    assert_eq!(parsed("4kib").bytes(), 4096);
    assert_eq!(parsed("256M").bytes(), 268_435_456);
    assert_eq!(parsed("256 MiB").bytes(), 268_435_456);
    assert_eq!(parsed("2g").bytes(), 2_147_483_648);
    assert_eq!(parsed("2 GiB").bytes(), 2_147_483_648);
}

#[test]
fn decimal_units_and_fractions_are_rejected_rather_than_guessed() {
    assert_eq!(
        "256MB".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::UnknownUnit("MB".to_string()))
    );
    assert_eq!(
        "1.5G".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::UnknownUnit(".5G".to_string()))
    );
}

#[test]
fn missing_counts_zero_and_overflow_are_rejected() {
    assert_eq!(
        "".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::MissingNumber)
    );
    assert_eq!(
        "M".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::MissingNumber)
    );
    assert_eq!(
        "-1".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::MissingNumber)
    );
    assert_eq!(
        "0".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::Zero)
    );
    assert_eq!(
        "0G".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::Zero)
    );
    assert_eq!(
        "99999999999999999999".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::TooLarge)
    );
    // Fits in 64 bits as a count, but not once multiplied by its unit.
    assert_eq!(
        "17179869184G".parse::<ImageSizeLimit>(),
        Err(ImageSizeLimitParseError::TooLarge)
    );
}

#[test]
fn display_uses_the_largest_exact_unit() {
    assert_eq!(ImageSizeLimit::DEFAULT.to_string(), "256 MiB");
    assert_eq!(parsed("1G").to_string(), "1 GiB");
    assert_eq!(parsed("1536K").to_string(), "1536 KiB");
    assert_eq!(parsed("1500").to_string(), "1500 bytes");
    assert_eq!(parsed("1").to_string(), "1 byte");
}

/// Pins that a limit quoted from a warning can be passed back to `--max-image-size`.
#[test]
fn every_displayed_limit_parses_back_to_itself() {
    for text in ["1", "1500", "4K", "1536K", "256M", "3G", "1025M"] {
        let limit = parsed(text);
        assert_eq!(parsed(&limit.to_string()), limit, "{text}");
    }
}

#[test]
fn the_limit_is_inclusive() {
    let limit = parsed("1K");
    assert!(limit.admits(1023));
    assert!(limit.admits(1024));
    assert!(!limit.admits(1025));
}
