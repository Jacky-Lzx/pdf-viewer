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
use std::{ops::Range, sync::OnceLock};

#[derive(Clone, Debug)]
pub struct TextSpan {
    pub range: Range<usize>,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub text: String,
    pub spans: Vec<TextSpan>,
    index: OnceLock<SearchIndex>,
}

#[derive(Clone, Debug)]
struct SearchIndex {
    folded: String,
    source: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub page: usize,
    pub rects: Vec<Rect>,
    pub snippet: String,
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

impl PageText {
    pub fn nearest_span(&self, point: Point) -> Option<usize> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
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
        // Reserve for Unicode lowercase expansion before the lazy index is built.
        let index_bytes = self.index.get().map_or_else(
            || self.text.len() * 3 * (1 + std::mem::size_of::<usize>()),
            |index| {
                index.folded.capacity() + index.source.capacity() * std::mem::size_of::<usize>()
            },
        );
        self.text.capacity() + self.spans.capacity() * std::mem::size_of::<TextSpan>() + index_bytes
    }

    pub fn search(&self, page: usize, query: &str) -> Vec<SearchHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        // Keep an explicit byte mapping: Unicode lowercasing can change byte length.
        let index = self.index.get_or_init(|| {
            let mut folded = String::with_capacity(self.text.len());
            let mut source = Vec::with_capacity(self.text.len());
            for (offset, ch) in self.text.char_indices() {
                for lower in ch.to_lowercase() {
                    folded.push(lower);
                    source.extend(std::iter::repeat_n(offset, lower.len_utf8()));
                }
            }
            SearchIndex { folded, source }
        });
        let needle = query.to_lowercase();
        index
            .folded
            .match_indices(&needle)
            .take(1000)
            .map(|(offset, matched)| {
                let start = index.source[offset];
                let last = index.source[offset + matched.len() - 1];
                let end = last + self.text[last..].chars().next().unwrap().len_utf8();
                let rects = self.rects_for_range(start..end);
                let before: String = self.text[..start]
                    .chars()
                    .rev()
                    .take(32)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let after: String = self.text[start..].chars().take(80).collect();
                SearchHit {
                    page,
                    rects,
                    snippet: format!("{before}{after}").replace('\n', " "),
                }
            })
            .collect()
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
        assert_eq!(index.source.len(), index.folded.len());
        assert!(
            index
                .source
                .iter()
                .all(|offset| page.text.is_char_boundary(*offset))
        );
    }

    #[test]
    fn lazy_index_reservation_covers_large_unicode_expansion() {
        let page = PageText {
            text: "İȺȾΣ 中文 ".repeat(1000),
            ..Default::default()
        };
        let reserved = page.memory_bytes();
        assert!(page.search(0, "missing").is_empty());
        assert!(page.memory_bytes() <= reserved);
        assert_eq!(page.search(0, "中文").len(), 1000);
        assert!(page.memory_bytes() <= reserved);
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
        assert_eq!(hits.len(), 1000);
        assert!(hits.iter().all(|hit| hit.rects.len() == 1 && hit.page == 3));
        assert_eq!(hits[10].rects[0], Rect::new(50.0, 0.0, 54.0, 10.0));
        assert_eq!(page.search(3, "RUST").len(), 1000);
    }
}
