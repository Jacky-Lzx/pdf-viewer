//! Unicode default case folding with a fast path for ASCII search fields.
use caseless::Caseless;

pub fn fold(text: &str) -> String {
    let mut folded = String::new();
    fold_into(text, &mut folded);
    folded
}

/// Replace the contents while retaining capacity for callers with scratch storage.
pub fn fold_into(text: &str, folded: &mut String) {
    folded.clear();
    folded.reserve(text.len());
    if text.is_ascii() {
        folded.push_str(text);
        folded.make_ascii_lowercase();
    } else {
        for ch in text.chars() {
            if ch.is_ascii() {
                folded.push(ch.to_ascii_lowercase());
            } else {
                folded.extend(std::iter::once(ch).default_case_fold());
            }
        }
    }
}

/// Strip a prefix from already-folded text without allocating its folded form.
/// Stop at the first mismatch, including within multi-character expansions.
pub fn strip_prefix<'a>(folded: &'a str, prefix: &str) -> Option<&'a str> {
    let mut rest = folded;
    for ch in prefix.chars() {
        if ch.is_ascii() {
            rest = rest.strip_prefix(ch.to_ascii_lowercase())?;
        } else {
            for ch in std::iter::once(ch).default_case_fold() {
                rest = rest.strip_prefix(ch)?;
            }
        }
    }
    Some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unicode_scalar_matches_default_case_folding() {
        let mut text = String::new();
        let mut actual = String::new();
        for ch in (0..=0x10ffff).filter_map(char::from_u32) {
            text.clear();
            text.push(ch);
            fold_into(&text, &mut actual);
            assert_eq!(actual, caseless::default_case_fold_str(&text), "{ch:?}");
        }
    }

    #[test]
    fn mixed_sequences_expansions_and_reused_storage_preserve_full_text() {
        let mut actual = String::with_capacity(8192);
        for text in [
            "",
            "ASCII AZ az 09 /-_.\t\n",
            "Straße İ ﬁ ẞ ﬃ ΟΣ ς σ K ſ 中文 😀\u{301}",
            "A\u{301}İ\u{307} ΣΟς AßZ ﬃZ",
            "final ASCII field",
            "",
        ] {
            fold_into(text, &mut actual);
            let expected = caseless::default_case_fold_str(text);
            assert_eq!(actual, expected);
            assert_eq!(fold(text), expected);
            assert_eq!(actual.capacity(), 8192, "reuse caller-owned storage");
        }
    }

    #[test]
    fn streaming_prefix_matches_complete_folding_for_every_scalar() {
        let mut prefix = String::new();
        for ch in (0..=0x10ffff).filter_map(char::from_u32) {
            prefix.clear();
            prefix.push(ch);
            let folded = caseless::default_case_fold_str(&prefix);
            let query = format!("{folded}尾");
            assert_eq!(strip_prefix(&query, &prefix), Some("尾"), "{ch:?}");
            let mismatch = format!("!{query}");
            assert_eq!(
                strip_prefix(&mismatch, &prefix),
                mismatch.strip_prefix(&folded),
                "{ch:?}"
            );
        }
    }

    #[test]
    fn streaming_prefix_handles_expansions_boundaries_and_early_mismatches() {
        for prefix in ["", "ASCII-", "Straße-", "ﬃİſ", "ΟΣ-中文😀", "é", "e\u{301}"] {
            let folded = caseless::default_case_fold_str(prefix);
            for query in [
                folded.clone(),
                format!("{folded}中文😀1"),
                format!("!{folded}"),
                "stras".into(),
                "ffiix".into(),
                "".into(),
                "é".into(),
                "e\u{301}".into(),
            ] {
                assert_eq!(strip_prefix(&query, prefix), query.strip_prefix(&folded));
            }
        }
    }
}
