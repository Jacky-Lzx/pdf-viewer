mod boundaries;
mod search_index;
mod snippet;
mod span;
pub use span::TextSpan;

use boundaries::WordBoundaries;
use hayro::{
    hayro_interpret::{
        self as interpret, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image,
        ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask,
        font::{Glyph, GlyphRun},
        hayro_cmap::BfString,
        util::TransformExt,
    },
    hayro_syntax::page::Page,
    kurbo::{BezPath, Point, Rect},
};
use search_index::SearchIndex;
use std::{ops::Range, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

pub struct SearchQuery {
    needle: String,
    options: SearchOptions,
}

impl SearchQuery {
    pub fn new(query: String, options: SearchOptions) -> Self {
        let needle = if query.trim().is_empty() {
            String::new()
        } else if options.match_case {
            query
        } else {
            caseless::default_case_fold_str(&query)
        };
        Self { needle, options }
    }

    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub text: String,
    pub spans: Vec<TextSpan>,
    index: OnceLock<SearchIndex>,
    selection_index: OnceLock<crate::selection_index::SelectionIndex>,
    word_boundaries: OnceLock<WordBoundaries>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub page: usize,
    pub rects: Vec<Rect>,
    pub snippet: String,
}

#[derive(Default)]
pub struct SearchCursor {
    offset: usize,
    previous: Option<(usize, usize)>,
}

pub struct SearchBatch {
    pub hits: Vec<SearchHit>,
    pub complete: bool,
}

/// Search batches arrive in page order, so only the visible page needs to be visited.
pub fn page_hits(hits: &[SearchHit], page: usize) -> Range<usize> {
    let first = hits.partition_point(|hit| hit.page < page);
    let count = hits[first..].partition_point(|hit| hit.page == page);
    first..first + count
}

pub fn initial_match(
    hits: &[SearchHit],
    page: usize,
    reverse: bool,
    scanned: usize,
    complete: bool,
) -> Option<usize> {
    if hits.is_empty() || (reverse && !complete && scanned <= page) {
        return None;
    }
    if reverse {
        hits.partition_point(|hit| hit.page <= page)
            .checked_sub(1)
            .or_else(|| complete.then_some(hits.len() - 1))
    } else {
        let index = hits.partition_point(|hit| hit.page < page);
        if index < hits.len() {
            Some(index)
        } else {
            complete.then_some(0)
        }
    }
}

/// Resolve a counted jump without walking intermediate hits or wrapping before
/// all pages have been searched. Arithmetic also handles arbitrarily large counts.
pub fn advance_match(
    index: usize,
    total: usize,
    reverse: bool,
    steps: usize,
    complete: bool,
) -> Option<usize> {
    if index >= total {
        return None;
    }
    if !complete {
        return if reverse {
            index.checked_sub(steps)
        } else {
            index.checked_add(steps).filter(|&index| index < total)
        };
    }
    let delta = steps % total;
    Some(if reverse {
        if delta <= index {
            index - delta
        } else {
            total - (delta - index)
        }
    } else if delta < total - index {
        index + delta
    } else {
        delta - (total - index)
    })
}

impl PageText {
    pub fn prepare_selection(&self) {
        if self.spans.len() >= 256 {
            self.word_boundaries
                .get_or_init(|| WordBoundaries::new(&self.text))
                .prepare_lines(&self.text);
            self.selection_index
                .get_or_init(|| crate::selection_index::SelectionIndex::new(&self.spans));
        }
    }

    pub fn word_range(&self, offset: usize) -> Range<usize> {
        self.word_boundaries.get().map_or_else(
            || boundaries::word_range(&self.text, offset),
            |boundaries| boundaries.word_range(&self.text, offset),
        )
    }

    pub fn line_range(&self, offset: usize) -> Range<usize> {
        self.word_boundaries.get().map_or_else(
            || boundaries::line_range(&self.text, offset),
            |boundaries| boundaries.line_range(&self.text, offset),
        )
    }

    pub fn nearest_span(&self, point: Point) -> Option<usize> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        if let Some(index) = self.selection_index.get() {
            return index.nearest(&self.spans, point);
        }
        self.spans
            .iter()
            .enumerate()
            .filter(|(_, span)| span.rect.is_finite() && !span.range.is_empty())
            .min_by(|(_, a), (_, b)| {
                let distance = |rect: Rect| {
                    let dx = (rect.x0 - point.x).max(0.0).max(point.x - rect.x1);
                    let dy = (rect.y0 - point.y).max(0.0).max(point.y - rect.y1);
                    (dy, dx)
                };
                let (ay, ax) = distance(a.rect);
                let (by, bx) = distance(b.rect);
                // Keep drags beyond a short line on that line, even if the next is longer.
                ay.total_cmp(&by).then_with(|| ax.total_cmp(&bx))
            })
            .map(|(index, _)| index)
    }

    pub fn rects_for_range(&self, range: Range<usize>) -> Vec<Rect> {
        let first = self
            .spans
            .partition_point(|span| span.range.end <= range.start);
        let mut rects: Vec<Rect> = Vec::new();
        for rect in self.spans[first..]
            .iter()
            .take_while(|span| span.range.start < range.end)
            .map(|span| span.rect)
            .filter(|rect| rect.is_finite())
        {
            if let Some(last) = rects.last_mut()
                && (last.y0 - rect.y0).abs() < rect.height() * 0.2
                && rect.x0 >= last.x0
                && rect.x0 - last.x1 < rect.height() * 0.3
            {
                *last = last.union(rect);
            } else {
                rects.push(rect);
            }
        }
        rects
    }

    pub fn memory_bytes(&self) -> usize {
        let index_bytes = self.index.get().map_or_else(
            || SearchIndex::reservation(&self.text),
            SearchIndex::memory_bytes,
        );
        self.text.capacity()
            + self.spans.capacity() * std::mem::size_of::<TextSpan>()
            + index_bytes
            + self
                .selection_index
                .get()
                .map_or(0, |index| index.memory_bytes())
            + self
                .word_boundaries
                .get()
                .map_or(0, |boundaries| boundaries.memory_bytes())
    }

    #[cfg(test)]
    pub fn search(&self, page: usize, query: &str) -> Vec<SearchHit> {
        self.search_with(
            page,
            &SearchQuery::new(query.into(), SearchOptions::default()),
        )
    }

    #[cfg(test)]
    pub fn search_with(&self, page: usize, query: &SearchQuery) -> Vec<SearchHit> {
        let mut cursor = SearchCursor::default();
        let mut hits = Vec::new();
        loop {
            let batch = self.search_batch(page, query, &mut cursor, 10_000 - hits.len());
            hits.extend(batch.hits);
            if batch.complete || hits.len() >= 10_000 {
                return hits;
            }
        }
    }

    pub fn search_batch(
        &self,
        page: usize,
        query: &SearchQuery,
        cursor: &mut SearchCursor,
        limit: usize,
    ) -> SearchBatch {
        if query.is_empty() {
            return SearchBatch {
                hits: Vec::new(),
                complete: true,
            };
        }
        // Case folding can expand one source glyph into several searchable characters.
        let index = (!query.options.match_case)
            .then(|| self.index.get_or_init(|| SearchIndex::new(&self.text)));
        let boundaries = query.options.whole_word.then(|| {
            self.word_boundaries
                .get_or_init(|| WordBoundaries::new(&self.text))
        });
        let haystack = index.map_or(self.text.as_str(), |index| &index.folded);
        let mut base = cursor.offset;
        let mut matches = haystack[base..].match_indices(query.needle.as_str());
        let mut hits = Vec::new();
        let mut candidates = 0;
        // Bound both emitted hits and rejected whole-word candidates per job.
        while hits.len() < limit && candidates < 4096 {
            let Some((relative, matched)) = matches.next() else {
                cursor.offset = haystack.len();
                break;
            };
            candidates += 1;
            let offset = base + relative;
            let matched_end = offset + matched.len();
            cursor.offset = matched_end;
            // A partial case-fold expansion is not an entire word (s != ß).
            let partial_expansion = boundaries.is_some()
                && index.is_some_and(|index| index.partial_expansion(offset..matched_end));
            let (start, end) = if let Some(index) = index {
                let range = index.source_range(&self.text, offset..matched_end);
                (range.start, range.end)
            } else {
                (offset, matched_end)
            };
            if partial_expansion
                || boundaries.is_some_and(|boundaries| {
                    !boundaries.contains(start) || !boundaries.contains(end)
                })
            {
                // Rejected candidates may overlap a later valid phrase.
                base = offset + haystack[offset..].chars().next().unwrap().len_utf8();
                cursor.offset = base;
                matches = haystack[base..].match_indices(query.needle.as_str());
                continue;
            }
            if cursor.previous == Some((start, end)) {
                continue;
            }
            cursor.previous = Some((start, end));
            let rects = self.rects_for_range(start..end);
            hits.push(SearchHit {
                page,
                rects,
                snippet: snippet::snippet(&self.text, start),
            });
        }
        SearchBatch {
            hits,
            complete: cursor.offset >= haystack.len(),
        }
    }
}

pub fn extract<'a>(page: &Page<'a>, cache: &InterpreterCache<'a>) -> PageText {
    let (w, h) = page.render_dimensions();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        Rect::new(0.0, 0.0, w as f64, h as f64),
        cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = TextDevice::default();
    interpret::interpret_page(page, &mut context, &mut device);
    device.output
}

#[derive(Default)]
struct TextDevice {
    output: PageText,
    previous: Option<Rect>,
}

impl<'a> Device<'a> for TextDevice {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for glyph in run.glyphs() {
            let Some(unicode) = glyph.as_unicode() else {
                continue;
            };
            let width = match &**glyph {
                Glyph::Outline(g) => g.advance_width().unwrap_or(500.0) as f64,
                Glyph::Type3(_) => 500.0,
            };
            let transform = props.transform * glyph.transform();
            let rect = transform.transform_rect_bbox(Rect::new(0.0, -200.0, width.max(1.0), 800.0));
            let origin = transform * Point::ZERO;
            if !origin.x.is_finite() || !origin.y.is_finite() {
                continue;
            }
            if let Some(previous) = self.previous {
                let line_height = rect.height().max(previous.height()).max(1.0);
                if (rect.y0 - previous.y0).abs() > line_height * 0.65 {
                    self.output.text.push('\n');
                } else if rect.x0 - previous.x1 > line_height * 0.15
                    && !self.output.text.ends_with(char::is_whitespace)
                {
                    self.output.text.push(' ');
                }
            }
            let start = self.output.text.len();
            match unicode {
                BfString::Char(ch) => self.output.text.push(ch),
                BfString::String(text) => self.output.text.push_str(&text),
            }
            self.output.spans.push(TextSpan {
                range: start..self.output.text.len(),
                rect,
            });
            self.previous = Some(rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_resume_dense_pages_without_losing_or_duplicating_hits() {
        let value = "ß Straße ﬃ rust ".repeat(2000);
        let page = PageText {
            spans: value
                .char_indices()
                .map(|(start, ch)| TextSpan {
                    range: start..start + ch.len_utf8(),
                    rect: Rect::new(start as f64, 0.0, (start + ch.len_utf8()) as f64, 10.0),
                })
                .collect(),
            text: value,
            ..Default::default()
        };
        for (needle, whole_word, expected) in [
            ("s", false, 8000),
            ("ss", true, 2000),
            ("rust", true, 2000),
            ("f", false, 2000),
        ] {
            let query = SearchQuery::new(
                needle.into(),
                SearchOptions {
                    match_case: false,
                    whole_word,
                },
            );
            let reference = page.search_with(4, &query);
            assert_eq!(reference.len(), expected);
            for limit in [1, 17, 256] {
                let mut cursor = SearchCursor::default();
                let mut actual = Vec::new();
                loop {
                    let batch = page.search_batch(4, &query, &mut cursor, limit);
                    assert!(batch.hits.len() <= limit);
                    actual.extend(batch.hits);
                    if batch.complete {
                        break;
                    }
                }
                assert_eq!(actual.len(), expected, "{needle}, {limit}");
                for (actual, reference) in actual.iter().zip(&reference) {
                    assert_eq!(actual.page, reference.page);
                    assert_eq!(actual.rects, reference.rects);
                    assert_eq!(actual.snippet, reference.snippet);
                }
                assert!(
                    page.search_batch(4, &query, &mut cursor, limit)
                        .hits
                        .is_empty()
                );
            }
        }
    }

    #[test]
    fn rejected_candidates_yield_and_preserve_overlapping_phrase_across_batches() {
        // Candidate 4096 is rejected; the overlapping candidate after it is valid.
        let value = format!("{}ba a a", "ba a ".repeat(4095));
        let page = PageText {
            text: value,
            ..Default::default()
        };
        let query = SearchQuery::new(
            "a a".into(),
            SearchOptions {
                match_case: false,
                whole_word: true,
            },
        );
        let mut cursor = SearchCursor::default();
        let first = page.search_batch(0, &query, &mut cursor, 256);
        assert!(first.hits.is_empty() && !first.complete);
        assert!(cursor.offset > 0 && cursor.offset < page.text.len());
        let last = page.search_batch(0, &query, &mut cursor, 256);
        assert_eq!(last.hits.len(), 1);
        assert!(last.complete);
    }

    #[test]
    fn counted_jumps_wrap_only_after_scan_completion_and_handle_large_counts() {
        assert_eq!(advance_match(0, 0, false, 1, true), None);
        assert_eq!(advance_match(3, 3, false, 1, true), None);
        assert_eq!(advance_match(1, 4, false, 2, false), Some(3));
        assert_eq!(advance_match(1, 4, false, 3, false), None);
        assert_eq!(advance_match(1, 4, false, 3, true), Some(0));
        assert_eq!(advance_match(2, 4, true, 2, false), Some(0));
        assert_eq!(advance_match(2, 4, true, 3, false), None);
        assert_eq!(advance_match(2, 4, true, 3, true), Some(3));
        for total in 1..20 {
            for index in 0..total {
                for steps in 0..100 {
                    assert_eq!(
                        advance_match(index, total, false, steps, true),
                        Some((index + steps) % total)
                    );
                    assert_eq!(
                        advance_match(index, total, true, steps, true),
                        Some((index + total - steps % total) % total)
                    );
                }
            }
        }
        assert_eq!(advance_match(0, 1, false, usize::MAX, true), Some(0));
        assert_eq!(
            advance_match(usize::MAX - 1, usize::MAX, false, usize::MAX - 1, true),
            Some(usize::MAX - 2)
        );
        assert_eq!(
            advance_match(0, usize::MAX, true, usize::MAX - 1, true),
            Some(1)
        );
        assert_eq!(
            advance_match(usize::MAX - 1, usize::MAX, false, usize::MAX, false),
            None
        );
    }

    #[test]
    fn match_case_uses_original_unicode_and_does_not_build_a_folded_index() {
        let page = PageText {
            text: "Rust rust RUST Straße STRASSE Σ σ ς İ i\u{307}".into(),
            ..Default::default()
        };
        let exact = |needle: &str| {
            SearchQuery::new(
                needle.into(),
                SearchOptions {
                    match_case: true,
                    whole_word: false,
                },
            )
        };
        for needle in [
            "Rust", "rust", "RUST", "Straße", "STRASSE", "Σ", "σ", "ς", "İ", "i\u{307}",
        ] {
            assert_eq!(page.search_with(2, &exact(needle)).len(), 1, "{needle}");
        }
        assert!(page.search_with(2, &exact("strasse")).is_empty());
        assert!(page.index.get().is_none());
        assert!(page.word_boundaries.get().is_none());
        assert_eq!(page.search(2, "strasse").len(), 2);
    }

    #[test]
    fn whole_words_follow_unicode_boundaries_and_keep_original_highlight_ranges() {
        let value =
            "rust rusting trust (RUST), rust-lang rust_lang don't don e\u{301} e Straße ß ﬃ 中文";
        let page = PageText {
            text: value.into(),
            spans: value
                .char_indices()
                .map(|(start, ch)| TextSpan {
                    range: start..start + ch.len_utf8(),
                    rect: Rect::new(start as f64, 0.0, (start + ch.len_utf8()) as f64, 10.0),
                })
                .collect(),
            ..Default::default()
        };
        let whole = |needle: &str| {
            SearchQuery::new(
                needle.into(),
                SearchOptions {
                    match_case: false,
                    whole_word: true,
                },
            )
        };
        for (needle, count) in [
            ("rust", 3),
            ("rust-lang", 1),
            ("rust_lang", 1),
            ("don", 1),
            ("don't", 1),
            ("e", 1),
            ("e\u{301}", 1),
            ("strasse", 1),
            ("ss", 1),
            ("s", 0),
            ("f", 0),
            ("ffi", 1),
            ("中文", 1),
            ("中", 1),
            ("rust rusting", 1),
        ] {
            assert_eq!(page.search_with(0, &whole(needle)).len(), count, "{needle}");
        }
        let start = value.find('ß').unwrap();
        assert_eq!(
            page.search_with(0, &whole("strasse"))[0].rects,
            [Rect::new((start - 4) as f64, 0.0, (start + 3) as f64, 10.0)]
        );
        assert!(page.word_boundaries.get().unwrap().contains(value.len()));
        let exact = SearchQuery::new(
            "rust".into(),
            SearchOptions {
                match_case: true,
                whole_word: true,
            },
        );
        assert_eq!(page.search_with(0, &exact).len(), 2);
    }

    #[test]
    fn prepared_query_reuses_unicode_folding_and_empty_search_stays_lazy() {
        let query = SearchQuery::new("STRASSE".into(), SearchOptions::default());
        assert_eq!(query.needle, "strasse");
        for value in ["Straße", "STRASSE", "strasse"] {
            let page = PageText {
                text: value.into(),
                ..Default::default()
            };
            assert_eq!(page.search_with(0, &query).len(), 1);
        }
        let page = PageText {
            text: "text".into(),
            ..Default::default()
        };
        let empty = SearchQuery::new(
            " \n\t".into(),
            SearchOptions {
                match_case: false,
                whole_word: true,
            },
        );
        assert!(page.search_with(0, &empty).is_empty());
        assert!(page.index.get().is_none() && page.word_boundaries.get().is_none());
    }

    #[test]
    fn rejected_whole_word_candidate_does_not_skip_an_overlapping_valid_phrase() {
        for match_case in [false, true] {
            let page = PageText {
                text: "ba a a".into(),
                ..Default::default()
            };
            let query = SearchQuery::new(
                "a a".into(),
                SearchOptions {
                    match_case,
                    whole_word: true,
                },
            );
            assert_eq!(page.search_with(0, &query).len(), 1);
        }
        // Accepting a phrase still consumes it, preserving non-overlapping results.
        let page = PageText {
            text: "a a a a".into(),
            ..Default::default()
        };
        let query = SearchQuery::new(
            "a a".into(),
            SearchOptions {
                match_case: false,
                whole_word: true,
            },
        );
        assert_eq!(page.search_with(0, &query).len(), 2);
    }

    #[test]
    fn page_hit_ranges_preserve_global_selection_indices() {
        let hits: Vec<_> = [0, 0, 2, 2, 2, 7]
            .into_iter()
            .map(|page| SearchHit {
                page,
                rects: Vec::new(),
                snippet: String::new(),
            })
            .collect();
        assert_eq!(page_hits(&hits, 0), 0..2);
        assert_eq!(page_hits(&hits, 1), 2..2);
        assert_eq!(page_hits(&hits, 2), 2..5);
        assert_eq!(page_hits(&hits, 7), 5..6);
        assert_eq!(page_hits(&hits, 8), 6..6);
        assert_eq!(page_hits(&[], 0), 0..0);
        assert_eq!(initial_match(&hits[..2], 2, false, 1, false), None);
        assert_eq!(initial_match(&hits, 2, false, 8, false), Some(2));
        assert_eq!(initial_match(&hits, 2, true, 2, false), None);
        assert_eq!(initial_match(&hits, 2, true, 3, false), Some(4));
        assert_eq!(initial_match(&hits, 8, false, 8, false), None);
        assert_eq!(initial_match(&hits, 8, false, 8, true), Some(0));
        assert_eq!(initial_match(&hits[2..], 0, true, 8, true), Some(3));
        assert_eq!(initial_match(&[], 0, false, 8, true), None);
    }
    #[test]
    fn search_handles_unicode_case_expansion_and_empty_queries() {
        let text = PageText {
            text: "İstanbul 中文 RUST rust".into(),
            spans: Vec::new(),
            ..Default::default()
        };
        assert_eq!(text.search(0, "RUST").len(), 2);
        assert_eq!(text.search(0, "中文").len(), 1);
        assert_eq!(text.search(0, "i\u{307}stanbul").len(), 1);
        assert!(text.search(0, " ").is_empty());
        assert!(text.search(0, "missing").is_empty());
    }

    #[test]
    fn ascii_search_preserves_mapped_geometry_options_and_unicode_queries_without_a_map() {
        let value = "RUST K FFI s rusting\nrust \0\t".repeat(20);
        let optimized = PageText {
            spans: value
                .char_indices()
                .map(|(offset, _)| TextSpan {
                    range: offset..offset + 1,
                    rect: Rect::new(offset as f64, 0.0, offset as f64 + 1.0, 10.0),
                })
                .collect(),
            text: value.clone(),
            ..Default::default()
        };
        let mapped = optimized.clone();
        mapped
            .index
            .set(SearchIndex {
                folded: value.to_ascii_lowercase(),
                source: Some(search_index::SourceMap {
                    start: 0,
                    offsets: (0..value.len()).collect(),
                }),
            })
            .unwrap();
        let reserved = optimized.memory_bytes();
        for whole_word in [false, true] {
            for needle in ["rUsT", "K", "ﬃ", "ſ", "s", "rust rusting", "missing", "\0"] {
                let query = SearchQuery::new(
                    needle.into(),
                    SearchOptions {
                        match_case: false,
                        whole_word,
                    },
                );
                let reference = mapped.search_with(2, &query);
                let mut cursor = SearchCursor::default();
                let mut hits = Vec::new();
                loop {
                    let batch = optimized.search_batch(2, &query, &mut cursor, 3);
                    hits.extend(batch.hits);
                    if batch.complete {
                        break;
                    }
                }
                assert_eq!(hits.len(), reference.len(), "{needle}, {whole_word}");
                for (actual, expected) in hits.iter().zip(reference) {
                    assert_eq!(actual.page, expected.page);
                    assert_eq!(actual.rects, expected.rects);
                    assert_eq!(actual.snippet, expected.snippet);
                }
            }
        }
        let index = optimized.index.get().unwrap();
        assert!(index.source.is_none());
        assert_eq!(index.memory_bytes(), value.len());
        // Whole-word data has its own independently accounted allocation.
        assert_eq!(
            optimized.memory_bytes(),
            reserved + optimized.word_boundaries.get().unwrap().memory_bytes()
        );
    }

    #[test]
    fn unchanged_unicode_boundaries_preserve_search_geometry_without_a_map() {
        use caseless::Caseless;

        let value = "ÉCOLE école Σσς ΟΣ ος 中文 cafe\u{301} 👩‍💻\n".repeat(20);
        let optimized = PageText {
            spans: value
                .char_indices()
                .enumerate()
                .map(|(glyph, (offset, ch))| TextSpan {
                    range: offset..offset + ch.len_utf8(),
                    rect: Rect::new(glyph as f64 * 10.0, 0.0, glyph as f64 * 10.0 + 8.0, 10.0),
                })
                .collect(),
            text: value.clone(),
            ..Default::default()
        };
        let reference = optimized.clone();
        let mut folded = String::new();
        let mut offsets = Vec::new();
        for (offset, ch) in value.char_indices() {
            for lower in std::iter::once(ch).default_case_fold() {
                folded.push(lower);
                offsets.extend(std::iter::repeat_n(offset, lower.len_utf8()));
            }
        }
        reference
            .index
            .set(SearchIndex {
                folded,
                source: Some(search_index::SourceMap { start: 0, offsets }),
            })
            .unwrap();
        for whole_word in [false, true] {
            for needle in [
                "École",
                "σ",
                "οσ",
                "中",
                "中文",
                "e\u{301}",
                "👩‍💻",
                "missing",
            ] {
                let query = SearchQuery::new(
                    needle.into(),
                    SearchOptions {
                        match_case: false,
                        whole_word,
                    },
                );
                let expected = reference.search_with(4, &query);
                let mut cursor = SearchCursor::default();
                let mut hits = Vec::new();
                loop {
                    let batch = optimized.search_batch(4, &query, &mut cursor, 7);
                    hits.extend(batch.hits);
                    if batch.complete {
                        break;
                    }
                }
                assert_eq!(hits.len(), expected.len(), "{needle}, {whole_word}");
                for (actual, expected) in hits.iter().zip(expected) {
                    assert_eq!(actual.page, expected.page);
                    assert_eq!(actual.rects, expected.rects);
                    assert_eq!(actual.snippet, expected.snippet);
                }
            }
        }
        let index = optimized.index.get().unwrap();
        assert!(index.source.is_none());
        assert_eq!(index.memory_bytes(), value.len());
    }

    #[test]
    fn compact_mapping_preserves_expanded_case_and_multibyte_glyph_ranges() {
        let value = "İ中İ 文";
        let spans = value
            .char_indices()
            .enumerate()
            .map(|(index, (start, ch))| TextSpan {
                range: start..start + ch.len_utf8(),
                rect: Rect::new(index as f64 * 10.0, 0.0, index as f64 * 10.0 + 10.0, 10.0),
            })
            .collect();
        let page = PageText {
            text: value.into(),
            spans,
            ..Default::default()
        };
        let reserve = page.memory_bytes();
        assert_eq!(
            page.search(0, "i")[0].rects,
            [Rect::new(0.0, 0.0, 10.0, 10.0)]
        );
        assert_eq!(
            page.search(0, "\u{307}中i")[0].rects,
            [Rect::new(0.0, 0.0, 30.0, 10.0)]
        );
        assert_eq!(
            page.search(0, "文")[0].rects,
            [Rect::new(40.0, 0.0, 50.0, 10.0)]
        );
        assert!(page.memory_bytes() <= reserve);
        let index = page.index.get().unwrap();
        let source = &index.source.as_ref().unwrap().offsets;
        assert_eq!(source.len(), index.folded.len());
        assert!(
            source
                .iter()
                .all(|offset| page.text.is_char_boundary(*offset))
        );
    }

    #[test]
    fn lazy_index_reservation_covers_large_unicode_expansion() {
        let ascii = PageText {
            text: "Rust ".repeat(100),
            ..Default::default()
        };
        let reserved = ascii.memory_bytes();
        ascii.search(0, "missing");
        assert_eq!(
            ascii.memory_bytes(),
            reserved,
            "ASCII indexes need no growth allowance"
        );
        for value in ["ΐ", "İ", "Ⱥ", "ῗ", "ΐİ", "中ΐ", "ﬃ"] {
            let page = PageText {
                text: value.into(),
                ..Default::default()
            };
            let reserved = page.memory_bytes();
            page.search(0, "missing");
            assert!(
                page.memory_bytes() <= reserved,
                "vector growth exceeded reservation for {value}"
            );
        }
        let page = PageText {
            text: "İȺȾΣΐ 中文 ".repeat(1000),
            ..Default::default()
        };
        let reserved = page.memory_bytes();
        assert!(page.search(0, "missing").is_empty());
        assert!(page.memory_bytes() <= reserved);
        assert_eq!(page.search(0, "中文").len(), 1000);
        assert!(page.memory_bytes() <= reserved);
    }

    #[test]
    fn unicode_case_folding_matches_sharp_s_sigma_and_ligatures() {
        let value = "Straße STRASSE ΟΣ ος Σσς ﬃ İ ı";
        let spans = value
            .char_indices()
            .enumerate()
            .map(|(index, (start, ch))| TextSpan {
                range: start..start + ch.len_utf8(),
                rect: Rect::new(index as f64 * 10.0, 0.0, index as f64 * 10.0 + 10.0, 10.0),
            })
            .collect();
        let text = PageText {
            text: value.into(),
            spans,
            ..Default::default()
        };
        let german = text.search(0, "strasse");
        assert_eq!(german.len(), 2);
        assert_eq!(german[0].rects, [Rect::new(0.0, 0.0, 60.0, 10.0)]);
        assert_eq!(text.search(0, "ΟΣ").len(), 2);
        assert_eq!(text.search(0, "σ").len(), 5);
        assert_eq!(text.search(0, "ffi").len(), 1);
        assert_eq!(
            text.search(0, "f").len(),
            1,
            "one ligature must not produce duplicate matches"
        );
        assert_eq!(
            text.search(0, "s").len(),
            5,
            "one sharp-s glyph must not produce duplicate matches"
        );
        assert_eq!(text.search(0, "ı").len(), 1);
    }

    #[test]
    fn dense_matches_use_correct_ranges_and_merge_adjacent_glyphs() {
        let text = "rust ".repeat(2000);
        let spans = text
            .char_indices()
            .map(|(i, _)| TextSpan {
                range: i..i + 1,
                rect: Rect::new(i as f64, 0.0, i as f64 + 1.0, 10.0),
            })
            .collect();
        let page = PageText {
            text,
            spans,
            ..Default::default()
        };
        let hits = page.search(3, "rust");
        assert_eq!(hits.len(), 2000);
        assert!(hits.iter().all(|hit| hit.rects.len() == 1 && hit.page == 3));
        assert_eq!(hits[10].rects[0], Rect::new(50.0, 0.0, 54.0, 10.0));
        assert_eq!(page.search(3, "RUST").len(), 2000);
    }
}
