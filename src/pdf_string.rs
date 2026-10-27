pub fn decode(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair));
        let mut text = String::with_capacity(units.len());
        text.extend(
            char::decode_utf16(units).map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER)),
        );
        return text;
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    // PDFDocEncoding's ASCII control accents and DEL need the table below.
    // All other ASCII bytes can be copied without re-encoding each character.
    if bytes
        .iter()
        .all(|&byte| byte < 0x18 || (0x20..=0x7e).contains(&byte))
    {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    bytes.iter().map(|&byte| pdf_doc_character(byte)).collect()
}

/// Decode a bounded scalar prefix without allocating the discarded suffix.
/// The flag reports whether the decoded text has an omitted suffix.
pub fn decode_prefix(bytes: &[u8], limit: usize) -> (String, bool) {
    // Every supported encoding yields at most one scalar per input byte.
    // Short titles retain the existing bulk decoding path.
    if bytes.len() <= limit {
        return (decode(bytes), false);
    }
    fn collect(mut characters: impl Iterator<Item = char>, limit: usize) -> (String, bool) {
        let mut text = String::new();
        text.extend(characters.by_ref().take(limit));
        (text, characters.next().is_some())
    }

    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair));
        return collect(
            char::decode_utf16(units).map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER)),
            limit,
        );
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        let prefix = &bytes[..bytes.len().min(limit)];
        if prefix.is_ascii() {
            return (
                String::from_utf8_lossy(prefix).into_owned(),
                bytes.len() > limit,
            );
        }
        // Four bytes per scalar, including one lookahead, suffice even if
        // this byte window cuts a multibyte character. Such a cut can only
        // affect characters beyond the requested prefix and its lookahead.
        let maximum = limit.saturating_add(1).saturating_mul(4);
        let mut text = String::new();
        let mut remaining = limit;
        for chunk in bytes[..bytes.len().min(maximum)].utf8_chunks() {
            let valid = chunk.valid();
            if let Some((end, _)) = valid.char_indices().nth(remaining) {
                text.push_str(&valid[..end]);
                return (text, true);
            }
            text.push_str(valid);
            remaining -= valid.chars().count();
            if !chunk.invalid().is_empty() {
                if remaining == 0 {
                    return (text, true);
                }
                text.push(char::REPLACEMENT_CHARACTER);
                remaining -= 1;
            }
        }
        return (text, false);
    }
    // Each PDFDocEncoding byte is exactly one scalar. Inspect and copy only
    // the retained bytes on the common ASCII path, without scanning a suffix.
    let prefix = &bytes[..bytes.len().min(limit)];
    let text = if prefix
        .iter()
        .all(|&byte| byte < 0x18 || (0x20..=0x7e).contains(&byte))
    {
        String::from_utf8_lossy(prefix).into_owned()
    } else {
        prefix.iter().map(|&byte| pdf_doc_character(byte)).collect()
    };
    (text, bytes.len() > limit)
}

fn pdf_doc_character(byte: u8) -> char {
    // PDFDocEncoding differs from Latin-1 in these two ranges and at 0xA0.
    const DIACRITICS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const SPECIAL: [char; 32] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '�',
    ];
    match byte {
        0x18..=0x1f => DIACRITICS[(byte - 0x18) as usize],
        0x80..=0x9f => SPECIAL[(byte - 0x80) as usize],
        0xa0 => '€',
        0x7f | 0xad => '�',
        _ => char::from(byte),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pdf_titles_decode_utf16_and_pdf_doc_encoding() {
        use super::decode;

        assert_eq!(decode(&[0xfe, 0xff, 0x4e, 0x2d, 0x65, 0x87]), "中文");
        assert_eq!(decode(b"Hello\x84world\xa0"), "Hello—world€");
        assert_eq!(decode(b"\xef\xbb\xbfUTF8"), "UTF8");
    }

    #[test]
    fn utf16_stream_matches_lossy_decoding_for_surrogates_and_all_code_units() {
        use super::decode;

        let mut cases = vec![
            vec![],
            vec![0xd800],
            vec![0xdc00],
            vec![0xd800, 0x61],
            vec![0xd800, 0xd800, 0xdc00],
            vec![0xdc00, 0xd800, 0xdc00],
            vec![0xd83d, 0xde00, 0xdbff, 0xdfff],
            (0..=u16::MAX).collect(),
        ];
        cases.extend(
            ["Chapter 4 中文 😀 𝄞", "Ελληνικά Straße Résumé"]
                .map(|text| text.encode_utf16().collect()),
        );
        for units in cases {
            let mut bytes = vec![0xfe, 0xff];
            bytes.extend(units.iter().flat_map(|unit| unit.to_be_bytes()));
            let expected = String::from_utf16_lossy(&units);
            assert_eq!(decode(&bytes), expected);
            // Preserve the previous treatment of an incomplete final code unit.
            bytes.push(0x61);
            assert_eq!(decode(&bytes), expected);
        }
        assert_eq!(decode(b"\xef\xbb\xbf\xff"), "�");
    }

    #[test]
    fn ascii_copy_preserves_pdf_control_accents_and_encoding_boundaries() {
        use super::decode;

        for byte in 0..0x7f {
            if !(0x18..=0x1f).contains(&byte) {
                assert_eq!(decode(&[byte]), char::from(byte).to_string());
            }
        }
        assert_eq!(decode(b""), "");
        assert_eq!(
            decode(b"Chapter 4\tPerformance\n"),
            "Chapter 4\tPerformance\n"
        );
        assert_eq!(decode(b"\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f"), "˘ˇˆ˙˝˛˚˜");
        assert_eq!(decode(b"before\x7fafter"), "before�after");
        assert_eq!(decode(b"before\x84after\xa0"), "before—after€");
        // A UTF-8-looking byte sequence without its BOM is still PDFDocEncoding.
        assert_eq!(decode(b"\xc3\xa9"), "Ã©");
    }

    #[test]
    fn bounded_decoding_matches_complete_decoding_at_scalar_and_encoding_boundaries() {
        use super::{decode, decode_prefix};
        let mut cases = vec![
            Vec::new(),
            (0..=u8::MAX).collect(),
            b"ASCII\t title\n".to_vec(),
            b"\xef\xbb\xbf\xff\xe2\x82\xac\xe2\x82\xe2\x82\xac\xf0\x9f\x98\x80\xc0\xaf".to_vec(),
            b"\xef\xbb\xbf\xf0\x9f\x98".to_vec(),
        ];
        for text in ["", "a", "中😀𝄞ΣStraße", &"中文😀 ".repeat(2200)] {
            let mut utf16 = vec![0xfe, 0xff];
            utf16.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
            cases.push(utf16.clone());
            utf16.push(0xd8); // An incomplete code unit remains ignored.
            cases.push(utf16);
            let mut utf8 = vec![0xef, 0xbb, 0xbf];
            utf8.extend_from_slice(text.as_bytes());
            cases.push(utf8);
        }
        let mut surrogates = vec![0xfe, 0xff];
        surrogates.extend((0..=u16::MAX).flat_map(u16::to_be_bytes));
        cases.push(surrogates);
        for bytes in cases {
            let full = decode(&bytes);
            for limit in [0, 1, 2, 3, 4, 7, 16, 255, 256, 2047, 2048, 2049, usize::MAX] {
                let (text, truncated) = decode_prefix(&bytes, limit);
                assert_eq!(text, full.chars().take(limit).collect::<String>());
                assert_eq!(truncated, full.chars().count() > limit);
            }
        }
        // Exhaust all short invalid UTF-8 combinations, including truncated
        // sequences and boundaries where one replacement consumes many bytes.
        for first in 0..=u8::MAX {
            for second in 0..=u8::MAX {
                let bytes = [0xef, 0xbb, 0xbf, first, second];
                let full = String::from_utf8_lossy(&bytes[3..]);
                for limit in 0..=2 {
                    let (text, truncated) = decode_prefix(&bytes, limit);
                    assert_eq!(text, full.chars().take(limit).collect::<String>());
                    assert_eq!(truncated, full.chars().count() > limit);
                }
            }
        }
    }

    #[test]
    fn utf8_byte_windows_preserve_long_four_byte_sequences_and_invalid_chunks() {
        use super::decode_prefix;
        let check = |bytes: &[u8], limit| {
            let mut encoded = vec![0xef, 0xbb, 0xbf];
            encoded.extend_from_slice(bytes);
            let full = String::from_utf8_lossy(bytes);
            let (text, truncated) = decode_prefix(&encoded, limit);
            assert_eq!(text, full.chars().take(limit).collect::<String>());
            assert_eq!(truncated, full.chars().count() > limit);
        };
        let text = "😀𝄞".repeat(2200);
        for length in 0..=80 {
            for limit in 0..=16 {
                check(&text.as_bytes()[..length], limit);
            }
        }
        for limit in [255, 256, 2047, 2048, 2049] {
            check(text.as_bytes(), limit);
        }
        let mut random = 17u64;
        for _ in 0..512 {
            let bytes: Vec<_> = (0..96)
                .map(|_| {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    (random >> 32) as u8
                })
                .collect();
            for limit in 0..=24 {
                check(&bytes, limit);
            }
        }
    }
}
