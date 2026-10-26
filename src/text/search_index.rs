use caseless::Caseless;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct SearchIndex {
    pub folded: String,
    // ASCII folding preserves every byte offset, so it needs no source map.
    pub source: Option<Vec<usize>>,
}

impl SearchIndex {
    pub fn new(text: &str) -> Self {
        if text.is_ascii() {
            return Self {
                folded: text.to_ascii_lowercase(),
                source: None,
            };
        }
        let mut folded = String::with_capacity(text.len());
        let mut source = Vec::with_capacity(text.len());
        for (offset, ch) in text.char_indices() {
            if ch.is_ascii() {
                folded.push(ch.to_ascii_lowercase());
                source.push(offset);
                continue;
            }
            for lower in std::iter::once(ch).default_case_fold() {
                folded.push(lower);
                source.extend(std::iter::repeat_n(offset, lower.len_utf8()));
            }
        }
        Self {
            folded,
            source: Some(source),
        }
    }

    pub fn source_range(&self, text: &str, range: Range<usize>) -> Range<usize> {
        let Some(source) = &self.source else {
            return range;
        };
        let start = source[range.start];
        let last = source[range.end - 1];
        start..last + text[last..].chars().next().unwrap().len_utf8()
    }

    pub fn partial_expansion(&self, range: Range<usize>) -> bool {
        self.source.as_ref().is_some_and(|source| {
            (range.start > 0 && source[range.start - 1] == source[range.start])
                || (range.end < source.len() && source[range.end - 1] == source[range.end])
        })
    }

    pub fn memory_bytes(&self) -> usize {
        self.folded.capacity()
            + self
                .source
                .as_ref()
                .map_or(0, |source| source.capacity() * std::mem::size_of::<usize>())
    }

    pub fn reservation(text: &str) -> usize {
        if text.is_ascii() {
            text.len()
        } else {
            // Unicode folding expands by at most 3×; growing vectors can reserve 4×.
            text.len() * 4 * (1 + std::mem::size_of::<usize>())
        }
    }
}
