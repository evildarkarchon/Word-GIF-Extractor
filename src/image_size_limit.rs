//! The Image size limit: the per-run ceiling on one archive image source's payload.
//!
//! The module owns the value, its default, its command-line spelling and the
//! wording a warning uses for it. Enforcing it belongs to Archive image discovery,
//! which checks it against the bytes it actually reads (ADR-0030).

use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;

/// The most bytes one archive image source may hold before it is skipped.
///
/// Never zero: a limit of zero would skip every image, which is not a size limit
/// but a request to extract nothing, so parsing rejects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageSizeLimit {
    bytes: NonZeroU64,
}

impl ImageSizeLimit {
    /// The limit a run uses unless `--max-image-size` names another: 256 MiB.
    ///
    /// Far above any realistic embedded picture, scanned TIFFs included, and still
    /// a bound on what one malformed archive entry can make a run hold in memory.
    pub(crate) const DEFAULT: Self = Self {
        bytes: NonZeroU64::new(256 * MIB).expect("256 MiB is not zero"),
    };

    /// [`Self::DEFAULT`] as `--help` prints it. A test pins that it parses to the
    /// default, so the two cannot drift apart; it exists only because `clap` quotes
    /// a default containing a space, and `256M` reads better than `"256 MiB"`.
    pub(crate) const DEFAULT_SPELLING: &str = "256M";

    /// Creates a limit of exactly `bytes`.
    pub(crate) const fn from_bytes(bytes: NonZeroU64) -> Self {
        Self { bytes }
    }

    /// Returns the limit in bytes.
    pub(crate) const fn bytes(self) -> u64 {
        self.bytes.get()
    }

    /// Returns whether a payload of `len` bytes fits within the limit.
    ///
    /// The limit is inclusive: a payload of exactly the limit is admitted.
    pub(crate) const fn admits(self, len: u64) -> bool {
        len <= self.bytes.get()
    }
}

impl Default for ImageSizeLimit {
    /// Returns [`ImageSizeLimit::DEFAULT`].
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for ImageSizeLimit {
    /// Formats the limit in the largest binary unit that divides it exactly.
    ///
    /// `256 MiB`, `1 GiB`, `1500 bytes`. Every spelling produced here parses back to
    /// the same limit, so a limit quoted from a warning can be handed straight back
    /// to `--max-image-size`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.bytes();
        for (unit, name) in [(GIB, "GiB"), (MIB, "MiB"), (KIB, "KiB")] {
            if bytes.is_multiple_of(unit) {
                return write!(formatter, "{} {name}", bytes / unit);
            }
        }
        if bytes == 1 {
            formatter.write_str("1 byte")
        } else {
            write!(formatter, "{bytes} bytes")
        }
    }
}

/// Why a command-line spelling names no Image size limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImageSizeLimitParseError {
    /// The spelling does not start with a byte count.
    MissingNumber,
    /// The suffix after the count is not one of the accepted units.
    UnknownUnit(String),
    /// The spelling names zero bytes.
    Zero,
    /// The spelling names more bytes than fit in 64 bits.
    TooLarge,
}

impl fmt::Display for ImageSizeLimitParseError {
    /// Names the problem and the accepted spellings, for `clap` to print.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const ACCEPTED: &str = "use a whole number of bytes, optionally followed by K, M or G \
                                (binary units: 1M is 1,048,576 bytes)";
        match self {
            Self::MissingNumber => write!(formatter, "expected a size; {ACCEPTED}"),
            Self::UnknownUnit(unit) => {
                write!(formatter, "unrecognized size unit '{unit}'; {ACCEPTED}")
            }
            Self::Zero => formatter.write_str("the image size limit must be at least 1 byte"),
            Self::TooLarge => formatter.write_str("the image size limit is too large"),
        }
    }
}

impl std::error::Error for ImageSizeLimitParseError {}

impl FromStr for ImageSizeLimit {
    type Err = ImageSizeLimitParseError;

    /// Parses a whole byte count with an optional binary unit suffix.
    ///
    /// The units are `K`, `M` and `G`, each optionally followed by `iB`, and `B`,
    /// `byte` or `bytes` for plain bytes; case is ignored and whitespace may sit
    /// between the count and its unit. Decimal units such as `MB` are rejected
    /// rather than read as binary, so a limit never silently differs from the one
    /// the user meant by five percent.
    ///
    /// # Errors
    ///
    /// Returns [`ImageSizeLimitParseError`] for a missing count, an unknown unit, a
    /// limit of zero, or one that overflows 64 bits.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let digits_end = text
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(text.len());
        let (count, unit) = text.split_at(digits_end);
        if count.is_empty() {
            return Err(ImageSizeLimitParseError::MissingNumber);
        }
        // `count` is ASCII digits only, so overflow is the one way parsing can fail.
        let count: u64 = count
            .parse()
            .map_err(|_| ImageSizeLimitParseError::TooLarge)?;

        let unit = unit.trim_start();
        let multiplier = match unit.to_ascii_lowercase().as_str() {
            "" | "b" | "byte" | "bytes" => 1,
            "k" | "kib" => KIB,
            "m" | "mib" => MIB,
            "g" | "gib" => GIB,
            _ => return Err(ImageSizeLimitParseError::UnknownUnit(unit.to_string())),
        };

        let bytes = count
            .checked_mul(multiplier)
            .ok_or(ImageSizeLimitParseError::TooLarge)?;
        NonZeroU64::new(bytes)
            .map(Self::from_bytes)
            .ok_or(ImageSizeLimitParseError::Zero)
    }
}

#[cfg(test)]
mod tests;
