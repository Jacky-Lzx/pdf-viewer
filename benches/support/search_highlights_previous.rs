// Baseline: SearchHighlights at 4670ef1, before neighbor reuse.
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
