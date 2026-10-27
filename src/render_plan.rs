use crate::{
    document::{RasterSize, RenderKey},
    navigation::Fit,
};

/// Visible pages followed by optional neighbors, in render priority order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderPlan {
    keys: Vec<RenderKey>,
}

impl RenderPlan {
    fn new() -> Self {
        Self { keys: Vec::new() }
    }

    fn push(&mut self, key: RenderKey) {
        self.keys.push(key);
    }
}

impl std::ops::Deref for RenderPlan {
    type Target = [RenderKey];

    fn deref(&self) -> &Self::Target {
        &self.keys
    }
}

#[derive(Clone, Copy)]
pub struct RenderView {
    pub fit: Fit,
    pub viewport: [f32; 2],
    pub pixels_per_point: f32,
    pub rotation: u8,
    pub night: bool,
    pub backwards: bool,
}

impl RenderView {
    pub fn scale(self, dimensions: [f32; 2]) -> f32 {
        let rotated = if self.rotation % 2 == 1 {
            [dimensions[1], dimensions[0]]
        } else {
            dimensions
        };
        self.fit.scale(rotated, self.viewport)
    }

    pub fn key(self, page: usize, dimensions: [f32; 2]) -> RenderKey {
        self.key_and_size(page, dimensions).0
    }

    fn key_and_size(
        self,
        page: usize,
        dimensions: [f32; 2],
    ) -> (RenderKey, Result<RasterSize, String>) {
        let width = (dimensions[0] * self.scale(dimensions) * self.pixels_per_point)
            .clamp(1.0, 16384.0)
            .ceil() as u32;
        let width = width.div_ceil(32) * 32;
        let raster = RasterSize::new(dimensions, width);
        // Requests beyond the pixel/side limit produce identical rasters. Use
        // one key, keeping a bucket strictly above the actual width so that
        // rasterization still selects exactly the same limiting scale.
        let width = raster.as_ref().map_or(width, |raster| {
            width.min((raster.size[0] as u32 / 32 + 1) * 32)
        });
        (
            RenderKey {
                page,
                width,
                rotation: self.rotation,
                night: self.night,
            },
            raster,
        )
    }

    /// Reduce raster resolution together when many pages share a viewport,
    /// reserving enough cache space for every visible page before prefetching.
    pub fn continuous_plan(
        self,
        visible: std::ops::Range<usize>,
        dimensions: &[[f32; 2]],
        budget: usize,
    ) -> RenderPlan {
        let mut view = self;
        loop {
            let bytes: usize = visible
                .clone()
                .filter_map(|page| {
                    view.key_and_size(page, dimensions[page])
                        .1
                        .ok()
                        .map(|raster| raster.bytes())
                })
                .sum();
            if bytes <= budget || view.pixels_per_point <= 0.01 {
                let mut plan = RenderPlan::new();
                for page in visible.clone() {
                    plan.push(view.key(page, dimensions[page]));
                }
                let mut remaining = budget.saturating_sub(bytes);
                let neighbors = if self.backwards {
                    [visible.start.checked_sub(1), Some(visible.end)]
                } else {
                    [Some(visible.end), visible.start.checked_sub(1)]
                };
                for page in neighbors.into_iter().flatten() {
                    if let Some(&size) = dimensions.get(page) {
                        let (key, raster) = view.key_and_size(page, size);
                        if let Ok(raster) = raster
                            && raster.bytes() <= remaining
                        {
                            remaining -= raster.bytes();
                            plan.push(key);
                        }
                    }
                }
                return plan;
            }
            view.pixels_per_point *= 0.75;
        }
    }

    /// Reserve the visible raster first, then admit neighbors that can coexist with it.
    /// Include cached pages in the reservation to prevent neighbors evicting one another.
    pub fn plan(self, page: usize, dimensions: &[[f32; 2]], budget: usize) -> RenderPlan {
        let mut remaining = budget;
        let mut keys = RenderPlan::new();
        let neighbors = if self.backwards {
            [page.checked_sub(1), page.checked_add(1)]
        } else {
            [page.checked_add(1), page.checked_sub(1)]
        };
        for candidate in [Some(page), neighbors[0], neighbors[1]]
            .into_iter()
            .flatten()
        {
            let Some(&size) = dimensions.get(candidate) else {
                continue;
            };
            let (key, raster) = self.key_and_size(candidate, size);
            let Ok(raster) = raster else {
                // Let the visible render report invalid aspect ratios through the worker.
                if candidate == page {
                    keys.push(key);
                }
                continue;
            };
            let bytes = raster.bytes();
            if candidate == page || bytes <= remaining {
                remaining = remaining.saturating_sub(bytes);
                keys.push(key);
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(fit: Fit) -> RenderView {
        RenderView {
            fit,
            viewport: [1000.0, 700.0],
            pixels_per_point: 2.0,
            rotation: 0,
            night: false,
            backwards: false,
        }
    }

    #[test]
    fn continuous_renders_every_visible_page_before_neighbors_within_budget() {
        let pages = [[600.0, 800.0]; 10];
        for fit in [Fit::Page, Fit::Width, Fit::Custom(8.0)] {
            let view = view(fit);
            let budget = 96 * 1024 * 1024;
            let plan = view.continuous_plan(2..6, &pages, budget);
            assert_eq!(
                plan[..4].iter().map(|key| key.page).collect::<Vec<_>>(),
                [2, 3, 4, 5]
            );
            let bytes: usize = plan
                .iter()
                .map(|key| RasterSize::new(pages[key.page], key.width).unwrap().bytes())
                .sum();
            assert!(bytes <= budget, "{bytes} > {budget}");
            assert!(
                plan.iter()
                    .all(|key| key.rotation == view.rotation && key.night == view.night)
            );
        }
        let backward = RenderView {
            backwards: true,
            ..view(Fit::Page)
        };
        let plan = backward.continuous_plan(2..4, &pages, 96 * 1024 * 1024);
        assert_eq!(
            plan.iter().map(|key| key.page).collect::<Vec<_>>(),
            [2, 3, 1, 4]
        );
    }

    #[test]
    fn mixed_size_prefetch_matches_the_key_after_navigation() {
        let pages = [[600.0, 800.0], [1200.0, 600.0], [500.0, 1000.0]];
        for fit in [Fit::Page, Fit::Width, Fit::Height, Fit::Custom(1.5)] {
            for rotation in 0..4 {
                let view = RenderView {
                    rotation,
                    ..view(fit)
                };
                let plan = view.plan(1, &pages, 96 * 1024 * 1024);
                assert_eq!(plan.len(), 3);
                for &key in plan.iter() {
                    assert_eq!(key, view.plan(key.page, &pages, 96 * 1024 * 1024)[0]);
                }
            }
        }
        let plan = view(Fit::Page).plan(0, &pages, 96 * 1024 * 1024);
        assert_ne!(plan[0].width, plan[1].width);
    }

    #[test]
    fn high_zoom_skips_rasters_that_would_be_discarded() {
        let pages = [[600.0, 800.0]; 3];
        let budget = 96 * 1024 * 1024;
        let plan = view(Fit::Custom(8.0)).plan(1, &pages, budget);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].page, 1);
        let normal = view(Fit::Page).plan(1, &pages, budget);
        assert_eq!(normal.len(), 3);
        let bytes: usize = normal
            .iter()
            .map(|key| RasterSize::new(pages[key.page], key.width).unwrap().bytes())
            .sum();
        assert!(bytes <= budget);
    }

    #[test]
    fn zooms_above_the_raster_limit_share_a_key_without_changing_pixels() {
        let dimensions = [600.0, 800.0];
        let mut keys = Vec::new();
        for (zoom, old_width) in [(5.0, 6016), (6.0, 7200), (7.0, 8416), (8.0, 9600)] {
            let key = view(Fit::Custom(zoom)).key(0, dimensions);
            let requested = RasterSize::new(dimensions, old_width).unwrap();
            let actual = RasterSize::new(dimensions, key.width).unwrap();
            assert_eq!(actual.size, requested.size);
            assert_eq!(actual.scale.to_bits(), requested.scale.to_bits());
            keys.push(key);
        }
        assert!(keys.iter().all(|key| *key == keys[0]), "{keys:?}");
        assert_eq!(view(Fit::Custom(1.0)).key(0, dimensions).width, 1216);
        assert_eq!(view(Fit::Custom(2.0)).key(0, dimensions).width, 2400);
    }

    #[test]
    fn canonical_keys_match_planned_rasters_at_pixel_and_side_boundaries() {
        for dimensions in [
            [600.0, 800.0],
            [1000.0, 1000.0],
            [4.0, 10000.0],
            [10000.0, 4.0],
            [600000.0, 800000.0],
            [f32::MAX; 2],
        ] {
            for fit in [
                Fit::Page,
                Fit::Width,
                Fit::Height,
                Fit::Custom(0.05),
                Fit::Custom(1.0),
                Fit::Custom(4.0),
                Fit::Custom(8.0),
            ] {
                for rotation in 0..4 {
                    for pixels_per_point in [0.5, 1.0, 1.25, 2.0, 4.0] {
                        let view = RenderView {
                            fit,
                            rotation,
                            pixels_per_point,
                            ..view(fit)
                        };
                        let (key, planned) = view.key_and_size(0, dimensions);
                        let actual = RasterSize::new(dimensions, key.width);
                        let signature = |raster: Result<RasterSize, String>| {
                            raster
                                .map(|raster| (raster.size, raster.scale.to_bits()))
                                .map_err(|_| ())
                        };
                        assert_eq!(
                            signature(actual),
                            signature(planned),
                            "{dimensions:?}, {fit:?}, rotation {rotation}, dpi {pixels_per_point}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn limited_prefetch_prioritizes_the_reading_direction() {
        let pages = [[600.0, 800.0]; 3];
        let forward = view(Fit::Page);
        let bytes = RasterSize::new(pages[1], forward.key(1, pages[1]).width)
            .unwrap()
            .bytes();
        assert_eq!(
            forward
                .plan(1, &pages, bytes * 2)
                .iter()
                .map(|key| key.page)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        let backward = RenderView {
            backwards: true,
            ..forward
        };
        assert_eq!(
            backward
                .plan(1, &pages, bytes * 2)
                .iter()
                .map(|key| key.page)
                .collect::<Vec<_>>(),
            [1, 0]
        );
    }

    #[test]
    fn a_smaller_neighbor_can_fit_after_a_larger_one_is_skipped() {
        let pages = [[100.0, 100.0], [600.0, 800.0], [600.0, 800.0]];
        let plan = view(Fit::Custom(8.0)).plan(1, &pages, 96 * 1024 * 1024);
        assert_eq!(plan.iter().map(|key| key.page).collect::<Vec<_>>(), [1, 0]);
    }

    #[test]
    fn inline_plans_expose_only_initialized_keys_at_document_edges() {
        let view = view(Fit::Page);
        let pages = [[600.0, 800.0]; 3];
        assert!(view.plan(0, &[], 0).is_empty());
        for (page, expected) in [(0, vec![0, 1]), (2, vec![2, 1])] {
            let plan = view.plan(page, &pages, 96 * 1024 * 1024);
            assert_eq!(plan.len(), 2);
            assert_eq!(
                plan.iter().map(|key| key.page).collect::<Vec<_>>(),
                expected
            );
            assert_eq!(plan, view.plan(page, &pages, 96 * 1024 * 1024));
        }
        let plan = view.plan(1, &pages, 0);
        assert_eq!(plan.len(), 1, "always request the visible page");
        assert_eq!(plan.first().unwrap().page, 1);
        let invalid = view.plan(0, &[[1.0e38, 1.0]], 0);
        assert_eq!(
            invalid.len(),
            1,
            "let the worker report invalid aspect ratios"
        );
    }
}
