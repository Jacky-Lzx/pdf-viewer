use crate::{
    navigation::{Fit, MIN_SCALE},
    render_plan::RenderView,
};
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
    /// Dimensions belong to one immutable document snapshot. Reset the layout
    /// when replacing that snapshot, including reloads with the same page count.
    pub fn update(&mut self, dimensions: &[[f32; 2]], view: RenderView) {
        // Scrolling still depends on height when the page geometry does not.
        self.height = view.viewport[1].max(1.0);
        let viewport = match view.fit {
            Fit::Width => [view.viewport[0], 0.0],
            Fit::Height => [0.0, view.viewport[1]],
            Fit::Custom(scale) if scale >= MIN_SCALE => [0.0; 2],
            // Sub-minimum and NaN custom zooms use the viewport-dependent
            // minimum/fallback in Fit::scale, as does fitting the whole page.
            _ => view.viewport,
        };
        let signature = (view.fit, viewport, view.rotation % 2);
        if self.signature == Some(signature) && self.pages.len() == dimensions.len() {
            return;
        }
        self.signature = Some(signature);
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

    #[test]
    fn resized_cached_layouts_match_fresh_geometry_and_scroll_bounds() {
        let dimensions = [
            [600.0, 800.0],
            [900.0, 120.0],
            [1.0, 8_000_000.0],
            [40_000_000.0, 20_000_000.0],
            [20.0, 30.0],
        ];
        let mut cached = Layout::default();
        for fit in [
            Fit::Page,
            Fit::Width,
            Fit::Height,
            Fit::Custom(0.01),
            Fit::Custom(MIN_SCALE),
            Fit::Custom(3.0),
            Fit::Custom(f32::NAN),
            Fit::Custom(f32::INFINITY),
            Fit::Custom(f32::NEG_INFINITY),
        ] {
            for viewport in [
                [600.0, 700.0],
                [600.0, 1600.0],
                [1200.0, 1600.0],
                [1200.0, 60.0],
                [0.0, -20.0],
                [600.0, 700.0],
            ] {
                for rotation in [0, 2, 1, 3, 0] {
                    let view = RenderView {
                        fit,
                        viewport,
                        rotation,
                        pixels_per_point: 2.0,
                        night: true,
                        backwards: true,
                    };
                    cached.update(&dimensions, view);
                    let mut fresh = Layout::default();
                    fresh.update(&dimensions, view);
                    for (actual, expected) in cached.pages.iter().zip(&fresh.pages) {
                        assert_eq!(actual.top, expected.top, "{view:?}");
                        assert_eq!(actual.size, expected.size, "{view:?}");
                        assert_eq!(actual.scale, expected.scale, "{view:?}");
                    }
                    assert_eq!(cached.maximum(), fresh.maximum(), "{view:?}");
                    for page in 0..dimensions.len() {
                        assert_eq!(cached.limit(page), fresh.limit(page), "{view:?}");
                        for offset in [0.0, 60.0, cached.limit(page)] {
                            assert_eq!(
                                cached.visible(page, offset, 24.0),
                                fresh.visible(page, offset, 24.0),
                                "{view:?}"
                            );
                            for delta in [-f32::MAX, -25.0, 0.0, 120.0, f32::MAX] {
                                assert_eq!(
                                    cached.position(page, offset, delta),
                                    fresh.position(page, offset, delta),
                                    "{view:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn height_changes_update_visibility_and_the_final_page_limit_at_fixed_zoom() {
        let dimensions = [[600.0, 800.0]; 4];
        let mut cached = layout(&dimensions, Fit::Custom(1.0), 0);
        assert_eq!(cached.visible(0, 0.0, 0.0), 0..1);
        assert_eq!(cached.limit(3), 100.0);
        cached.update(
            &dimensions,
            RenderView {
                fit: Fit::Custom(1.0),
                viewport: [600.0, 1700.0],
                pixels_per_point: 1.0,
                rotation: 0,
                night: false,
                backwards: false,
            },
        );
        assert_eq!(cached.visible(0, 0.0, 0.0), 0..3);
        assert_eq!(cached.limit(3), 0.0);
        assert_eq!(cached.position(0, 0.0, f32::MAX), (3, 0.0));
        // Count changes still rebuild the geometry even at the same zoom.
        cached.update(
            &dimensions[..2],
            RenderView {
                fit: Fit::Custom(1.0),
                viewport: [600.0, 1700.0],
                pixels_per_point: 1.0,
                rotation: 0,
                night: false,
                backwards: false,
            },
        );
        assert_eq!(cached.pages.len(), 2);
        assert_eq!(cached.visible(0, 0.0, 0.0), 0..2);
        assert_eq!(cached.position(0, 0.0, f32::MAX), (1, 0.0));
    }
}
