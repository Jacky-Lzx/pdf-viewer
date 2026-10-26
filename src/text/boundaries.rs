use unicode_segmentation::UnicodeSegmentation;

/// One bit per source byte; membership does not depend on the number of words.
#[derive(Clone, Debug)]
pub(super) struct WordBoundaries {
    bits: Vec<u64>,
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
        Self { bits }
    }

    pub fn contains(&self, offset: usize) -> bool {
        self.bits
            .get(offset / 64)
            .is_some_and(|bits| bits & (1 << (offset % 64)) != 0)
    }

    pub fn memory_bytes(&self) -> usize {
        self.bits.capacity() * std::mem::size_of::<u64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
