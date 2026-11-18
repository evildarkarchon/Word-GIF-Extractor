//! Tests for fitting output file names to what the filesystem can create.

use super::*;

/// Returns the base length past which `<base><suffix>.<extension>` exceeds the cap.
fn longest_fitting_base(suffix: &str, extension: &str) -> usize {
    MAX_NAME_UNITS - name_units(&format!("{suffix}.{extension}"))
}

/// Verifies a name at or under the cap is returned exactly as built.
#[test]
fn name_that_fits_is_unchanged() {
    for suffix in ["", "_1", "_12_3"] {
        let base = "a".repeat(longest_fitting_base(suffix, "jpeg"));
        let expected = format!("{base}{suffix}.jpeg");

        let fitted = fitted_file_name(&base, suffix, "jpeg");

        assert_eq!(name_units(&fitted), MAX_NAME_UNITS);
        assert_eq!(fitted, expected);
    }
    assert_eq!(
        fitted_file_name("Author - Title", "_2", "png"),
        "Author - Title_2.png"
    );
}

/// Verifies a name one unit over the cap keeps its suffix and extension whole.
///
/// The base is cut to the reserve length rather than by the single unit it is over,
/// so the result is shorter than the cap by the reserve's unused room.
#[test]
fn overlong_name_loses_characters_from_the_end_of_its_base() {
    let base = "b".repeat(longest_fitting_base("_1", "png") + 1);

    let fitted = fitted_file_name(&base, "_1", "png");

    let kept = MAX_NAME_UNITS - SHORTENED_SUFFIX_RESERVE;
    assert_eq!(fitted, format!("{}_1.png", "b".repeat(kept)));
}

/// Verifies every overlong name in one document shares one stem.
///
/// Ordinals of different widths, a different extension and a collision suffix
/// would each need a different cut if each name were fitted exactly.
#[test]
fn overlong_names_of_one_document_share_one_stem() {
    let base = "Author - ".to_string() + &"Title ".repeat(60);

    let stems = [
        ("", "png"),
        ("_1", "png"),
        ("_10", "jpg"),
        ("_99999", "webp"),
        ("_3_1000", "tiff"),
    ]
    .map(|(suffix, extension)| {
        let fitted = fitted_file_name(&base, suffix, extension);
        assert!(name_units(&fitted) <= MAX_NAME_UNITS, "{fitted}");
        fitted
            .strip_suffix(&format!("{suffix}.{extension}"))
            .expect("suffix and extension should be kept whole")
            .to_string()
    });

    assert!(stems.iter().all(|stem| stem == &stems[0]), "{stems:?}");
    assert!(base.starts_with(&stems[0]));
}

/// Verifies a suffix wider than the reserve still yields a name that fits.
#[test]
fn ordinal_wider_than_the_reserve_is_fitted_exactly() {
    let base = "c".repeat(300);
    let suffix = "_1234567890_1000";

    let fitted = fitted_file_name(&base, suffix, "webp");

    assert_eq!(name_units(&fitted), MAX_NAME_UNITS);
    assert!(fitted.ends_with("_1234567890_1000.webp"));
}

/// Verifies the cut never splits a character and drops whitespace it leaves behind.
///
/// Each emoji is two UTF-16 units and four UTF-8 bytes, so an odd budget falls
/// inside one on every host.
#[test]
fn cut_falls_on_a_character_boundary() {
    let base = "😀".repeat(200);

    let fitted = fitted_file_name(&base, "_1", "png");

    assert!(name_units(&fitted) <= MAX_NAME_UNITS);
    let stem = fitted
        .strip_suffix("_1.png")
        .expect("suffix should be kept");
    assert!(stem.chars().all(|character| character == '😀'));
    assert!(base.starts_with(stem));
}

/// Verifies whitespace left at the end of a shortened base is dropped.
#[test]
fn cut_drops_trailing_whitespace() {
    let kept = MAX_NAME_UNITS - SHORTENED_SUFFIX_RESERVE;
    // The cut lands just after the run of spaces, which would otherwise end the stem.
    let base = format!("{}   {}", "d".repeat(kept - 3), "e".repeat(50));

    let fitted = fitted_file_name(&base, "", "png");

    assert_eq!(fitted, format!("{}.png", "d".repeat(kept - 3)));
}

/// Verifies the cap is counted in UTF-16 units on Windows.
///
/// `é` is one UTF-16 unit but two UTF-8 bytes. NTFS holds 250 of them in one name,
/// so a Windows run must not shorten that name as if it were 500 bytes long.
#[cfg(windows)]
#[test]
fn length_is_counted_in_utf16_units_on_windows() {
    let base = "é".repeat(250);

    assert_eq!(fitted_file_name(&base, "", "png"), format!("{base}.png"));
}

/// Verifies the cap is counted in UTF-8 bytes off Windows.
///
/// `é` is two UTF-8 bytes, so 250 of them are 500 bytes and ext4 rejects the name.
#[cfg(not(windows))]
#[test]
fn length_is_counted_in_utf8_bytes_off_windows() {
    let base = "é".repeat(250);

    let fitted = fitted_file_name(&base, "", "png");

    assert!(fitted.len() <= MAX_NAME_UNITS, "{} bytes", fitted.len());
    assert!(fitted.ends_with(".png"));
}

/// Verifies a name Windows would open as a device gains a leading underscore.
#[test]
fn device_name_gains_a_leading_underscore() {
    let cases = [
        ("NUL", "", "_NUL.png"),
        ("nul", "", "_nul.png"),
        ("Con. A Novel", "", "_Con. A Novel.png"),
        ("Con. A Novel", "_2", "_Con. A Novel_2.png"),
        ("AUX ", "", "_AUX .png"),
        ("COM1", "", "_COM1.png"),
        ("lpt²", "", "_lpt².png"),
        ("CONIN$", "", "_CONIN$.png"),
    ];

    for (base, suffix, expected) in cases {
        assert_eq!(fitted_file_name(base, suffix, "png"), expected, "{base:?}");
    }
}

/// Verifies names that only resemble a device name are left alone.
///
/// A numbered name's part before the dot includes its ordinal, so `NUL_1.png` is
/// not a device even though `NUL.png` is.
#[test]
fn name_that_only_resembles_a_device_is_unchanged() {
    let cases = [
        ("NUL", "_1", "NUL_1.png"),
        ("NULL", "", "NULL.png"),
        ("CONSOLE", "", "CONSOLE.png"),
        ("COM10", "", "COM10.png"),
        ("Author - CON", "", "Author - CON.png"),
    ];

    for (base, suffix, expected) in cases {
        assert_eq!(fitted_file_name(base, suffix, "png"), expected, "{base:?}");
    }
}

/// Verifies a device name that is also overlong is both prefixed and fitted.
#[test]
fn overlong_device_name_is_prefixed_and_fitted() {
    let base = format!("Con.{}", "f".repeat(300));

    let fitted = fitted_file_name(&base, "", "png");

    assert_eq!(
        name_units(&fitted),
        MAX_NAME_UNITS - SHORTENED_SUFFIX_RESERVE + 4
    );
    assert!(fitted.starts_with("_Con.fff"));
    assert!(fitted.ends_with("f.png"));
}
