# Fit each output name to what the filesystem can create

The 2026-11-14 audit (L5) found two kinds of output name that the operating system refuses or misreads, although every character in them is legal. An EPUB's base name is its creator and title, so a long subtitle can push `<base>_<n>.<ext>` past 255 units. That is the cap NTFS, ext4, APFS and exFAT put on one path component. The create fails and the whole document fails with it. On Windows the error is `os error 123`. An Office document's base name is its file stem, which already fits, but it can still go over once an ordinal and extension are added. A title such as `NUL`, `CON` or `COM1` gives a name that Windows 10 and earlier open as a device. Those versions read only the part before the first dot, so `NUL.png` and `Con. A Novel.png` both name devices. Windows 11 creates both as ordinary files.

## Why long-path support is not the fix

The first proposal was to make the program long-path aware, so that names would be shortened only on systems that cannot use long paths. That does not reach this failure. `MAX_PATH`, `LongPathsEnabled` and the `longPathAware` manifest setting govern a whole path's length, not one name's. Rust's standard library already gives a Windows path past `MAX_PATH` the `\\?\` form. A probe on a machine with `LongPathsEnabled` set wrote a 550-character path without a manifest. On the same machine, a single 256-unit name failed. No setting lifts the per-name cap, so no system exists on which an overlong name could be kept. A manifest would change nothing this program does, and none is added.

## Decision

Image file emission fits every candidate name before claiming it. ADR-0022 makes it the one owner of output names, and a fitted name depends on the ordinal, collision suffix and extension, which only emission knows. Document identity still only replaces illegal characters, and an Output placement still carries the base name unrevised.

- **Only a name that does not fit is changed.** A name of 255 units or fewer is used exactly as built, so every name that worked before this change is byte-for-byte unchanged.
- **The cut comes from the end of the base name.** The suffix and extension are kept whole, the cut falls on a character boundary, and whitespace the cut leaves at the end of the base is dropped.
- **Every overlong name in one document is cut to the same base length.** The base is cut to 255 units minus a 16-unit reserve, which fits `_99999_1000.webp`. Cutting each name just enough to fit would give `_9.png` and `_10.png` stems one character apart. Only a suffix wider than the reserve, from an ordinal past five digits, is fitted exactly.
- **Length is counted in the host filesystem's unit.** That is UTF-16 code units on Windows and UTF-8 bytes elsewhere. A single cross-platform count would have to be the stricter of the two. That would cut a 200-character CJK title that NTFS holds, contradicting the first rule.
- **A Windows device name gains a leading `_`.** The check is case-insensitive on the part of the full name before the first dot, ignoring trailing spaces, against `CON`, `PRN`, `AUX`, `NUL`, `COM0` to `COM9`, `LPT0` to `LPT9`, their superscript `¹²³` forms, `CONIN$` and `CONOUT$`. It applies on every platform. Unlike length, a device name fails on another system after the file is copied there, not on the system that writes it. The same document should get the same names everywhere, as ADR-0031 decided for search order. Telling Windows 10 from Windows 11 at run time would cost more than the prefix. A numbered name such as `NUL_1.png` reads `NUL_1` before its dot and is unchanged.

The collision suffix now follows the fitted ordinal name, so `<base>_<ordinal>_<collision>.<ext>` is fitted as one name. For any non-empty base this is the name emission built before. Running out of collision suffixes is reported with the unfitted stem, in the same wording as before.

## Consequences

ADR-0022 is extended, not replaced. Its list of naming decisions gains one item, fitting, inside the same module. Its out-of-scope note about changing naming behaviour bound only that refactor. Output names change only for a name over 255 units, which previously failed, and for a name that is a device name before its first dot. The README's Output Naming section states both rules. The glossary's Image file emission entry says names are fitted. Nothing here becomes public, and ADR-0003 still governs the library interface.
