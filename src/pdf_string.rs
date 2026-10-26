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
    // PDFDocEncoding differs from Latin-1 in these two ranges and at 0xA0.
    const DIACRITICS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const SPECIAL: [char; 32] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '�',
    ];
    bytes
        .iter()
        .map(|byte| match byte {
            0x18..=0x1f => DIACRITICS[(byte - 0x18) as usize],
            0x80..=0x9f => SPECIAL[(byte - 0x80) as usize],
            0xa0 => '€',
            0x7f | 0xad => '�',
            _ => char::from(*byte),
        })
        .collect()
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
}
