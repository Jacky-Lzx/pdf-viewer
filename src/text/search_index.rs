use caseless::Caseless;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct SearchIndex {
    pub folded: String,
    // Keep direct offsets until folding changes a source character boundary.
    pub source: Option<SourceMap>,
}

#[derive(Clone, Debug)]
pub(super) struct SourceMap {
    pub start: usize,
    pub offsets: Vec<usize>,
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
        let mut source: Option<SourceMap> = None;
        for (offset, ch) in text.char_indices() {
            if ch.is_ascii() {
                folded.push(ch.to_ascii_lowercase());
                if let Some(source) = &mut source {
                    source.offsets.push(offset);
                }
                continue;
            }
            let start = folded.len();
            let mut characters = 0;
            for lower in std::iter::once(ch).default_case_fold() {
                folded.push(lower);
                characters += 1;
            }
            let bytes = folded.len() - start;
            if source.is_none() && (characters != 1 || bytes != ch.len_utf8()) {
                source = Some(SourceMap {
                    start,
                    offsets: Vec::with_capacity(text.len() - offset),
                });
            }
            if let Some(source) = &mut source {
                source.offsets.extend(std::iter::repeat_n(offset, bytes));
            }
        }
        Self { folded, source }
    }

    pub fn source_range(&self, text: &str, range: Range<usize>) -> Range<usize> {
        let Some(source) = &self.source else {
            return range;
        };
        if range.end <= source.start {
            return range;
        }
        let start = if range.start < source.start {
            range.start
        } else {
            source.offsets[range.start - source.start]
        };
        let last = source.offsets[range.end - 1 - source.start];
        start..last + text[last..].chars().next().unwrap().len_utf8()
    }

    pub fn partial_expansion(&self, range: Range<usize>) -> bool {
        self.source.as_ref().is_some_and(|source| {
            let partial_at = |offset: usize| {
                offset > source.start
                    && offset - source.start < source.offsets.len()
                    && source.offsets[offset - source.start - 1]
                        == source.offsets[offset - source.start]
            };
            partial_at(range.start) || partial_at(range.end)
        })
    }

    pub fn memory_bytes(&self) -> usize {
        self.folded.capacity()
            + self.source.as_ref().map_or(0, |source| {
                source.offsets.capacity() * std::mem::size_of::<usize>()
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_expansion_preserves_direct_prefix_and_crossing_unicode_ranges() {
        let prefix = "中文 Σσς École 👩‍💻 ".repeat(100);
        for changed in ["ß", "ﬃ", "İ", "Ⱥ", "ſ", "K", "ΐ"] {
            let text = format!("{prefix}{changed}尾中文 Rust");
            let index = SearchIndex::new(&text);
            let map = index.source.as_ref().unwrap();
            assert_eq!(map.start, prefix.len());
            assert_eq!(map.offsets.len(), index.folded.len() - prefix.len());
            let reference: Vec<_> = text
                .char_indices()
                .flat_map(|(offset, ch)| {
                    std::iter::once(ch)
                        .default_case_fold()
                        .flat_map(move |lower| std::iter::repeat_n(offset, lower.len_utf8()))
                })
                .collect();
            assert_eq!(index.folded, caseless::default_case_fold_str(&text));
            assert!(index.memory_bytes() <= SearchIndex::reservation(&text));
            for (start, ch) in index.folded.char_indices() {
                for end in [start + ch.len_utf8(), index.folded.len()] {
                    let last = reference[end - 1];
                    let expected =
                        reference[start]..last + text[last..].chars().next().unwrap().len_utf8();
                    assert_eq!(index.source_range(&text, start..end), expected);
                    let partial = (start > 0 && reference[start - 1] == reference[start])
                        || (end < reference.len() && reference[end - 1] == reference[end]);
                    assert_eq!(index.partial_expansion(start..end), partial);
                }
            }
        }
    }
}
