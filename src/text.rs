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
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct TextSpan {
    pub range: Range<usize>,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub text: String,
    pub spans: Vec<TextSpan>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub page: usize,
    pub rects: Vec<Rect>,
    pub snippet: String,
}

impl PageText {
    pub fn search(&self, page: usize, query: &str) -> Vec<SearchHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        // Keep an explicit byte mapping: Unicode lowercasing can change byte length.
        let mut folded = String::new();
        let mut source = Vec::new();
        for (offset, ch) in self.text.char_indices() {
            for lower in ch.to_lowercase() {
                folded.push(lower);
                source.extend(std::iter::repeat_n(
                    (offset, offset + ch.len_utf8()),
                    lower.len_utf8(),
                ));
            }
        }
        let needle = query.to_lowercase();
        folded
            .match_indices(&needle)
            .take(1000)
            .map(|(offset, matched)| {
                let start = source[offset].0;
                let end = source[offset + matched.len() - 1].1;
                let rects = self
                    .spans
                    .iter()
                    .filter(|span| span.range.start < end && span.range.end > start)
                    .map(|span| span.rect)
                    .collect();
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
            let text = match unicode {
                BfString::Char(ch) => ch.to_string(),
                BfString::String(text) => text,
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
            self.output.text.push_str(&text);
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
    fn search_handles_unicode_case_expansion_and_empty_queries() {
        let text = PageText {
            text: "İstanbul 中文 RUST rust".into(),
            spans: Vec::new(),
        };
        assert_eq!(text.search(0, "RUST").len(), 2);
        assert_eq!(text.search(0, "中文").len(), 1);
        assert_eq!(text.search(0, "i\u{307}stanbul").len(), 1);
        assert!(text.search(0, " ").is_empty());
        assert!(text.search(0, "missing").is_empty());
    }
}
