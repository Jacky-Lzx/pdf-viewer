/// Keep 32 preceding and 80 following Unicode scalars, including the match start.
pub(super) fn snippet(
    text: &str,
    matched: std::ops::Range<usize>,
) -> (String, std::ops::Range<usize>) {
    let start = matched.start;
    let first = text[..start]
        .char_indices()
        .rev()
        .nth(31)
        .map_or(0, |(offset, _)| offset);
    let end = text[start..]
        .char_indices()
        .nth(80)
        .map_or(text.len(), |(offset, _)| start + offset);
    // Replacing LF with a space preserves byte offsets, including matches
    // spanning lines. A long match may extend beyond the preview budget.
    (
        text[first..end].replace('\n', " "),
        start - first..matched.end.min(end) - first,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_match_original_at_every_unicode_boundary_and_context_limit() {
        // Preserve the original character limits, ordering and newline replacement.
        fn original(text: &str, start: usize) -> String {
            let before: String = text[..start]
                .chars()
                .rev()
                .take(32)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let after: String = text[start..].chars().take(80).collect();
            format!("{before}{after}").replace('\n', " ")
        }

        for text in [
            String::new(),
            "\n\r\n\t\0".into(),
            "ASCII Rust text\n".repeat(20),
            "Straße Σσς ﬃ 中文 İ cafe\u{301} 👩‍💻 🇨🇳\r\n".repeat(8),
            "a".repeat(31),
            "中".repeat(32),
            "👩".repeat(33),
            "\n".repeat(80),
            "ß".repeat(81),
            "e\u{301}".repeat(113),
        ] {
            for start in text
                .char_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(text.len()))
            {
                assert_eq!(
                    snippet(&text, start..start).0,
                    original(&text, start),
                    "{start}"
                );
            }
        }
    }

    #[test]
    fn matched_preview_offsets_preserve_unicode_newlines_and_clipped_long_matches() {
        let text = format!("{}Straße\n中文{}", "前".repeat(40), "后".repeat(100));
        let start = text.find("Straße").unwrap();
        let end = text.find("中文").unwrap() + "中文".len();
        let (preview, matched) = snippet(&text, start..end);
        assert_eq!(&preview[matched.clone()], "Straße 中文");
        assert_eq!(preview[..matched.start], "前".repeat(32));
        let (preview, matched) = snippet(&text, start..text.len());
        assert_eq!(matched.end, preview.len());
        assert_eq!(preview[matched].chars().count(), 80);
    }
}
