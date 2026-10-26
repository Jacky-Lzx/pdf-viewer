use crate::text::PageText;
use hayro::kurbo::{Point, Rect};
use std::{ops::Range, sync::Arc};

#[derive(Clone, Copy, Default, PartialEq)]
pub enum SelectionUnit {
    #[default]
    Glyph,
    Word,
    Line,
}

#[derive(Default)]
pub struct TextSelection {
    pub active: bool,
    pub rects: Vec<Rect>,
    text: Option<Arc<PageText>>,
    anchor: Option<Point>,
    anchor_span: Option<usize>,
    head: Option<Point>,
    range: Option<Range<usize>>,
    unit: SelectionUnit,
}

impl TextSelection {
    pub fn start(&mut self) {
        *self = Self {
            active: true,
            ..Self::default()
        };
    }

    pub fn cancel(&mut self) {
        *self = Self::default();
    }

    pub fn ready(&self) -> bool {
        self.text.is_some()
    }

    pub fn set_text(&mut self, text: Arc<PageText>) {
        self.text = Some(text);
        self.anchor_span = None;
        self.range = None;
        self.rects.clear();
        self.update();
    }

    pub fn begin(&mut self, point: Point) {
        self.anchor = Some(point);
        self.anchor_span = None;
        self.head = Some(point);
        self.update();
    }

    pub fn select(&mut self, point: Point, unit: SelectionUnit) {
        self.unit = unit;
        self.begin(point);
    }

    pub fn extend(&mut self, point: Point) {
        if self.head == Some(point) {
            return;
        }
        self.head = Some(point);
        self.update();
    }

    pub fn selected_text(&self) -> Option<&str> {
        let text = self.text.as_ref()?;
        text.text.get(self.range.clone()?)
    }

    fn update(&mut self) {
        let (Some(text), Some(anchor), Some(head)) = (&self.text, self.anchor, self.head) else {
            return;
        };
        let first = self.anchor_span.or_else(|| text.nearest_span(anchor));
        self.anchor_span = first;
        let (Some(first), Some(last)) = (first, text.nearest_span(head)) else {
            return;
        };
        let mut range =
            text.spans[first.min(last)].range.start..text.spans[first.max(last)].range.end;
        if self.unit != SelectionUnit::Glyph
            && let Some((end, _)) = text.text[..range.end].char_indices().next_back()
        {
            range = if self.unit == SelectionUnit::Word {
                text.word_range(range.start).start..text.word_range(end).end
            } else {
                text.line_range(range.start).start..text.line_range(end).end
            };
            // Keep CRLF's carriage return out of copied full lines.
            if self.unit == SelectionUnit::Line && text.text[..range.end].ends_with('\r') {
                range.end -= 1;
            }
        }
        if self.range.as_ref() != Some(&range) {
            self.rects = text.rects_for_range(range.clone());
            self.range = Some(range);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::TextSpan;

    fn text() -> Arc<PageText> {
        let mut page = PageText::default();
        page.text = "Rust 中文\nreader".into();
        page.spans = page
            .text
            .char_indices()
            .filter(|(_, ch)| !ch.is_whitespace())
            .enumerate()
            .map(|(i, (start, ch))| TextSpan {
                range: start..start + ch.len_utf8(),
                rect: Rect::new(
                    (i % 6) as f64 * 10.0,
                    (i / 6) as f64 * 20.0,
                    (i % 6) as f64 * 10.0 + 10.0,
                    (i / 6) as f64 * 20.0 + 10.0,
                ),
            })
            .collect();
        Arc::new(page)
    }

    #[test]
    fn forward_and_reverse_selections_preserve_unicode_and_line_breaks() {
        let mut selection = TextSelection::default();
        selection.start();
        selection.set_text(text());
        selection.begin(Point::new(5.0, 5.0));
        selection.extend(Point::new(15.0, 25.0));
        assert_eq!(selection.selected_text(), Some("Rust 中文\nre"));
        assert_eq!(selection.rects.len(), 2);
        selection.begin(Point::new(15.0, 25.0));
        selection.extend(Point::new(5.0, 5.0));
        assert_eq!(selection.selected_text(), Some("Rust 中文\nre"));
        selection.cancel();
        assert!(!selection.active);
        assert!(selection.selected_text().is_none());
    }

    #[test]
    fn pointer_selection_survives_text_arriving_after_the_drag() {
        let mut selection = TextSelection::default();
        selection.start();
        selection.begin(Point::new(45.0, 5.0));
        selection.extend(Point::new(55.0, 5.0));
        assert!(selection.selected_text().is_none());
        selection.set_text(text());
        assert_eq!(selection.selected_text(), Some("中文"));
    }

    #[test]
    fn dragging_past_a_short_line_keeps_the_endpoint_on_that_line() {
        let mut page = PageText::default();
        page.text = "A\nB".into();
        page.spans = vec![
            TextSpan {
                range: 0..1,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            },
            TextSpan {
                range: 2..3,
                rect: Rect::new(100.0, 20.0, 110.0, 30.0),
            },
        ];
        let mut selection = TextSelection::default();
        selection.start();
        selection.set_text(Arc::new(page));
        selection.begin(Point::new(5.0, 5.0));
        selection.extend(Point::new(105.0, 5.0));
        assert_eq!(selection.selected_text(), Some("A"));
        selection.extend(Point::new(105.0, 25.0));
        assert_eq!(selection.selected_text(), Some("A\nB"));
    }

    #[test]
    fn word_and_line_drags_expand_both_ends_in_either_direction() {
        let mut selection = TextSelection::default();
        selection.start();
        selection.set_text(text());
        selection.select(Point::new(15.0, 5.0), SelectionUnit::Word);
        assert_eq!(selection.selected_text(), Some("Rust"));
        selection.extend(Point::new(15.0, 25.0));
        assert_eq!(selection.selected_text(), Some("Rust 中文\nreader"));
        selection.select(Point::new(15.0, 25.0), SelectionUnit::Word);
        selection.extend(Point::new(15.0, 5.0));
        assert_eq!(selection.selected_text(), Some("Rust 中文\nreader"));
        selection.select(Point::new(45.0, 5.0), SelectionUnit::Word);
        assert_eq!(selection.selected_text(), Some("中"));
        selection.select(Point::new(15.0, 25.0), SelectionUnit::Line);
        assert_eq!(selection.selected_text(), Some("reader"));
        selection.extend(Point::new(15.0, 5.0));
        assert_eq!(selection.selected_text(), Some("Rust 中文\nreader"));
        selection.select(Point::new(15.0, 5.0), SelectionUnit::Glyph);
        assert_eq!(selection.selected_text(), Some("u"));
    }

    #[test]
    fn unicode_words_keep_combining_marks_emoji_apostrophes_and_punctuation_boundaries() {
        let text = "cafe\u{301}, can't 中文 👩‍💻\nnext";
        let mut page = PageText::default();
        page.text = text.into();
        for (needle, expected) in [
            ("e", "cafe\u{301}"),
            ("\u{301}", "cafe\u{301}"),
            (",", ","),
            ("n'", "can't"),
            ("中", "中"),
            ("💻", "👩‍💻"),
            ("ext", "next"),
        ] {
            let index = text.find(needle).unwrap();
            assert_eq!(&text[page.word_range(index)], expected);
        }
        assert_eq!(
            page.line_range(text.len()),
            text.rfind('\n').unwrap() + 1..text.len()
        );
        assert_eq!(PageText::default().line_range(0), 0..0);
    }

    #[test]
    fn delayed_text_retains_word_selection_and_full_line_copy_omits_crlf() {
        let mut selection = TextSelection::default();
        selection.start();
        selection.select(Point::new(15.0, 5.0), SelectionUnit::Word);
        selection.extend(Point::new(15.0, 25.0));
        selection.set_text(text());
        assert_eq!(selection.selected_text(), Some("Rust 中文\nreader"));
        let mut page = PageText::default();
        page.text = "first\r\nsecond".into();
        page.spans = vec![
            TextSpan {
                range: 0..5,
                rect: Rect::new(0.0, 0.0, 40.0, 10.0),
            },
            TextSpan {
                range: 7..13,
                rect: Rect::new(0.0, 20.0, 40.0, 30.0),
            },
        ];
        selection.set_text(Arc::new(page));
        selection.select(Point::new(5.0, 5.0), SelectionUnit::Line);
        assert_eq!(selection.selected_text(), Some("first"));
        selection.extend(Point::new(5.0, 25.0));
        assert_eq!(selection.selected_text(), Some("first\r\nsecond"));
    }
}
