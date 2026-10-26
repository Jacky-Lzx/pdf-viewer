/// Keep 32 preceding and 80 following Unicode scalars, including the match start.
pub(super) fn snippet(text: &str, start: usize) -> String {
    let first = text[..start]
        .char_indices()
        .rev()
        .nth(31)
        .map_or(0, |(offset, _)| offset);
    let end = text[start..]
        .char_indices()
        .nth(80)
        .map_or(text.len(), |(offset, _)| start + offset);
    text[first..end].replace('\n', " ")
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
                assert_eq!(snippet(&text, start), original(&text, start), "{start}");
            }
        }
    }
}
