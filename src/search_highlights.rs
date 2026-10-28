//! Bounded screen geometry for immutable search regions between result batches.
use crate::{
    text::{SearchHit, page_hits},
    visible_rects::{PageView, VisibleRects},
};
use eframe::egui::Rect;

#[derive(Default)]
pub struct SearchHighlights {
    current: VisibleRects<usize, false>,
    other: VisibleRects<usize, false>,
}

impl SearchHighlights {
    /// Accepted result batches can add regions without changing the view.
    pub fn invalidate(&mut self) {
        self.current.invalidate();
        self.other.invalidate();
    }

    /// Query and document changes release geometry owned by the old results.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn clear_other(&mut self) {
        self.other = VisibleRects::default();
    }

    pub fn visible(
        &mut self,
        hits: &[SearchHit],
        view: PageView,
        active_page: usize,
    ) -> impl Iterator<Item = (usize, Rect)> + '_ {
        let visible = if view.page == active_page {
            &mut self.current
        } else {
            // Neighboring pages share one allocation, never a document-sized
            // cache. Their contents are rebuilt even when drawn consecutively.
            self.other.invalidate();
            &mut self.other
        };
        visible.update_keyed(
            view,
            page_hits(hits, view.page)
                .flat_map(|index| hits[index].rects.iter().map(move |&rect| (index, rect))),
        );
        visible
            .indices
            .iter()
            .zip(&visible.rects)
            .filter_map(move |(&index, &rect)| {
                (rect.is_positive() && view.clip.intersects(rect)).then_some((index, rect))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Vec2, pos2};
    use hayro::kurbo::Rect as PdfRect;

    fn hit(page: usize, rect: PdfRect) -> SearchHit {
        SearchHit {
            page,
            rects: vec![rect],
            snippet: String::new(),
            snippet_match: 0..0,
        }
    }

    #[test]
    fn current_geometry_survives_neighbor_draws_and_all_buffers_release_at_boundaries() {
        let view = PageView {
            document: 1,
            page: 1,
            page_rect: Rect::from_min_size(pos2(20.0, 30.0), Vec2::new(600.0, 800.0)),
            clip: Rect::from_min_max(pos2(0.0, 0.0), pos2(500.0, 500.0)),
            dimensions: [600.0, 800.0],
            rotation: 0,
            scale: 1.0,
        };
        let mut hits = vec![
            hit(0, PdfRect::new(20.0, 50.0, 100.0, 70.0)),
            hit(1, PdfRect::new(40.0, 70.0, 120.0, 90.0)),
            hit(2, PdfRect::new(60.0, 90.0, 140.0, 110.0)),
        ];
        let mut cache = SearchHighlights::default();
        let current: Vec<_> = cache.visible(&hits, view, 1).collect();
        let allocation = cache.current.rects.as_ptr();
        for page in [0, 2, 0, 1] {
            let regions: Vec<_> = cache.visible(&hits, PageView { page, ..view }, 1).collect();
            assert_eq!(regions.len(), 1);
            assert_eq!(regions[0].0, page);
            assert_eq!(cache.current.rects.as_ptr(), allocation);
            assert_eq!(cache.current.indices, [1]);
        }
        assert_eq!(cache.visible(&hits, view, 1).collect::<Vec<_>>(), current);
        hits[1].rects[0] = PdfRect::new(80.0, 120.0, 160.0, 140.0);
        cache.invalidate();
        assert_ne!(cache.visible(&hits, view, 1).collect::<Vec<_>>(), current);
        assert!(cache.other.rects.capacity() > 0);
        cache.clear_other();
        assert_eq!(cache.other.rects.capacity(), 0);
        assert_eq!(cache.other.indices.capacity(), 0);
        assert!(!cache.current.rects.is_empty());
        cache.clear();
        assert_eq!(cache.current.rects.capacity(), 0);
        assert_eq!(cache.current.indices.capacity(), 0);
        assert!(cache.visible(&[], view, 1).next().is_none());
    }
}
