use super::SearchOptions;
use std::ops::Range;

pub struct SearchQuery {
    pub(super) matcher: memchr::memmem::Finder<'static>,
    pub(super) options: SearchOptions,
    phrase: Option<Phrase>,
    restart_rejected: bool,
}

struct Phrase {
    before: Vec<String>,
    after: Vec<String>,
    leading_space: bool,
    trailing_space: bool,
}

impl SearchQuery {
    pub fn new(query: String, options: SearchOptions) -> Self {
        let needle = if query.trim().is_empty() {
            String::new()
        } else if options.match_case {
            query
        } else {
            caseless::default_case_fold_str(&query)
        };
        let (anchor, phrase) = if !needle.is_empty() && needle.contains(char::is_whitespace) {
            let words: Vec<_> = needle.split_whitespace().collect();
            // A long literal anchor avoids scanning every occurrence of a
            // common short first word when the rest of the phrase is absent.
            let anchor = words
                .iter()
                .enumerate()
                .max_by_key(|&(index, word)| (word.len(), std::cmp::Reverse(index)))
                .map(|(index, _)| index)
                .unwrap();
            (
                words[anchor],
                Some(Phrase {
                    before: words[..anchor]
                        .iter()
                        .map(|word| (*word).to_owned())
                        .collect(),
                    after: words[anchor + 1..]
                        .iter()
                        .map(|word| (*word).to_owned())
                        .collect(),
                    leading_space: needle.starts_with(char::is_whitespace),
                    trailing_space: needle.ends_with(char::is_whitespace),
                }),
            )
        } else {
            (needle.as_str(), None)
        };
        // Most word anchors cannot overlap themselves. Keep the iterator's
        // scan state on a rejected phrase instead of rebuilding it per word.
        let restart_rejected = phrase.is_none()
            || anchor
                .as_bytes()
                .first()
                .is_some_and(|&first| memchr::memchr(first, &anchor.as_bytes()[1..]).is_some());
        Self {
            matcher: memchr::memmem::Finder::new(anchor.as_bytes()).into_owned(),
            options,
            phrase,
            restart_rejected,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.matcher.needle().is_empty()
    }

    pub(super) fn is_phrase(&self) -> bool {
        self.phrase.is_some()
    }

    pub(super) fn restart_rejected(&self) -> bool {
        self.restart_rejected
    }

    pub(super) fn anchor_starts_phrase(&self) -> bool {
        self.phrase
            .as_ref()
            .is_some_and(|phrase| phrase.before.is_empty() && !phrase.leading_space)
    }

    /// Keep offsets in the original (or case-folded) text. Whitespace can span
    /// several lines without another normalized text copy or source map.
    pub(super) fn match_range(&self, text: &str, anchor: usize) -> Option<Range<usize>> {
        let mut start = anchor;
        let mut end = anchor + self.matcher.needle().len();
        if let Some(phrase) = &self.phrase {
            for word in phrase.before.iter().rev() {
                start = space_before(text, start)?;
                start = text[..start].strip_suffix(word)?.len();
            }
            for word in &phrase.after {
                end = space_after(text, end)?;
                text[end..].strip_prefix(word)?;
                end += word.len();
            }
            if phrase.leading_space {
                start = space_before(text, start)?;
            }
            if phrase.trailing_space {
                end = space_after(text, end)?;
            }
        }
        Some(start..end)
    }
}

fn space_before(text: &str, end: usize) -> Option<usize> {
    let start = text[..end].trim_end_matches(char::is_whitespace).len();
    (start < end).then_some(start)
}

fn space_after(text: &str, start: usize) -> Option<usize> {
    let end = text.len() - text[start..].trim_start_matches(char::is_whitespace).len();
    (end > start).then_some(end)
}
