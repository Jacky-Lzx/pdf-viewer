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
        // A cheap block scan selects a constructor with no per-character bulk
        // checks for short or densely mixed Unicode text. Missing an unaligned
        // short run only loses an optimization; it never changes the folding.
        if text
            .as_bytes()
            .as_chunks::<64>()
            .0
            .iter()
            .any(|block| block.is_ascii())
        {
            Self::mixed::<true>(text)
        } else {
            Self::mixed::<false>(text)
        }
    }

    fn mixed<const BULK_ASCII: bool>(text: &str) -> Self {
        let mut folded = String::with_capacity(text.len());
        let mut source: Option<SourceMap> = None;
        let mut chars = text.char_indices();
        let mut base = 0;
        let mut check_from = 0;
        while let Some((index, ch)) = chars.next() {
            let offset = base + index;
            if ch.is_ascii() {
                // Avoid probing every byte in short mixed-language runs.
                if BULK_ASCII && offset >= check_from {
                    check_from = offset + 64;
                    let remaining = &text[offset..];
                    if remaining.as_bytes().get(..64).is_some_and(<[u8]>::is_ascii) {
                        let bytes = 64 + ascii_prefix_len(&remaining.as_bytes()[64..]);
                        let start = folded.len();
                        folded.push_str(&remaining[..bytes]);
                        folded[start..].make_ascii_lowercase();
                        base = offset + bytes;
                        chars = text[base..].char_indices();
                        check_from = base;
                        continue;
                    }
                }
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

/// Scan full ASCII blocks before locating the next UTF-8 lead byte. ASCII
/// boundaries are always UTF-8 boundaries, so the returned prefix can be copied
/// and lowercased together without a temporary string or source-map entries.
fn ascii_prefix_len(bytes: &[u8]) -> usize {
    let mut length = 0;
    for block in bytes.chunks(64) {
        if !block.is_ascii() {
            return length + block.iter().position(|byte| !byte.is_ascii()).unwrap();
        }
        length += block.len();
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_reference_ranges(text: &str) {
        let index = SearchIndex::new(text);
        let reference: Vec<_> = text
            .char_indices()
            .flat_map(|(offset, ch)| {
                std::iter::once(ch)
                    .default_case_fold()
                    .flat_map(move |lower| std::iter::repeat_n(offset, lower.len_utf8()))
            })
            .collect();
        assert_eq!(index.folded, caseless::default_case_fold_str(text));
        let boundaries: Vec<_> = index
            .folded
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(index.folded.len()))
            .collect();
        for (position, &start) in boundaries.iter().enumerate() {
            for &end in &boundaries[position + 1..] {
                let last = reference[end - 1];
                let expected =
                    reference[start]..last + text[last..].chars().next().unwrap().len_utf8();
                assert_eq!(
                    index.source_range(text, start..end),
                    expected,
                    "{text:?}, {start}..{end}"
                );
                let partial = (start > 0 && reference[start - 1] == reference[start])
                    || (end < reference.len() && reference[end - 1] == reference[end]);
                assert_eq!(
                    index.partial_expansion(start..end),
                    partial,
                    "{text:?}, {start}..{end}"
                );
            }
        }
        assert!(index.memory_bytes() <= SearchIndex::reservation(text));
    }

    #[test]
    fn direct_suffixes_and_deferred_gaps_preserve_all_unicode_ranges() {
        for first in ["ß", "ﬃ", "İ", "Ⱥ", "ſ", "K", "ΐ"] {
            for second in ["", "ß", "ﬃ", "İ", "Ⱥ", "ſ", "K", "ΐ"] {
                for gap in ["", " ASCII\r\n_0123 ", " 中ÉΣσς 👩‍💻 e\u{301}\n"] {
                    for suffix in ["", " tail ASCII", "尾中文 Σσς 👩‍💻"] {
                        assert_reference_ranges(&format!("前 É {first}{gap}{second}{suffix}"));
                    }
                }
            }
        }
    }

    #[test]
    fn ascii_run_boundaries_keep_unicode_expansions_and_source_ranges() {
        for length in [0, 1, 31, 32, 63, 64, 65, 127, 128, 129] {
            let ascii = "Az_09".repeat(length / 5) + &"Az_09"[..length % 5];
            for unicode in ["ß", "İ", "ﬃ", "Ⱥ", "K", "中", "👩‍💻", "É"] {
                assert_reference_ranges(&format!("{ascii}{unicode}{ascii}ß尾"));
                assert_reference_ranges(&format!("İ{ascii}{unicode}{ascii}"));
            }
        }
    }

    #[test]
    fn unchanged_suffixes_do_not_grow_the_source_map() {
        for suffix in [" Rust PDF reader\n", " 中文 Σσς École 👩‍💻\n"] {
            for repeats in [0, 1, 100, 10_000] {
                let text = format!("前 ßİ{}", suffix.repeat(repeats));
                let index = SearchIndex::new(&text);
                let source = index.source.as_ref().unwrap();
                assert_eq!(source.start, "前 ".len());
                assert_eq!(source.end, "前 ßİ".len());
                assert_eq!(source.offsets.len(), "ssi\u{307}".len());
                assert!(
                    index.memory_bytes() - index.folded.capacity()
                        <= 8 * std::mem::size_of::<usize>()
                );
                assert_eq!(
                    index.source_range(&text, 0..index.folded.len()),
                    0..text.len()
                );
                if repeats > 0 {
                    let end = index.folded.len();
                    assert_eq!(
                        index.source_range(&text, end - suffix.len()..end),
                        text.len() - suffix.len()..text.len()
                    );
                }
            }
        }
    }

    #[test]
    fn late_expansion_preserves_direct_prefix_and_crossing_unicode_ranges() {
        let prefix = "中文 Σσς École 👩‍💻 ".repeat(100);
        for changed in ["ß", "ﬃ", "İ", "Ⱥ", "ſ", "K", "ΐ"] {
            let text = format!("{prefix}{changed}尾中文 Rust");
            let index = SearchIndex::new(&text);
            let map = index.source.as_ref().unwrap();
            assert_eq!(map.start, prefix.len());
            assert_eq!(map.end, prefix.len() + changed.len());
            assert_eq!(
                map.offsets.len(),
                index.folded.len() - prefix.len() - "尾中文 Rust".len()
            );
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
