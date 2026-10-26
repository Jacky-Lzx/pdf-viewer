use std::{ops::Range, sync::OnceLock};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn line_range(text: &str, offset: usize) -> Range<usize> {
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index);
    start..end
}

pub(super) fn word_range(text: &str, offset: usize) -> Range<usize> {
    let line = line_range(text, offset);
    text[line.clone()]
        .split_word_bound_indices()
        .find_map(|(start, word)| {
            let range = line.start + start..line.start + start + word.len();
            range.contains(&offset).then_some(range)
        })
        .unwrap_or(offset..offset)
}

/// One bit per source byte; membership does not depend on the number of words.
#[derive(Clone, Debug)]
pub(super) struct WordBoundaries {
    bits: Vec<u64>,
    line_breaks: OnceLock<Vec<usize>>,
}

impl WordBoundaries {
    pub fn new(text: &str) -> Self {
        let mut bits = vec![0; (text.len() + 1).div_ceil(64)];
        for offset in text
            .split_word_bound_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
        {
            bits[offset / 64] |= 1 << (offset % 64);
        }
        Self {
            bits,
            line_breaks: OnceLock::new(),
        }
    }

    pub fn contains(&self, offset: usize) -> bool {
        self.bits
            .get(offset / 64)
            .is_some_and(|bits| bits & (1 << (offset % 64)) != 0)
    }

    fn containing_range(&self, offset: usize) -> Option<Range<usize>> {
        let mut block = offset / 64;
        let mut bits = *self.bits.get(block)? & (u64::MAX >> (63 - offset % 64));
        let start = loop {
            if bits != 0 {
                break block * 64 + 63 - bits.leading_zeros() as usize;
            }
            block = block.checked_sub(1)?;
            bits = self.bits[block];
        };
        let next = offset.checked_add(1)?;
        block = next / 64;
        bits = *self.bits.get(block)? & (u64::MAX << (next % 64));
        let end = loop {
            if bits != 0 {
                break block * 64 + bits.trailing_zeros() as usize;
            }
            block += 1;
            bits = *self.bits.get(block)?;
        };
        Some(start..end)
    }

    pub fn line_range(&self, text: &str, offset: usize) -> Range<usize> {
        self.line_breaks.get().map_or_else(
            || line_range(text, offset),
            |breaks| {
                let next = breaks.partition_point(|&position| position < offset);
                let start = next
                    .checked_sub(1)
                    .map_or(0, |previous| breaks[previous] + 1);
                start..breaks.get(next).copied().unwrap_or(text.len())
            },
        )
    }

    pub fn word_range(&self, text: &str, offset: usize) -> Range<usize> {
        let line = self.line_range(text, offset);
        if offset == line.end {
            return offset..offset;
        }
        self.containing_range(offset)
            .map_or(offset..offset, |range| {
                // Keep the original per-line behavior, including CRLF boundaries.
                range.start.max(line.start)..range.end.min(line.end)
            })
    }

    pub fn prepare_lines(&self, text: &str) {
        self.line_breaks
            .get_or_init(|| text.match_indices('\n').map(|(offset, _)| offset).collect());
    }

    pub fn memory_bytes(&self) -> usize {
        self.bits.capacity() * std::mem::size_of::<u64>()
            + self
                .line_breaks
                .get()
                .map_or(0, |breaks| breaks.capacity() * std::mem::size_of::<usize>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_ranges_match_scanning_at_unicode_and_machine_word_boundaries() {
        for text in [
            String::new(),
            "a".repeat(10000),
            "a ".repeat(1000),
            "can't e\u{301} 中文 👩‍💻 \r\n\nnext\rword\n \u{200d}ß\u{2028}ΟΣ!".repeat(8),
            "\u{301}a\n\u{301}b\r\n\u{200d}c\n🇨🇳\n🇨🇳\r\n".into(),
            format!("{}\n{}", "a".repeat(63), "b".repeat(128)),
        ] {
            let index = WordBoundaries::new(&text);
            for prepared in [false, true] {
                if prepared {
                    index.prepare_lines(&text);
                }
                for offset in text
                    .char_indices()
                    .map(|(offset, _)| offset)
                    .filter(|&offset| {
                        text.len() <= 2000
                            || offset % 97 == 0
                            || matches!(offset, 1 | 63 | 64 | 65 | 127 | 128 | 129)
                            || offset + 2 >= text.len()
                    })
                    .chain(std::iter::once(text.len()))
                {
                    assert_eq!(
                        index.word_range(&text, offset),
                        word_range(&text, offset),
                        "{offset}"
                    );
                    assert_eq!(index.line_range(&text, offset), line_range(&text, offset));
                }
            }
            assert!(index.containing_range(text.len()).is_none());
            assert!(index.containing_range(usize::MAX).is_none());
        }
    }

    #[test]
    fn line_table_is_lazy_reused_and_counted_by_actual_capacity() {
        let text = "first\r\nsecond\n\nlast\n";
        let index = WordBoundaries::new(text);
        let before = index.memory_bytes();
        assert!(index.line_breaks.get().is_none());
        index.word_range(text, 2);
        assert!(
            index.line_breaks.get().is_none(),
            "Queries must not allocate on the UI thread"
        );
        index.prepare_lines(text);
        let lines = index.line_breaks.get().unwrap();
        assert_eq!(lines, &[6, 13, 14, 19]);
        assert_eq!(
            index.memory_bytes(),
            before + lines.capacity() * std::mem::size_of::<usize>()
        );
        let pointer = lines.as_ptr();
        index.prepare_lines(text);
        assert_eq!(index.line_breaks.get().unwrap().as_ptr(), pointer);
    }

    #[test]
    fn bitset_matches_unicode_boundaries_at_every_byte_including_word_edges() {
        for text in [
            String::new(),
            "a".repeat(63),
            "a ".repeat(64),
            "don't e\u{301} 中文 Straße 👩‍💻".repeat(100),
        ] {
            let boundaries = WordBoundaries::new(&text);
            let expected: Vec<_> = text
                .split_word_bound_indices()
                .map(|(offset, _)| offset)
                .chain(std::iter::once(text.len()))
                .collect();
            for offset in 0..=text.len() + 65 {
                assert_eq!(
                    boundaries.contains(offset),
                    expected.contains(&offset),
                    "{offset}"
                );
            }
            assert_eq!(boundaries.memory_bytes(), (text.len() + 1).div_ceil(64) * 8);
        }
    }
}
