// Frozen lookup methods from 0058ab9. The wrapper borrows the same prepared
// PageLabels snapshot; number parsing and label formatting remain unchanged.
use super::{PageLabels, parse_number};

pub struct Previous<'a>(pub &'a PageLabels);

impl std::ops::Deref for Previous<'_> {
    type Target = PageLabels;
    fn deref(&self) -> &Self::Target { self.0 }
}

impl Previous<'_> {
    /// Physical page numbers remain stable; `=1` explicitly addresses a numeric PDF label.
    pub fn resolve_input(&self, input: &str, count: usize) -> Option<usize> {
        let input = input.trim();
        if let Some(label) = input.strip_prefix('=') {
            return self.find(label.trim());
        }
        if let Ok(number) = input.parse::<usize>() {
            return number.checked_sub(1).filter(|page| *page < count);
        }
        self.find(input)
    }

    fn find(&self, label: &str) -> Option<usize> {
        if label.is_empty() {
            return None;
        }
        // Candidate lookup walks label ranges, not every page in a long PDF.
        let mut folded = None;
        let mut folded_label = None;
        let ascii = label.is_ascii();
        for (index, range) in self.ranges.iter().enumerate() {
            if label
                .get(..range.prefix.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&range.prefix))
                && let Some((page, candidate)) = self.candidate(index, &label[range.prefix.len()..])
            {
                if candidate == label {
                    return Some(page);
                }
                if folded.is_none() && candidate.eq_ignore_ascii_case(label) {
                    folded = Some(page);
                }
            }
            // Only non-ASCII lookup needs Unicode folding. Keep checking later
            // exact labels even after finding the earliest folded candidate.
            if folded.is_none() && (!ascii || !range.prefix.is_ascii()) {
                let normalized =
                    folded_label.get_or_insert_with(|| caseless::default_case_fold_str(label));
                let prefix = caseless::default_case_fold_str(&range.prefix);
                if let Some(suffix) = normalized.strip_prefix(&prefix)
                    && let Some((page, candidate)) = self.candidate(index, suffix)
                    && caseless::default_case_fold_str(&candidate) == *normalized
                {
                    folded = Some(page);
                }
            }
        }
        folded
    }

    fn candidate(&self, index: usize, suffix: &str) -> Option<(usize, String)> {
        let range = &self.ranges[index];
        let value = parse_number(range.style, suffix, range.first)?;
        let offset = usize::try_from(value.checked_sub(range.first)?).ok()?;
        let page = range.page.checked_add(offset)?;
        let end = self
            .ranges
            .get(index + 1)
            .map_or(self.count, |next| next.page);
        if page >= end {
            return None;
        }
        Some((page, self.label(page)?))
    }
}
