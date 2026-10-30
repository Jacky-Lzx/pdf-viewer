//! Declarative page overlays in the same unrotated coordinates as text spans.
use serde::{Deserialize, Serialize};

pub type Rect = [f64; 4];

#[derive(Clone, Debug, Serialize)]
pub struct Line {
    pub text: String,
    pub rect: Rect,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scene {
    /// Dim every displayed page, leaving the specified holes untouched.
    pub dim: Option<[u8; 4]>,
    pub pages: Vec<PageOverlay>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageOverlay {
    pub page: usize,
    #[serde(default)]
    pub holes: Vec<Rect>,
    #[serde(default)]
    pub rectangles: Vec<Rectangle>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rectangle {
    pub rect: Rect,
    pub color: [u8; 4],
}

pub fn valid_rect(rect: Rect) -> bool {
    rect.iter().all(|x| x.is_finite() && x.abs() <= 1_000_000.0)
        && rect[0] < rect[2]
        && rect[1] < rect[3]
}

impl Scene {
    pub fn validate(&self, count: usize) -> Result<(), String> {
        if self.pages.len() > 64 {
            return Err("Too many overlay pages (maximum 64)".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for page in &self.pages {
            if page.page == 0 || page.page > count || !seen.insert(page.page) {
                return Err("Overlay pages must be unique and inside the document".into());
            }
            if page.holes.len() > 64 || page.rectangles.len() > 256 {
                return Err("Overlay exceeds 64 holes or 256 rectangles per page".into());
            }
            if page.holes.iter().any(|r| !valid_rect(*r))
                || page.rectangles.iter().any(|r| !valid_rect(r.rect))
            {
                return Err("Overlay contains an invalid rectangle".into());
            }
        }
        Ok(())
    }
}

/// Build bounded line geometry once when a page's text arrives, never per frame.
pub fn lines(text: &crate::text::PageText) -> (Vec<Line>, bool) {
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in text.text.split_inclusive('\n').take(2048) {
        ranges.push((offset, offset + line.len(), None::<hayro::kurbo::Rect>));
        offset += line.len();
    }
    for span in &text.spans {
        let index = ranges.partition_point(|(_, end, _)| *end <= span.range.start);
        if let Some((_, _, rect)) = ranges.get_mut(index)
            && span.rect.is_finite()
            && span.rect.width() > 0.0
            && span.rect.height() > 0.0
        {
            *rect = Some(rect.map_or(span.rect, |rect| rect.union(span.rect)));
        }
    }
    let truncated = offset < text.text.len();
    let lines = ranges
        .into_iter()
        .filter_map(|(start, end, rect)| {
            let rect = rect?;
            let text = &text.text[start..end];
            let mut limit = text.len().min(128);
            while !text.is_char_boundary(limit) {
                limit -= 1;
            }
            Some(Line {
                text: text[..limit].trim().into(),
                rect: [rect.x0, rect.y0, rect.x1, rect.y1],
            })
        })
        .collect();
    (lines, truncated)
}

/// Disjoint rectangles covering a dimmed page minus the union of clear holes.
pub fn mask(page: eframe::egui::Rect, holes: &[eframe::egui::Rect]) -> Vec<eframe::egui::Rect> {
    use eframe::egui::{Rect, pos2};
    if !page.is_positive() {
        return Vec::new();
    }
    let mut pieces = vec![page];
    for hole in holes {
        pieces = pieces
            .into_iter()
            .flat_map(|piece| {
                let cut = piece.intersect(*hole);
                if !cut.is_positive() {
                    return vec![piece];
                }
                [
                    Rect::from_min_max(piece.min, pos2(piece.right(), cut.top())),
                    Rect::from_min_max(pos2(piece.left(), cut.bottom()), piece.max),
                    Rect::from_min_max(
                        pos2(piece.left(), cut.top()),
                        pos2(cut.left(), cut.bottom()),
                    ),
                    Rect::from_min_max(
                        pos2(cut.right(), cut.top()),
                        pos2(piece.right(), cut.bottom()),
                    ),
                ]
                .into_iter()
                .filter(Rect::is_positive)
                .collect()
            })
            .collect();
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Rect, pos2};

    #[test]
    fn overlapping_holes_are_clear_and_remaining_mask_has_no_overdraw() {
        let page = Rect::from_min_max(pos2(0., 0.), pos2(100., 100.));
        let holes = [
            Rect::from_min_max(pos2(-10., 20.), pos2(70., 60.)),
            Rect::from_min_max(pos2(50., 40.), pos2(110., 80.)),
        ];
        let pieces = mask(page, &holes);
        for y in 0..100 {
            for x in 0..100 {
                let point = pos2(x as f32 + 0.5, y as f32 + 0.5);
                let expected = usize::from(!holes.iter().any(|r| r.contains(point)));
                assert_eq!(
                    pieces.iter().filter(|r| r.contains(point)).count(),
                    expected
                );
            }
        }
    }

    #[test]
    fn line_geometry_tracks_the_renderers_text_spans() {
        let pdf =
            crate::document::load(std::path::Path::new("assets/pagewing-guide.pdf"), "").unwrap();
        let text = crate::text::extract(
            &pdf.pages()[2],
            &hayro::hayro_interpret::InterpreterCache::new(),
        );
        let (lines, truncated) = lines(&text);
        assert!(!truncated);
        assert!(lines.len() > 6);
        assert!(
            lines
                .iter()
                .any(|line| line.text.contains("Make yourself at home"))
        );
        assert!(lines.iter().all(|line| valid_rect(line.rect)));
    }

    #[test]
    fn scenes_reject_ambiguous_pages_and_invalid_geometry() {
        for json in [
            r#"{"pages":[{"page":0}]}"#,
            r#"{"pages":[{"page":1},{"page":1}]}"#,
            r#"{"pages":[{"page":1,"holes":[[0,0,-1,10]]}]}"#,
            r#"{"pages":[{"page":1,"rectangles":[{"rect":[0,0,1e20,10],"color":[0,0,0,255]}]}]}"#,
        ] {
            assert!(
                serde_json::from_str::<Scene>(json)
                    .unwrap()
                    .validate(6)
                    .is_err()
            );
        }
    }
}
