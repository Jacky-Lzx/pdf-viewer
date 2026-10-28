use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, Name, Object, String as PdfString},
};
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write,
};

const LABEL_EXCERPT_CHARACTERS: usize = 12;

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

struct LabelExcerpt<'a> {
    prefix: &'a str,
    suffix: String,
    truncated: bool,
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
        let mut budget = crate::pdf_objects::ScanBudget::default();
        while let Some((node, depth)) = stack.pop() {
            if budget.is_empty() {
                break;
            }
            if depth > 32
                || seen.len() >= 10_000
                || ranges.len() >= 10_000
                || !seen.insert(node.data().as_ptr() as usize)
            {
                continue;
            }
            if let Some(nums) = node.get::<Array<'_>>(b"Nums") {
                for (index, value) in budget.pairs(pdf, &nums, 10_000) {
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
                            crate::pdf_string::decode_prefix(&bytes[..bytes.len().min(1024)], 256).0
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
                    budget
                        .dictionaries(pdf, &kids, available)
                        .map(|kid| (kid, depth + 1)),
                );
            }
        }
        Self {
            ranges: ranges.into_values().collect(),
            count,
        }
    }

    fn range(&self, page: usize) -> Option<&LabelRange> {
        if page >= self.count {
            return None;
        }
        let index = self
            .ranges
            .partition_point(|range| range.page <= page)
            .checked_sub(1)?;
        Some(&self.ranges[index])
    }

    pub fn label(&self, page: usize) -> Option<String> {
        let range = self.range(page)?;
        let suffix = number(range.style, range.first + (page - range.page) as u64);
        if range.prefix.is_empty() {
            return Some(suffix);
        }
        Some(format!("{}{}", range.prefix, suffix))
    }

    pub fn display_name(&self, page: usize) -> String {
        let physical = (page + 1).to_string();
        let Some(excerpt) = self.excerpt(page, &physical) else {
            return physical;
        };
        let ellipsis = if excerpt.truncated { "…" } else { "" };
        let extra = ellipsis.len() + " · ".len() + physical.len();
        let mut label = if excerpt.prefix.is_empty() {
            let mut label = excerpt.suffix;
            label.reserve(extra);
            label
        } else {
            let mut label =
                String::with_capacity(excerpt.prefix.len() + excerpt.suffix.len() + extra);
            label.push_str(excerpt.prefix);
            label.push_str(&excerpt.suffix);
            label
        };
        label.push_str(ellipsis);
        label.push_str(" · ");
        label.push_str(&physical);
        label
    }

    /// Name the current page or pair, followed by the document page count.
    pub fn status_caption(&self, first: usize, second: Option<usize>, total: usize) -> String {
        let mut caption = self.display_name(first);
        let second = second.map(|page| self.display_name(page));
        let extra = " / ".len()
            + total
                .checked_ilog10()
                .map_or(1, |digits| digits as usize + 1)
            + second.as_ref().map_or(0, |label| "–".len() + label.len());
        caption.reserve(extra);
        if let Some(second) = second {
            caption.push('–');
            caption.push_str(&second);
        }
        let _ = write!(caption, " / {total}");
        caption
    }

    // Borrow the visible prefix and format a suffix only when it can appear.
    // Check full-label equality first, including unusual numeric prefixes.
    fn excerpt(&self, page: usize, physical: &str) -> Option<LabelExcerpt<'_>> {
        let range = self.range(page)?;
        let value = range.first + (page - range.page) as u64;
        if range.prefix.is_empty() {
            if matches!(range.style, Style::Decimal) && value == (page + 1) as u64 {
                return None;
            }
            let mut suffix = number(range.style, value);
            if suffix.is_empty() || suffix == physical {
                return None;
            }
            let truncated = suffix.len() > LABEL_EXCERPT_CHARACTERS;
            suffix.truncate(LABEL_EXCERPT_CHARACTERS);
            return Some(LabelExcerpt {
                prefix: "",
                suffix,
                truncated,
            });
        }
        let mut suffix = None;
        if let Some(tail) = physical.strip_prefix(&range.prefix) {
            let number = number(range.style, value);
            if number == tail {
                return None;
            }
            suffix = Some(number);
        }
        if let Some((end, _)) = range.prefix.char_indices().nth(LABEL_EXCERPT_CHARACTERS) {
            return Some(LabelExcerpt {
                prefix: &range.prefix[..end],
                suffix: String::new(),
                truncated: true,
            });
        }
        if matches!(range.style, Style::None) {
            return Some(LabelExcerpt {
                prefix: &range.prefix,
                suffix: String::new(),
                truncated: false,
            });
        }
        let remaining = LABEL_EXCERPT_CHARACTERS - range.prefix.chars().count();
        if remaining == 0 && value > 0 {
            return Some(LabelExcerpt {
                prefix: &range.prefix,
                suffix: String::new(),
                truncated: true,
            });
        }
        let mut suffix = suffix.unwrap_or_else(|| number(range.style, value));
        // All PDF numbering styles produce ASCII, so the remaining scalar
        // budget is also the byte limit for the formatted suffix.
        let truncated = suffix.len() > remaining;
        suffix.truncate(remaining);
        Some(LabelExcerpt {
            prefix: &range.prefix,
            suffix,
            truncated,
        })
    }

    /// Keep the physical page first in both sidebar lists and previews.
    pub fn sidebar_caption(&self, page: usize) -> String {
        let mut caption = format!("Page {}", page + 1);
        if let Some(excerpt) = self.excerpt(page, &caption["Page ".len()..]) {
            caption.reserve(
                " · ".len() + excerpt.prefix.len() + excerpt.suffix.len() + '…'.len_utf8(),
            );
            caption.push_str(" · ");
            caption.extend(excerpt.prefix.chars().map(|character| {
                if character.is_whitespace() {
                    ' '
                } else {
                    character
                }
            }));
            caption.push_str(&excerpt.suffix);
            if excerpt.truncated {
                caption.push('…');
            }
        }
        caption
    }

    /// Sidebar hover text uses the complete retained label, independently of its caption.
    pub fn sidebar_tooltip(&self, page: usize) -> String {
        let mut tooltip = format!("Go to page {}", page + 1);
        if let Some(label) = self.label(page)
            && !label.is_empty()
            && label != tooltip["Go to page ".len()..]
        {
            tooltip.push_str("\nPDF page label: ");
            tooltip.push_str(&label);
        }
        tooltip
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
                && let Some((page, canonical)) =
                    self.candidate_suffix(index, &label[range.prefix.len()..])
            {
                let (prefix, suffix) = label.split_at(range.prefix.len());
                if prefix == range.prefix && canonical == suffix {
                    return Some(page);
                }
                if folded.is_none() && canonical.eq_ignore_ascii_case(suffix) {
                    folded = Some(page);
                }
            }
            // Only non-ASCII lookup needs Unicode folding. Keep checking later
            // exact labels even after finding the earliest folded candidate.
            if folded.is_none() && (!ascii || !range.prefix.is_ascii()) {
                folded = self.folded_candidate(index, label, &mut folded_label);
            }
        }
        folded
    }

    // Keep Unicode folding and formatting out of the common range-scanning loop.
    #[inline(never)]
    fn folded_candidate(
        &self,
        index: usize,
        label: &str,
        folded_label: &mut Option<String>,
    ) -> Option<usize> {
        let normalized = folded_label.get_or_insert_with(|| crate::case_fold::fold(label));
        let suffix = crate::case_fold::strip_prefix(normalized, &self.ranges[index].prefix)?;
        let (page, canonical) = self.candidate_suffix(index, suffix)?;
        // All numbering formats generate ASCII; only prefixes need full folding.
        canonical.eq_ignore_ascii_case(suffix).then_some(page)
    }

    fn candidate_suffix(&self, index: usize, suffix: &str) -> Option<(usize, String)> {
        let range = &self.ranges[index];
        let value = parse_number(range.style, suffix, range.first)?;
        let offset = usize::try_from(value.checked_sub(range.first)?).ok()?;
        let page = range.page.checked_add(offset)?;
        let end = self
            .ranges
            .get(index + 1)
            .map_or(self.count, |next| next.page);
        if page >= end || page >= self.count {
            return None;
        }
        Some((page, number(range.style, value)))
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
        let bytes = include_bytes!("../assets/pagewing-guide.pdf");
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
    fn unresolved_label_pairs_do_not_hide_later_ranges() {
        let source = include_bytes!("../assets/pagewing-guide.pdf");
        let marker = b"/Nums [2 << /S /D >> 4 ";
        let mut bytes = source.to_vec();
        let offset = bytes
            .windows(marker.len())
            .position(|value| value == marker)
            .unwrap();
        bytes[offset..offset + marker.len()].copy_from_slice(b"/Nums [2 999 0 R     4 ");
        let pdf = Pdf::new(bytes).unwrap();
        let labels = PageLabels::read(&pdf);
        assert_eq!(labels.label(4).as_deref(), Some("A-1"));
        assert_eq!(labels.resolve_input("A-2", 6), Some(5));
    }

    #[test]
    fn label_tree_shares_one_raw_budget_across_leaf_pairs_and_child_arrays() {
        for (items, leaves, kids, late) in [
            (9999, 5, false, true),
            (10000, 5, false, false),
            (10000, 10, true, true),
            (10000, 11, true, false),
        ] {
            let pdf = crate::pdf_objects::repeated_tree(items, leaves, kids);
            let labels = PageLabels::read(&pdf);
            let expected = if late { "late1" } else { "early1" };
            assert_eq!(
                labels.label(0).as_deref(),
                Some(expected),
                "{items} items in {leaves} shared arrays; kids={kids}"
            );
            assert_eq!(labels.resolve_input(expected, 1), Some(0));
        }
    }

    #[test]
    fn invalid_number_tree_children_do_not_hide_later_label_ranges() {
        let source = include_bytes!("../assets/pagewing-guide.pdf");
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
    fn lookup_matches_linear_canonical_label_oracle() {
        // Enumerate canonical labels independently of the range candidate parser.
        // Exact matches across the whole document precede any folded match.
        fn oracle(labels: &PageLabels, input: &str) -> Option<usize> {
            if input.is_empty() {
                return None;
            }
            let all: Vec<_> = (0..labels.count).map(|page| labels.label(page)).collect();
            all.iter()
                .position(|label| label.as_deref() == Some(input))
                .or_else(|| {
                    let folded = caseless::default_case_fold_str(input);
                    all.iter().position(|label| {
                        label
                            .as_ref()
                            .is_some_and(|label| caseless::default_case_fold_str(label) == folded)
                    })
                })
        }

        let prefixes = [
            "",
            "A-",
            "a-",
            "Straße-",
            "STRASSE-",
            "ΟΣ-",
            "ος-",
            "ﬃİ-",
            "FFİ-",
            "附录-",
            "é-",
            "e\u{301}-",
            "S",
            "ſ",
            "1",
            "😀-",
        ];
        let styles = [
            Style::Decimal,
            Style::Roman,
            Style::LowerRoman,
            Style::Letters,
            Style::LowerLetters,
            Style::None,
        ];
        for seed in 0..24 {
            let labels = PageLabels {
                count: 40,
                // Leave page zero unlabeled in half the snapshots.
                ranges: (0..8)
                    .map(|index| LabelRange {
                        page: index * 5 + seed % 2,
                        first: [1, 18, 25, 27, 830, 39998, u32::MAX as u64][(seed + index) % 7],
                        prefix: prefixes[(seed + index) % prefixes.len()].into(),
                        style: styles[(seed / 4 + index) % styles.len()],
                    })
                    .collect(),
            };
            let mut queries = vec![String::new()];
            for page in 0..labels.count {
                if let Some(label) = labels.label(page) {
                    queries.extend([
                        label.clone(),
                        label.to_uppercase(),
                        label.to_lowercase(),
                        caseless::default_case_fold_str(&label),
                        format!("{label}0"),
                        format!("!{label}"),
                        label.replace('S', "ſ"),
                    ]);
                }
            }
            for prefix in prefixes {
                for suffix in [
                    "",
                    "0",
                    "01",
                    "+1",
                    "-1",
                    "iiii",
                    "IIV",
                    "AB",
                    "ſ",
                    "SS",
                    "832",
                    "833",
                    "39999",
                    "40000",
                    "4294967295",
                    "18446744073709551616",
                ] {
                    queries.push(format!("{prefix}{suffix}"));
                }
            }
            for query in queries {
                assert_eq!(
                    labels.find(&query),
                    oracle(&labels, &query),
                    "seed={seed}, {query:?}"
                );
                let explicit = format!(" ={} ", query.trim());
                assert_eq!(
                    labels.resolve_input(&explicit, labels.count),
                    oracle(&labels, query.trim())
                );
            }
            for physical in 0..=labels.count + 1 {
                assert_eq!(
                    labels.resolve_input(&physical.to_string(), labels.count),
                    physical.checked_sub(1).filter(|page| *page < labels.count)
                );
            }
        }
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

    #[test]
    fn preview_names_prioritize_physical_pages_and_keep_full_labels_in_tooltips() {
        for (label, caption) in [
            ("", "Page 1"),
            ("1", "Page 1"),
            ("i", "Page 1 · i"),
            ("first\nline\t!", "Page 1 · first line !"),
            (
                "中文😀".repeat(5).as_str(),
                "Page 1 · 中文😀中文😀中文😀中文😀…",
            ),
        ] {
            let labels = PageLabels {
                count: 1,
                ranges: vec![LabelRange {
                    page: 0,
                    first: 1,
                    prefix: label.into(),
                    style: Style::None,
                }],
            };
            assert_eq!(labels.sidebar_caption(0), caption);
            assert_eq!(
                labels.sidebar_tooltip(0),
                if label.is_empty() || label == "1" {
                    "Go to page 1".to_owned()
                } else {
                    format!("Go to page 1\nPDF page label: {label}")
                }
            );
        }
        assert_eq!(PageLabels::default().sidebar_caption(4), "Page 5");
        assert_eq!(PageLabels::default().sidebar_tooltip(4), "Go to page 5");
    }

    fn assert_excerpt_matches_full_label(labels: &PageLabels, page: usize) {
        let physical = (page + 1).to_string();
        let full = labels
            .label(page)
            .filter(|label| !label.is_empty() && label != &physical);
        let (display, caption) = if let Some(full) = full {
            let prefix: String = full.chars().take(12).collect();
            let ellipsis = if full.chars().count() > 12 { "…" } else { "" };
            let normalized: String = prefix
                .chars()
                .map(|character| {
                    if character.is_whitespace() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect();
            (
                format!("{prefix}{ellipsis} · {physical}"),
                format!("Page {physical} · {normalized}{ellipsis}"),
            )
        } else {
            (physical.clone(), format!("Page {physical}"))
        };
        assert_eq!(
            labels.display_name(page),
            display,
            "page {page}, {labels:?}"
        );
        assert_eq!(
            labels.sidebar_caption(page),
            caption,
            "page {page}, {labels:?}"
        );
    }

    #[test]
    fn bounded_excerpts_match_full_labels_across_numbering_styles_and_prefix_boundaries() {
        let prefixes = [
            String::new(),
            "1".into(),
            "12".into(),
            "000".into(),
            "123456789012".into(),
            "1234567890123".into(),
            "Appendix A: ".into(),
            "中文😀".repeat(100),
            "\n\r\t\u{2028} prefix ".into(),
        ];
        for style in [
            Style::Decimal,
            Style::Roman,
            Style::LowerRoman,
            Style::Letters,
            Style::LowerLetters,
            Style::None,
        ] {
            for prefix in &prefixes {
                for first in [0, 1, 26, 27, 832, 833, 39_999, 40_000, u64::from(u32::MAX)] {
                    let labels = PageLabels {
                        count: 128,
                        ranges: vec![LabelRange {
                            page: 5,
                            first,
                            prefix: prefix.clone(),
                            style,
                        }],
                    };
                    for page in [0, 4, 5, 6, 7, 12, 37, 126, 127, 128] {
                        assert_excerpt_matches_full_label(&labels, page);
                    }
                }
            }
        }
    }

    #[test]
    fn long_numeric_labels_equal_to_the_physical_page_keep_the_full_number() {
        let page = usize::MAX - 1;
        let physical = (page + 1).to_string();
        for (prefix, style, first) in [
            (physical.clone(), Style::None, 1),
            (
                physical[..physical.len() - 1].into(),
                Style::Decimal,
                u64::from(physical.as_bytes()[physical.len() - 1] - b'0'),
            ),
        ] {
            let labels = PageLabels {
                count: usize::MAX,
                ranges: vec![LabelRange {
                    page,
                    first,
                    prefix,
                    style,
                }],
            };
            assert_excerpt_matches_full_label(&labels, page);
            assert_eq!(labels.display_name(page), physical);
        }
        for style in [
            Style::Roman,
            Style::LowerRoman,
            Style::Letters,
            Style::LowerLetters,
        ] {
            let labels = PageLabels {
                count: 40_001,
                ranges: vec![LabelRange {
                    page: 0,
                    first: 1,
                    prefix: String::new(),
                    style,
                }],
            };
            assert_excerpt_matches_full_label(&labels, 39_999);
            assert_eq!(labels.display_name(39_999), "40000");
        }
    }
}
