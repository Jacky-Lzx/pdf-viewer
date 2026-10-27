use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, Name, Object, String as PdfString},
};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;

#[derive(Clone, Copy, Debug)]
enum Style {
    Decimal,
    Roman,
    LowerRoman,
    Letters,
    LowerLetters,
    None,
}

#[derive(Clone, Debug)]
struct LabelRange {
    page: usize,
    first: u64,
    prefix: String,
    style: Style,
}

#[derive(Clone, Debug, Default)]
pub struct PageLabels {
    ranges: Vec<LabelRange>,
    count: usize,
}

impl PageLabels {
    pub fn read(pdf: &Pdf) -> Self {
        let count = pdf.pages().len();
        let root = pdf
            .xref()
            .get::<Dict<'_>>(pdf.xref().root_id())
            .and_then(|catalog| catalog.get::<Dict<'_>>(b"PageLabels"));
        let mut stack: Vec<_> = root.into_iter().map(|node| (node, 0)).collect();
        let mut seen = HashSet::new();
        let mut ranges = BTreeMap::new();
        while let Some((node, depth)) = stack.pop() {
            if depth > 32
                || seen.len() >= 10_000
                || ranges.len() >= 10_000
                || !seen.insert(node.data().as_ptr() as usize)
            {
                continue;
            }
            if let Some(nums) = node.get::<Array<'_>>(b"Nums") {
                let mut items = nums.iter::<Object<'_>>().take(20_000);
                while let (Some(index), Some(value)) = (items.next(), items.next()) {
                    let Object::Dict(label) = value else {
                        continue;
                    };
                    let Some(page) = index
                        .into_i32()
                        .filter(|page| *page >= 0 && (*page as usize) < count)
                        .map(|page| page as usize)
                    else {
                        continue;
                    };
                    let style = match label
                        .get::<Name<'_>>(b"S")
                        .as_ref()
                        .map(|name| name.as_ref())
                    {
                        Some(b"D") => Style::Decimal,
                        Some(b"R") => Style::Roman,
                        Some(b"r") => Style::LowerRoman,
                        Some(b"A") => Style::Letters,
                        Some(b"a") => Style::LowerLetters,
                        _ => Style::None,
                    };
                    let prefix = label
                        .get::<PdfString<'_>>(b"P")
                        .map(|value| {
                            let bytes = value.as_bytes();
                            crate::pdf_string::decode(&bytes[..bytes.len().min(1024)])
                                .chars()
                                .take(256)
                                .collect()
                        })
                        .unwrap_or_default();
                    let first = label.get::<u32>(b"St").unwrap_or(1).max(1) as u64;
                    ranges.insert(
                        page,
                        LabelRange {
                            page,
                            first,
                            prefix,
                            style,
                        },
                    );
                    if ranges.len() >= 10_000 {
                        break;
                    }
                }
            }
            if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
                let available = 10_000_usize.saturating_sub(stack.len());
                stack.extend(
                    crate::pdf_objects::dictionaries(pdf, &kids, available)
                        .map(|kid| (kid, depth + 1)),
                );
            }
        }
        Self {
            ranges: ranges.into_values().collect(),
            count,
        }
    }

    pub fn label(&self, page: usize) -> Option<String> {
        if page >= self.count {
            return None;
        }
        let index = self
            .ranges
            .partition_point(|range| range.page <= page)
            .checked_sub(1)?;
        let range = &self.ranges[index];
        let suffix = number(range.style, range.first + (page - range.page) as u64);
        if range.prefix.is_empty() {
            return Some(suffix);
        }
        Some(format!("{}{}", range.prefix, suffix))
    }

    pub fn display_name(&self, page: usize) -> String {
        let physical = (page + 1).to_string();
        match self.label(page) {
            Some(mut label) if !label.is_empty() && label != physical => {
                // Only inspect enough characters to decide whether to truncate.
                // Reuse the label allocation for both its prefix and page number.
                if let Some((end, _)) = label.char_indices().nth(12) {
                    label.truncate(end);
                    label.push('…');
                }
                let _ = write!(label, " · {}", page + 1);
                label
            }
            _ => physical,
        }
    }

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

fn number(style: Style, value: u64) -> String {
    match style {
        Style::Decimal => value.to_string(),
        Style::None => String::new(),
        Style::Roman | Style::LowerRoman if value <= 39_999 => {
            let mut remaining = value;
            let mut result = String::new();
            for (amount, symbol) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while remaining >= amount {
                    result.push_str(symbol);
                    remaining -= amount;
                }
            }
            if matches!(style, Style::LowerRoman) {
                result.make_ascii_lowercase();
            }
            result
        }
        Style::Letters | Style::LowerLetters if (1..=832).contains(&value) => {
            // PDF labels repeat a letter: A…Z, AA…ZZ, AAA…ZZZ.
            let base = if matches!(style, Style::LowerLetters) {
                b'a'
            } else {
                b'A'
            };
            std::iter::repeat_n(
                (base + ((value - 1) % 26) as u8) as char,
                value.div_ceil(26) as usize,
            )
            .collect()
        }
        _ => value.to_string(),
    }
}

fn parse_number(style: Style, value: &str, first: u64) -> Option<u64> {
    match style {
        Style::None => value.is_empty().then_some(first),
        Style::Decimal => value.parse().ok(),
        Style::Roman | Style::LowerRoman => {
            if let Ok(decimal) = value.parse::<u64>() {
                return (decimal > 39_999).then_some(decimal);
            }
            if value.len() > 64 || value.is_empty() {
                return None;
            }
            let mut total: i64 = 0;
            let mut previous = 0;
            for letter in value.bytes().rev() {
                let amount = match letter.to_ascii_uppercase() {
                    b'I' => 1,
                    b'V' => 5,
                    b'X' => 10,
                    b'L' => 50,
                    b'C' => 100,
                    b'D' => 500,
                    b'M' => 1000,
                    _ => return None,
                };
                total += if amount < previous { -amount } else { amount };
                previous = amount;
            }
            u64::try_from(total).ok().filter(|value| *value > 0)
        }
        Style::Letters | Style::LowerLetters => {
            if let Ok(decimal) = value.parse::<u64>() {
                return (decimal > 832).then_some(decimal);
            }
            let letter = value.as_bytes().first()?.to_ascii_uppercase();
            if !letter.is_ascii_uppercase()
                || value.len() > 32
                || !value.bytes().all(|ch| ch.to_ascii_uppercase() == letter)
            {
                return None;
            }
            Some((value.len() as u64 - 1) * 26 + u64::from(letter - b'A') + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats_follow_pdf_conventions_and_bound_extreme_values() {
        assert_eq!(number(Style::LowerRoman, 49), "xlix");
        assert_eq!(number(Style::Roman, 1999), "MCMXCIX");
        assert_eq!(number(Style::Letters, 27), "AA");
        assert_eq!(number(Style::Letters, 28), "BB");
        assert_eq!(number(Style::LowerLetters, 53), "aaa");
        assert!(number(Style::Roman, u64::MAX).len() <= 20);
        assert!(number(Style::Letters, u64::MAX).len() <= 20);
    }

    #[test]
    fn real_number_tree_resolves_labels_and_malformed_cycles_terminate() {
        let bytes = include_bytes!("../assets/folio-guide.pdf");
        let pdf = Pdf::new(bytes.to_vec()).unwrap();
        let labels = PageLabels::read(&pdf);
        assert_eq!(
            (0..6)
                .map(|page| labels.label(page).unwrap())
                .collect::<Vec<_>>(),
            ["i", "ii", "1", "2", "A-1", "A-2"]
        );
        assert_eq!(labels.resolve_input("A-2", 6), Some(5));
        assert_eq!(labels.resolve_input("=1", 6), Some(2));
        assert_eq!(
            (0..6)
                .map(|page| labels.display_name(page))
                .collect::<Vec<_>>(),
            ["i · 1", "ii · 2", "1 · 3", "2 · 4", "A-1 · 5", "A-2 · 6"]
        );
        let mut modified = bytes.to_vec();
        let kids = b"/Kids [25 0 R 26 0 R]";
        let offset = modified
            .windows(kids.len())
            .position(|value| value == kids)
            .unwrap();
        modified[offset..offset + kids.len()].copy_from_slice(b"/Kids [24 0 R 24 0 R]");
        let pdf = Pdf::new(modified).unwrap();
        assert!(PageLabels::read(&pdf).ranges.is_empty());
    }

    #[test]
    fn invalid_number_tree_children_do_not_hide_later_label_ranges() {
        let source = include_bytes!("../assets/folio-guide.pdf");
        let marker = b"/Kids [25 0 R 26 0 R]";
        for invalid in ["null", "0", "false", "()", "[]", "/Bad", "999 0 R"] {
            let mut bytes = source.to_vec();
            let offset = bytes
                .windows(marker.len())
                .position(|value| value == marker)
                .unwrap();
            let replacement = format!("/Kids [{invalid:<7}26 0 R]");
            assert_eq!(replacement.len(), marker.len());
            bytes[offset..offset + marker.len()].copy_from_slice(replacement.as_bytes());
            let pdf = Pdf::new(bytes).unwrap();
            let labels = PageLabels::read(&pdf);
            assert_eq!(labels.label(4).as_deref(), Some("A-1"), "{invalid}");
            assert_eq!(labels.resolve_input("A-2", 6), Some(5), "{invalid}");
        }
    }

    #[test]
    fn label_lookup_preserves_physical_numbers_and_supports_explicit_labels() {
        let labels = PageLabels {
            count: 8,
            ranges: vec![
                LabelRange {
                    page: 0,
                    first: 1,
                    prefix: String::new(),
                    style: Style::LowerRoman,
                },
                LabelRange {
                    page: 2,
                    first: 1,
                    prefix: String::new(),
                    style: Style::Decimal,
                },
                LabelRange {
                    page: 4,
                    first: 27,
                    prefix: "附录-".into(),
                    style: Style::Letters,
                },
            ],
        };
        assert_eq!(labels.resolve_input("ii", 8), Some(1));
        assert_eq!(labels.resolve_input("II", 8), Some(1));
        assert_eq!(labels.resolve_input("1", 8), Some(0));
        assert_eq!(labels.resolve_input("=1", 8), Some(2));
        assert_eq!(labels.resolve_input("附录-BB", 8), Some(5));
        assert_eq!(labels.resolve_input("=01", 8), None);
        assert_eq!(labels.resolve_input("iiii", 8), None);
        assert_eq!(labels.resolve_input("99", 8), None);
        assert_eq!(PageLabels::default().resolve_input("2", 8), Some(1));
    }

    #[test]
    fn unicode_label_lookup_handles_expansion_and_prefers_later_exact_labels() {
        let labels = PageLabels {
            count: 12,
            ranges: [
                (0, 1, "Straße-", Style::Decimal),
                (2, 1, "STRASSE-", Style::Decimal),
                (4, 1, "ΟΣ-", Style::LowerRoman),
                (6, 3, "ος-", Style::LowerRoman),
                (8, 27, "附录-", Style::Letters),
                (10, 1, "é-", Style::None),
            ]
            .into_iter()
            .map(|(page, first, prefix, style)| LabelRange {
                page,
                first,
                prefix: prefix.into(),
                style,
            })
            .collect(),
        };
        for (input, page) in [
            ("strasse-1", 0),
            ("Straße-2", 1),
            ("STRASSE-1", 2),
            ("Ος-I", 4),
            ("οσ-iii", 6),
            ("附录-bb", 9),
            ("É-", 10),
        ] {
            assert_eq!(labels.resolve_input(input, 12), Some(page), "{input}");
        }
        for input in ["strasse-01", "strasse-3", "οσ-iiii", "É-1", "e\u{301}-"] {
            assert_eq!(labels.resolve_input(input, 12), None, "{input}");
        }
        let letters = PageLabels {
            count: 1,
            ranges: vec![LabelRange {
                page: 0,
                first: 19,
                prefix: String::new(),
                style: Style::Letters,
            }],
        };
        assert_eq!(letters.resolve_input("ſ", 1), Some(0));
    }

    #[test]
    fn display_names_truncate_at_unicode_boundaries_without_changing_full_labels() {
        for (label, displayed) in [
            ("", "1"),
            ("1", "1"),
            ("123456789012", "123456789012 · 1"),
            ("1234567890123", "123456789012… · 1"),
            (
                "中文😀".repeat(100).as_str(),
                "中文😀中文😀中文😀中文😀… · 1",
            ),
        ] {
            let labels = PageLabels {
                count: 2,
                ranges: vec![LabelRange {
                    page: 0,
                    first: 1,
                    prefix: label.to_owned(),
                    style: Style::None,
                }],
            };
            assert_eq!(labels.display_name(0), displayed);
            assert_eq!(labels.label(0).as_deref(), Some(label));
        }
        assert_eq!(PageLabels::default().display_name(4), "5");
    }
}
