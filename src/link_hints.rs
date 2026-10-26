use std::ops::Range;

pub const MAX_LINKS: usize = 1000;

fn digits(count: usize) -> usize {
    if count > 676 {
        3
    } else if count > 26 {
        2
    } else {
        1
    }
}

pub fn label(mut index: usize, count: usize) -> String {
    let mut label = vec![b'a'; digits(count)];
    for position in (0..label.len()).rev() {
        label[position] += (index % 26) as u8;
        index /= 26;
    }
    String::from_utf8(label).expect("ASCII link hint")
}

#[derive(Default)]
pub struct LinkHints {
    pub active: bool,
    pub prefix: String,
    count: usize,
}

impl LinkHints {
    pub fn start(&mut self, count: usize) {
        self.count = count.min(MAX_LINKS);
        self.active = self.count > 0;
        self.prefix.clear();
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.prefix.clear();
    }

    pub fn matching_range(&self) -> Range<usize> {
        if !self.active || self.prefix.len() > digits(self.count) {
            return 0..0;
        }
        let mut index = 0;
        for byte in self.prefix.bytes() {
            if !byte.is_ascii_lowercase() {
                return 0..0;
            }
            index = index * 26 + usize::from(byte - b'a');
        }
        let suffixes = 26usize.pow((digits(self.count) - self.prefix.len()) as u32);
        let start = (index * suffixes).min(self.count);
        start..(start + suffixes).min(self.count)
    }

    pub fn press(&mut self, letter: char) -> Option<usize> {
        if !self.active || !letter.is_ascii_lowercase() {
            return None;
        }
        self.prefix.push(letter);
        let matching = self.matching_range();
        if matching.is_empty() {
            self.cancel();
        } else if self.prefix.len() == digits(self.count) {
            self.cancel();
            return Some(matching.start);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels_roundtrip_every_link_and_prefixes_select_exactly_the_matching_labels() {
        use super::*;
        for count in [1, 26, 27, 676, 677, MAX_LINKS] {
            let labels: Vec<_> = (0..count).map(|index| label(index, count)).collect();
            let unique: std::collections::HashSet<_> = labels.iter().collect();
            assert_eq!(unique.len(), count);
            for (index, label) in labels.iter().enumerate() {
                let mut hints = LinkHints::default();
                hints.start(count);
                for (position, letter) in label.chars().enumerate() {
                    let selected = hints.press(letter);
                    if position + 1 == label.len() {
                        assert_eq!(selected, Some(index));
                        assert!(!hints.active);
                    } else {
                        assert_eq!(selected, None);
                        let expected: Vec<_> = labels
                            .iter()
                            .enumerate()
                            .filter(|(_, value)| value.starts_with(&hints.prefix))
                            .map(|(index, _)| index)
                            .collect();
                        assert_eq!(hints.matching_range().collect::<Vec<_>>(), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn partial_final_group_backspace_invalid_prefixes_and_empty_pages_are_safe() {
        use super::*;
        let mut hints = LinkHints::default();
        hints.start(27);
        assert_eq!(hints.press('b'), None);
        assert_eq!(hints.matching_range(), 26..27);
        hints.prefix.pop();
        assert_eq!(hints.matching_range(), 0..27);
        hints.press('b');
        assert_eq!(hints.press('b'), None);
        assert!(!hints.active);
        hints.start(1000);
        assert_eq!(hints.press('b'), None);
        assert_eq!(hints.press('m'), None);
        assert_eq!(hints.matching_range(), 988..1000);
        assert_eq!(hints.press('l'), Some(999));
        hints.start(0);
        assert!(!hints.active);
        assert!(hints.matching_range().is_empty());
        hints.start(usize::MAX);
        assert_eq!(hints.matching_range(), 0..MAX_LINKS);
        assert_eq!(hints.press('界'), None);
        assert_eq!(hints.matching_range(), 0..MAX_LINKS);
        hints.cancel();
        assert!(hints.matching_range().is_empty());
    }
}
