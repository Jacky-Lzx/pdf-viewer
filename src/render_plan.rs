use crate::{
    document::{RasterSize, RenderKey},
    navigation::Fit,
};

/// Current page plus at most two neighbors, stored inline for per-frame planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderPlan {
    keys: [RenderKey; 3],
    len: usize,
}

impl RenderPlan {
    fn new() -> Self {
        Self {
            keys: [RenderKey {
                page: 0,
                width: 0,
                rotation: 0,
                night: false,
            }; 3],
            len: 0,
        }
    }

    fn push(&mut self, key: RenderKey) {
        self.keys[self.len] = key;
        self.len += 1;
    }
}

impl std::ops::Deref for RenderPlan {
    type Target = [RenderKey];

    fn deref(&self) -> &Self::Target {
        &self.keys[..self.len]
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
        let width = (dimensions[0] * self.scale(dimensions) * self.pixels_per_point)
            .clamp(1.0, 16384.0)
            .ceil() as u32;
        RenderKey {
            page,
            width: width.div_ceil(32) * 32,
            rotation: self.rotation,
            night: self.night,
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
            let key = self.key(candidate, size);
            let Ok(raster) = RasterSize::new(size, key.width) else {
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
    fn mixed_size_prefetch_matches_the_key_after_navigation() {
        let pages = [[600.0, 800.0], [1200.0, 600.0], [500.0, 1000.0]];
        for fit in [Fit::Page, Fit::Width, Fit::Custom(1.5)] {
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
