//! Image format detection by file header signatures.

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1A\n";
const JPG_MAGIC: &[u8] = b"\xFF\xD8\xFF";
const GIF_MAGIC: &[u8] = b"GIF8";
const BMP_MAGIC: &[u8] = b"BM";
const TIFF_LE_MAGIC: &[u8] = b"II\x2A\x00";
const TIFF_BE_MAGIC: &[u8] = b"MM\x00\x2A";
const RIFF_MAGIC: &[u8] = b"RIFF";
const WEBP_MAGIC: &[u8] = b"WEBP";
const ICO_MAGIC: &[u8] = b"\x00\x00\x01\x00";
const WMF_PLACEABLE_MAGIC: &[u8] = b"\xD7\xCD\xC6\x9A";
const WMF_STANDARD_MAGIC: &[u8] = b"\x01\x00\x09\x00";
const EMF_PREFIX_MAGIC: &[u8] = b"\x01\x00\x00\x00";
const EMF_SIGNATURE_OFFSET: usize = 40;
const EMF_SIGNATURE: &[u8] = b" EMF";
const SVG_SEARCH_LIMIT: usize = 1024;

/// Detects a supported image format from the provided header/content bytes.
///
/// Most supported formats are identified from the first 12 bytes. EMF is the
/// exception required by the OpenSpec scenario: it has the `01 00 00 00` prefix
/// and the standard ` EMF` signature at offset 40.
pub fn detect_image_format(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(PNG_MAGIC) {
        return Some("png");
    }
    if data.starts_with(JPG_MAGIC) {
        return Some("jpg");
    }
    if data.starts_with(GIF_MAGIC) {
        return Some("gif");
    }
    if data.starts_with(BMP_MAGIC) {
        return Some("bmp");
    }
    if data.starts_with(TIFF_LE_MAGIC) || data.starts_with(TIFF_BE_MAGIC) {
        return Some("tiff");
    }
    if data.len() >= 12 && data.starts_with(RIFF_MAGIC) && &data[8..12] == WEBP_MAGIC {
        return Some("webp");
    }
    if data.starts_with(ICO_MAGIC) {
        return Some("ico");
    }
    if data.starts_with(WMF_PLACEABLE_MAGIC) || data.starts_with(WMF_STANDARD_MAGIC) {
        return Some("wmf");
    }
    if is_emf(data) {
        return Some("emf");
    }
    if is_svg(data) {
        return Some("svg");
    }

    None
}

fn is_emf(data: &[u8]) -> bool {
    data.starts_with(EMF_PREFIX_MAGIC)
        && data
            .get(EMF_SIGNATURE_OFFSET..EMF_SIGNATURE_OFFSET + EMF_SIGNATURE.len())
            .is_some_and(|signature| signature == EMF_SIGNATURE)
}

fn is_svg(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let search_window = &data[..data.len().min(SVG_SEARCH_LIMIT)];

    (0..search_window.len()).any(|start| {
        starts_with_ignore_ascii_case(&search_window[start..], b"<svg")
            && matches!(
                search_window.get(start + 4),
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>')
            )
    })
}

fn starts_with_ignore_ascii_case(data: &[u8], prefix: &[u8]) -> bool {
    data.len() >= prefix.len()
        && data[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png() {
        assert_eq!(detect_image_format(b"\x89PNG\r\n\x1A\nmore"), Some("png"));
    }

    #[test]
    fn detects_jpg() {
        assert_eq!(detect_image_format(b"\xFF\xD8\xFF\xE0"), Some("jpg"));
    }

    #[test]
    fn detects_gif() {
        assert_eq!(detect_image_format(b"GIF89a"), Some("gif"));
    }

    #[test]
    fn detects_bmp() {
        assert_eq!(detect_image_format(b"BMrest"), Some("bmp"));
    }

    #[test]
    fn detects_tiff_little_endian() {
        assert_eq!(detect_image_format(b"II\x2A\x00rest"), Some("tiff"));
    }

    #[test]
    fn detects_tiff_big_endian() {
        assert_eq!(detect_image_format(b"MM\x00\x2Arest"), Some("tiff"));
    }

    #[test]
    fn detects_webp() {
        assert_eq!(
            detect_image_format(b"RIFF\x00\x00\x00\x00WEBP"),
            Some("webp")
        );
    }

    #[test]
    fn detects_ico() {
        assert_eq!(detect_image_format(b"\x00\x00\x01\x00rest"), Some("ico"));
    }

    #[test]
    fn detects_wmf_placeable() {
        assert_eq!(detect_image_format(b"\xD7\xCD\xC6\x9Arest"), Some("wmf"));
    }

    #[test]
    fn detects_wmf_standard() {
        assert_eq!(detect_image_format(b"\x01\x00\x09\x00rest"), Some("wmf"));
    }

    #[test]
    fn detects_emf() {
        let mut data = vec![0x01, 0x00, 0x00, 0x00];
        data.resize(40, 0);
        data.extend_from_slice(b" EMF");
        assert_eq!(detect_image_format(&data), Some("emf"));
    }

    #[test]
    fn ignores_emf_prefix_without_signature() {
        let mut data = vec![0x01, 0x00, 0x00, 0x00];
        data.resize(44, 0);
        assert_eq!(detect_image_format(&data), None);
    }

    #[test]
    fn detects_svg_with_svg_prefix() {
        assert_eq!(detect_image_format(b"  <svg xmlns=\"x\"/>"), Some("svg"));
    }

    #[test]
    fn detects_svg_with_xml_prefix() {
        assert_eq!(
            detect_image_format(b"\xEF\xBB\xBF\n<?xml version=\"1.0\"?><svg/>"),
            Some("svg")
        );
    }

    #[test]
    fn ignores_non_svg_xml() {
        assert_eq!(detect_image_format(b"<?xml version=\"1.0\"?><root/>"), None);
    }

    #[test]
    fn returns_none_for_unknown_data() {
        assert_eq!(detect_image_format(b"not an image"), None);
    }

    #[test]
    fn returns_none_for_short_data() {
        assert_eq!(detect_image_format(b"RIF"), None);
    }
}
