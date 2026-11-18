//! Fitting an output file name to what the filesystem will create.
//!
//! Image file emission builds each name from the Output placement's base name, an
//! ordinal or collision suffix, and the image's extension. Two things can stop the
//! operating system creating that name even though every character in it is legal
//! (ADR-0032):
//!
//! - **Length.** NTFS, ext4, APFS and exFAT cap one path component at 255 units, so
//!   a long EPUB title used to fail its whole document with a create error. The cap
//!   is on one name, not the whole path. Rust's standard library already gives a
//!   Windows path past `MAX_PATH` the `\\?\` form, and neither `LongPathsEnabled`
//!   nor a `longPathAware` manifest raises the per-name cap. Long-path support would
//!   not help, so the cap is checked on every system. It shortens only a name that
//!   is over it, and a name that fits is never changed.
//! - **Windows device names.** Windows 10 and earlier treat a name as a device when
//!   the part before its first dot is `NUL`, `CON`, `COM1` or the like, whether or
//!   not an extension follows. Such a name gains a leading `_`. This applies on
//!   every platform, so that the same document gets the same names everywhere and a
//!   file stays usable when it is later copied to Windows.

use std::borrow::Cow;

/// The most units one file name may hold, on every filesystem this tool targets.
const MAX_NAME_UNITS: usize = 255;

/// The room kept for the suffix and extension once a base name has to be shortened.
///
/// It fits `_99999_1000.webp`: a five-digit ordinal, the largest collision suffix
/// and the longest extension. Cutting every overlong name in a document to the same
/// base length keeps the document's images under one stem. Cutting each name just
/// enough to fit would leave `_9.png` and `_10.png` on stems one character apart.
const SHORTENED_SUFFIX_RESERVE: usize = 16;

/// Device names Windows reserves, compared ignoring ASCII case.
///
/// Windows treats the superscript digits `¹`, `²` and `³` as digits in these names,
/// and opens the console for `CONIN$` and `CONOUT$`.
const RESERVED_DEVICE_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
    "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³", "CONIN$", "CONOUT$",
];

/// Builds one output file's name, fitted to what the filesystem can create.
///
/// `suffix` is everything between the base name and the extension's dot. It is
/// empty for a singular name and otherwise one of `_<ordinal>`, `_<collision>` or
/// `_<ordinal>_<collision>`. The result is `<base><suffix>.<extension>`, unchanged
/// unless it would name a Windows device or exceed the length cap. A device name
/// gains a leading `_`. An overlong name keeps its suffix and extension whole and
/// loses characters from the end of its base name. It never splits a character,
/// and it drops any whitespace the cut leaves at the end of the base.
pub(super) fn fitted_file_name(base: &str, suffix: &str, extension: &str) -> String {
    let tail = format!("{suffix}.{extension}");
    let base: Cow<'_, str> = if names_a_device(&format!("{base}{tail}")) {
        Cow::Owned(format!("_{base}"))
    } else {
        Cow::Borrowed(base)
    };

    let tail_units = name_units(&tail);
    if name_units(&base) + tail_units <= MAX_NAME_UNITS {
        return format!("{base}{tail}");
    }

    // A tail longer than the reserve has an ordinal past five digits. It is then fitted
    // exactly, and only that image's stem is shorter than the document's others.
    // Shortening cannot turn a name into a device name. If the base's first dot
    // survives the cut, the part before it is unchanged. If it does not, that part
    // is the whole kept base plus the suffix, which is far longer than any device name.
    let budget = MAX_NAME_UNITS.saturating_sub(tail_units.max(SHORTENED_SUFFIX_RESERVE));
    format!("{}{tail}", shorten(&base, budget).trim_end())
}

/// Checks whether Windows would open a device for this file name.
///
/// Windows reads only the part before the first dot, ignoring trailing spaces, so
/// `nul.png`, `NUL .png` and `Con. A Novel.png` all name a device.
fn names_a_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    RESERVED_DEVICE_STEMS
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

/// Returns the longest prefix of `base` that fits in `budget` units.
fn shorten(base: &str, budget: usize) -> &str {
    let mut used = 0;
    for (index, character) in base.char_indices() {
        used += character_units(character);
        if used > budget {
            return &base[..index];
        }
    }
    base
}

/// Measures a whole name in the host filesystem's units.
fn name_units(name: &str) -> usize {
    name.chars().map(character_units).sum()
}

/// Measures one character the way the host's filesystems count a name's length.
///
/// Windows filesystems count UTF-16 code units, and Linux and macOS ones count UTF-8
/// bytes. Using the host's own unit means a name that fits there is never shortened
/// to suit a stricter count that does not apply to it. For example, a CJK title fits
/// 255 characters on NTFS but only 85 on ext4.
#[cfg(windows)]
fn character_units(character: char) -> usize {
    character.len_utf16()
}

/// Measures one character the way the host's filesystems count a name's length.
///
/// See the Windows variant for why the unit follows the host.
#[cfg(not(windows))]
fn character_units(character: char) -> usize {
    character.len_utf8()
}

#[cfg(test)]
mod tests;
