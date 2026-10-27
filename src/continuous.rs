use crate::{navigation::Fit, render_plan::RenderView};
use std::ops::Range;

pub const GAP: f32 = 24.0;

#[derive(Clone, Copy, Debug)]
pub struct Page {
    pub top: f64,
    pub size: [f32; 2],
    pub scale: f32,
}

/// Keep document distances in f64 and only convert viewport-relative distances
/// to UI coordinates. Very long documents must not lose individual scroll steps.
#[derive(Default)]
pub struct Layout {
    signature: Option<(Fit, [f32; 2], u8)>,
    pub pages: Vec<Page>,
    height: f32,
}

impl Layout {
    pub fn update(&mut self, dimensions: &[[f32; 2]], view: RenderView) {
        let signature = (view.fit, view.viewport, view.rotation);
        if self.signature == Some(signature) && self.pages.len() == dimensions.len() {
            return;
        }
        self.signature = Some(signature);
        self.height = view.viewport[1].max(1.0);
        self.pages.clear();
        let mut top = 0.0;
        for &dimensions in dimensions {
            let scale = view.scale(dimensions);
            let rotated = if view.rotation % 2 == 1 {
                [dimensions[1], dimensions[0]]
            } else {
                dimensions
            };
            let size = rotated.map(|value| value * scale);
            self.pages.push(Page { top, size, scale });
            top += f64::from(size[1]) + f64::from(GAP);
        }
    }

    pub fn maximum(&self) -> f64 {
        self.pages.last().map_or(0.0, |page| {
            // Allow even a short final page to be explicitly reached at the top.
            page.top + f64::from((page.size[1] - self.height).max(0.0))
        })
    }

    pub fn limit(&self, page: usize) -> f32 {
        self.pages.get(page).map_or(0.0, |current| {
            self.pages.get(page + 1).map_or_else(
                || (current.size[1] - self.height).max(0.0),
                |next| (next.top - current.top) as f32,
            )
        })
    }

    pub fn position(&self, page: usize, offset: f32, delta: f32) -> (usize, f32) {
        let Some(current) = self.pages.get(page) else {
            return (0, 0.0);
        };
        let top = (current.top + f64::from(offset) + f64::from(delta)).clamp(0.0, self.maximum());
        let page = self
            .pages
            .partition_point(|page| page.top <= top)
            .saturating_sub(1);
        (page, (top - self.pages[page].top) as f32)
    }

    pub fn visible(&self, page: usize, offset: f32, margin: f32) -> Range<usize> {
        let Some(current) = self.pages.get(page) else {
            return 0..0;
        };
        let top = current.top + f64::from(offset);
        let start = self
            .pages
            .partition_point(|page| page.top + f64::from(page.size[1]) < top - f64::from(margin));
        let end = self
            .pages
            .partition_point(|page| page.top <= top + f64::from(self.height) + f64::from(margin));
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(dimensions: &[[f32; 2]], fit: Fit, rotation: u8) -> Layout {
        let mut layout = Layout::default();
        layout.update(
            dimensions,
            RenderView {
                fit,
                viewport: [600.0, 700.0],
                pixels_per_point: 1.0,
                rotation,
                night: false,
                backwards: false,
            },
        );
        layout
    }

    #[test]
    fn scrolling_carries_remainder_across_mixed_pages_and_clamps_document_edges() {
        let layout = layout(
            &[[600.0, 800.0], [600.0, 300.0], [600.0, 1000.0]],
            Fit::Custom(1.0),
            0,
        );
        assert_eq!(layout.position(0, 790.0, 100.0), (1, 66.0));
        assert_eq!(layout.position(1, 66.0, -100.0), (0, 790.0));
        assert_eq!(layout.position(0, 0.0, 1300.0), (2, 152.0));
        assert_eq!(layout.position(1, 0.0, -f32::MAX), (0, 0.0));
        assert_eq!(layout.position(0, 0.0, f32::MAX), (2, 300.0));
        assert_eq!(layout.visible(0, 790.0, 0.0), 0..3);
    }

    #[test]
    fn short_pages_share_the_view_and_the_last_page_remains_reachable() {
        let layout = layout(&[[600.0, 100.0]; 20], Fit::Custom(1.0), 0);
        assert_eq!(layout.visible(0, 0.0, 0.0), 0..6);
        assert_eq!(layout.position(0, 0.0, f32::MAX), (19, 0.0));
        assert_eq!(layout.limit(19), 0.0);
    }

    #[test]
    fn rotation_fit_and_long_document_distances_stay_precise() {
        let layout = layout(&[[600.0, 800.0]; 100_000], Fit::Custom(8.0), 1);
        assert_eq!(layout.pages[0].size, [6400.0, 4800.0]);
        assert_eq!(layout.position(99_998, 4500.0, 400.0), (99_999, 76.0));
        assert_eq!(layout.visible(99_998, 4500.0, 0.0), 99_998..100_000);
        let fitted = super::tests::layout(&[[600.0, 800.0], [800.0, 600.0]], Fit::Width, 0);
        assert_eq!(fitted.pages[0].size, [600.0, 800.0]);
        assert_eq!(fitted.pages[1].size, [600.0, 450.0]);
        assert!(Layout::default().visible(0, 0.0, 0.0).is_empty());
    }
}
