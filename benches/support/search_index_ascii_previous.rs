// Frozen from 25f1e76 for mixed-text index construction comparisons.
use caseless::Caseless;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct SearchIndex {
    pub folded: String,
    // Only the interval between the first and last changed character needs a
    // byte map. The prefix and suffix retain direct (possibly shifted) offsets.
    pub source: Option<SourceMap>,
}

#[derive(Clone, Debug)]
pub(super) struct SourceMap {
    /// Folded byte offset at the beginning of the mapped interval.
    pub start: usize,
    pub offsets: Vec<usize>,
    /// Source byte offset immediately after the mapped interval.
    pub end: usize,
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
                continue;
            }
            let start = folded.len();
            let mut characters = 0;
            for lower in std::iter::once(ch).default_case_fold() {
                folded.push(lower);
                characters += 1;
            }
            let bytes = folded.len() - start;
            if characters != 1 || bytes != ch.len_utf8() {
                let source = source.get_or_insert_with(|| SourceMap {
                    start,
                    offsets: Vec::new(),
                    end: offset,
                });
                // Unchanged characters after the last exception may be the
                // direct suffix. Materialize them only if another exception
                // makes them part of the mapped interval.
                let gap = &text[source.end..offset];
                source.offsets.reserve(gap.len() + bytes);
                if gap.is_ascii() {
                    source.offsets.extend(source.end..offset);
                } else {
                    for (relative, ch) in gap.char_indices() {
                        source
                            .offsets
                            .extend(std::iter::repeat_n(source.end + relative, ch.len_utf8()));
                    }
                }
                source.offsets.extend(std::iter::repeat_n(offset, bytes));
                source.end = offset + ch.len_utf8();
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
        let mapped_end = source.start + source.offsets.len();
        let start = if range.start < source.start {
            range.start
        } else if range.start >= mapped_end {
            source.end + (range.start - mapped_end)
        } else {
            source.offsets[range.start - source.start]
        };
        let end = if range.end >= mapped_end {
            source.end + (range.end - mapped_end)
        } else {
            let last = source.offsets[range.end - 1 - source.start];
            last + text[last..].chars().next().unwrap().len_utf8()
        };
        start..end
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
