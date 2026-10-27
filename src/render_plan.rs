use crate::{
    document::{RasterSize, RenderKey},
    navigation::Fit,
};
use eframe::egui::{Rect, Vec2};
use std::sync::Arc;

pub use crate::document::Tile;

const MAX_DETAIL_KEYS: usize = 512;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderView {
    pub fit: Fit,
    pub viewport: [f32; 2],
    pub pixels_per_point: f32,
    pub rotation: u8,
    pub night: bool,
    pub backwards: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanMode {
    Single,
    Continuous,
}

#[derive(Clone)]
pub struct PlannedView {
    pub plan: Arc<RenderPlan>,
    pub detailed: bool,
}

struct CachedView {
    mode: PlanMode,
    view: RenderView,
    visible: Vec<(usize, Rect)>,
    dimensions: Vec<[f32; 2]>,
    clip: Rect,
    budget: usize,
    result: PlannedView,
}

/// Retain a geometry allocation only while its length matches the current view.
fn update_buffer<T: Copy>(buffer: &mut Vec<T>, source: &[T]) {
    if buffer.len() == source.len() {
        buffer.copy_from_slice(source);
    } else {
        *buffer = source.to_vec();
    }
}

/// Keep only the current view's plan. Request retries and texture residency are
/// deliberately independent: a cached plan may still need to be sent again.
#[derive(Default)]
pub struct RenderPlanner {
    cached: Option<CachedView>,
}

impl RenderPlanner {
    /// Visible pages must be in contiguous document order, as in the canvas.
    /// Only their dimensions and immediate prefetch neighbors affect planning.
    pub fn plan(
        &mut self,
        mode: PlanMode,
        view: RenderView,
        visible: &[(usize, Rect)],
        dimensions: &[[f32; 2]],
        clip: Rect,
        budget: usize,
    ) -> PlannedView {
        let pages = visible
            .first()
            .zip(visible.last())
            .map_or(0..0, |(first, last)| first.0..last.0 + 1);
        let relevant = &dimensions
            [pages.start.saturating_sub(1)..pages.end.saturating_add(1).min(dimensions.len())];
        if let Some(cached) = &self.cached
            && cached.mode == mode
            && cached.view == view
            && cached.visible == visible
            && cached.dimensions == relevant
            && cached.clip == clip
            && cached.budget == budget
        {
            return cached.result.clone();
        }
        let detail = view.detail_plan(visible, dimensions, clip, budget);
        let detailed = detail.is_some();
        let plan = detail.unwrap_or_else(|| match mode {
            PlanMode::Single if !pages.is_empty() => view.plan(pages.start, dimensions, budget),
            PlanMode::Single => RenderPlan::new(),
            PlanMode::Continuous => view.continuous_plan(pages, dimensions, budget),
        });
        let result = PlannedView {
            plan: Arc::new(plan),
            detailed,
        };
        if let Some(cached) = &mut self.cached {
            cached.mode = mode;
            cached.view = view;
            update_buffer(&mut cached.visible, visible);
            update_buffer(&mut cached.dimensions, relevant);
            cached.clip = clip;
            cached.budget = budget;
            cached.result = result.clone();
        } else {
            self.cached = Some(CachedView {
                mode,
                view,
                visible: visible.to_vec(),
                dimensions: relevant.to_vec(),
                clip,
                budget,
                result: result.clone(),
            });
        }
        result
    }
}

impl RenderView {
    /// When an entire page cannot be rasterized at display density, keep a
    /// modest full-page preview and render only visible tiles at native density.
    /// Previews and tiles share the existing texture budget and cancellation.
    pub fn detail_plan(
        self,
        visible: &[(usize, Rect)],
        dimensions: &[[f32; 2]],
        clip: Rect,
        budget: usize,
    ) -> Option<RenderPlan> {
        if visible.is_empty()
            || visible.len() > MAX_DETAIL_KEYS
            || budget < 4096
            || !visible.iter().any(|&(page, _)| {
                self.key_and_size(page, dimensions[page])
                    .1
                    .is_ok_and(|raster| {
                        raster.scale < self.scale(dimensions[page]) * self.pixels_per_point * 0.99
                    })
            })
        {
            return None;
        }
        let mut preview = self;
        let preview_budget = budget / 4 / visible.len();
        let mut attempts = 0;
        while visible.iter().any(|&(page, _)| {
            preview
                .key_and_size(page, dimensions[page])
                .1
                .is_ok_and(|raster| raster.bytes() > preview_budget)
        }) {
            if attempts == 128 {
                return None;
            }
            attempts += 1;
            preview.pixels_per_point *= 0.75;
        }
        let mut density = self.pixels_per_point;
        let mut plan = RenderPlan::new();
        'density: for _ in 0..128 {
            // Failed densities share this bounded buffer instead of allocating
            // a full tile list before discovering that it exceeds the budget.
            plan.keys.clear();
            let mut bytes = 0;
            // Finish previews first so a large old fallback can be replaced
            // before the visible detail tiles reserve their share of the cache.
            for &(page, _) in visible {
                let (key, raster) = preview.key_and_size(page, dimensions[page]);
                bytes += raster.ok().map_or(0, |raster| raster.bytes());
                plan.push(key);
            }
            for &(page, rect) in visible {
                let intersection = rect.intersect(clip);
                if !intersection.is_positive() {
                    continue;
                }
                let scale = self.scale(dimensions[page]) * density;
                let extent = (rect.size() * density).ceil();
                let start = ((intersection.min - rect.min) * density).max(Vec2::ZERO);
                let end = ((intersection.max - rect.min) * density).min(extent);
                let key = self.key(page, dimensions[page]);
                const SIDE: u32 = 1024;
                for y in (start.y as u32 / SIDE)..(end.y.ceil() as u32).div_ceil(SIDE) {
                    for x in (start.x as u32 / SIDE)..(end.x.ceil() as u32).div_ceil(SIDE) {
                        let left = (x * SIDE).saturating_sub(1);
                        let top = (y * SIDE).saturating_sub(1);
                        let right = ((x + 1) * SIDE + 1).min(extent.x as u32);
                        let bottom = ((y + 1) * SIDE + 1).min(extent.y as u32);
                        let tile = Tile {
                            scale_bits: scale.to_bits(),
                            x: left,
                            y: top,
                            size: [(right - left) as u16, (bottom - top) as u16],
                        };
                        bytes += tile.bytes();
                        if bytes > budget || plan.len() >= MAX_DETAIL_KEYS {
                            density *= 0.75;
                            continue 'density;
                        }
                        plan.push(RenderKey {
                            tile: Some(tile),
                            ..key
                        });
                    }
                }
            }
            if bytes <= budget {
                return Some(plan);
            }
            // Very large/HiDPI windows still have a bounded working set.
            density *= 0.75;
        }
        None
    }

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
                tile: None,
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

    fn assert_matches_uncached(
        result: &PlannedView,
        mode: PlanMode,
        view: RenderView,
        visible: &[(usize, Rect)],
        dimensions: &[[f32; 2]],
        clip: Rect,
        budget: usize,
    ) {
        let detail = view.detail_plan(visible, dimensions, clip, budget);
        assert_eq!(result.detailed, detail.is_some());
        let expected = detail.unwrap_or_else(|| match mode {
            PlanMode::Single => view.plan(visible[0].0, dimensions, budget),
            PlanMode::Continuous => view.continuous_plan(
                visible[0].0..visible.last().unwrap().0 + 1,
                dimensions,
                budget,
            ),
        });
        assert_eq!(*result.plan, expected);
    }

    #[test]
    fn planner_reuses_the_current_plan_and_tracks_all_view_inputs() {
        let dimensions = [[600.0, 800.0]; 4];
        let clip = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::new(1000.0, 700.0));
        let rect = Rect::from_min_size(egui_pos(-1777.0, -2321.0), Vec2::new(4800.0, 6400.0));
        for mode in [PlanMode::Single, PlanMode::Continuous] {
            let mut planner = RenderPlanner::default();
            let mut view = view(Fit::Custom(8.0));
            let mut pages = vec![(1, rect)];
            let mut clip = clip;
            let mut budget = 96 * 1024 * 1024;
            let mut previous: Option<PlannedView> = None;
            for change in 0..12 {
                match change {
                    0 => {}
                    1 => view.fit = Fit::Custom(4.0),
                    2 => view.viewport[0] += 100.0,
                    3 => view.pixels_per_point = 3.0,
                    4 => view.rotation = 1,
                    5 => view.night = true,
                    6 => view.backwards = true,
                    7 => pages[0].1 = rect.translate(Vec2::new(-1024.0, 0.0)),
                    8 => clip.max.x -= 100.0,
                    9 => budget /= 4,
                    10 => pages[0].0 = 2,
                    11 => view.fit = Fit::Page,
                    _ => unreachable!(),
                }
                let result = planner.plan(mode, view, &pages, &dimensions, clip, budget);
                assert_matches_uncached(&result, mode, view, &pages, &dimensions, clip, budget);
                if let Some(previous) = &previous {
                    assert!(
                        !Arc::ptr_eq(&previous.plan, &result.plan),
                        "change {change}"
                    );
                }
                let again = planner.plan(mode, view, &pages, &dimensions, clip, budget);
                assert!(Arc::ptr_eq(&result.plan, &again.plan));
                previous = Some(result);
            }
        }
    }

    #[test]
    fn planner_tracks_visible_dimensions_neighbors_page_count_and_mode() {
        let mut dimensions = vec![[600.0, 800.0]; 4];
        let view = view(Fit::Width);
        let clip = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::new(1000.0, 700.0));
        let pages = [(1, clip), (2, clip.translate(Vec2::new(0.0, 100.0)))];
        let budget = 96 * 1024 * 1024;
        let mut planner = RenderPlanner::default();
        let mut previous = planner.plan(
            PlanMode::Continuous,
            view,
            &pages,
            &dimensions,
            clip,
            budget,
        );
        for index in [0, 1, 2, 3] {
            dimensions[index] = [300.0, 1200.0];
            let result = planner.plan(
                PlanMode::Continuous,
                view,
                &pages,
                &dimensions,
                clip,
                budget,
            );
            assert!(!Arc::ptr_eq(&previous.plan, &result.plan));
            assert_matches_uncached(
                &result,
                PlanMode::Continuous,
                view,
                &pages,
                &dimensions,
                clip,
                budget,
            );
            previous = result;
        }
        dimensions.push([800.0, 600.0]);
        let result = planner.plan(
            PlanMode::Continuous,
            view,
            &pages,
            &dimensions,
            clip,
            budget,
        );
        assert!(Arc::ptr_eq(&previous.plan, &result.plan));
        dimensions.pop();
        dimensions.pop(); // Remove the next prefetch neighbor.
        let result = planner.plan(
            PlanMode::Continuous,
            view,
            &pages,
            &dimensions,
            clip,
            budget,
        );
        assert!(!Arc::ptr_eq(&previous.plan, &result.plan));
        assert_matches_uncached(
            &result,
            PlanMode::Continuous,
            view,
            &pages,
            &dimensions,
            clip,
            budget,
        );
        let pages = &pages[..1];
        let result = planner.plan(PlanMode::Continuous, view, pages, &dimensions, clip, budget);
        let single = planner.plan(PlanMode::Single, view, pages, &dimensions, clip, budget);
        assert!(!Arc::ptr_eq(&result.plan, &single.plan));
        assert_matches_uncached(
            &single,
            PlanMode::Single,
            view,
            pages,
            &dimensions,
            clip,
            budget,
        );
    }

    #[test]
    fn changing_visible_ranges_preserves_prior_plans_and_tracks_neighbors() {
        let dimensions = [[600.0, 800.0]; 24];
        let clip = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::new(1000.0, 700.0));
        let view = view(Fit::Custom(0.05));
        let budget = 96 << 20;
        let mut planner = RenderPlanner::default();
        let mut snapshots: Vec<(Arc<RenderPlan>, RenderPlan)> = Vec::new();
        for (iteration, (start, count)) in [
            (1, 1),
            (1, 16),
            (2, 2),
            (0, 2),
            (0, 8),
            (22, 2),
            (23, 1),
            (2, 1),
        ]
        .into_iter()
        .enumerate()
        {
            let pages: Vec<_> = (start..start + count)
                .map(|page| {
                    (
                        page,
                        Rect::from_min_size(
                            egui_pos(iteration as f32, (page - start) as f32 * 64.0),
                            Vec2::new(30.0, 40.0),
                        ),
                    )
                })
                .collect();
            let result = planner.plan(
                PlanMode::Continuous,
                view,
                &pages,
                &dimensions,
                clip,
                budget,
            );
            assert_matches_uncached(
                &result,
                PlanMode::Continuous,
                view,
                &pages,
                &dimensions,
                clip,
                budget,
            );
            let again = planner.plan(
                PlanMode::Continuous,
                view,
                &pages,
                &dimensions,
                clip,
                budget,
            );
            assert!(Arc::ptr_eq(&result.plan, &again.plan));
            for (prior, expected) in &snapshots {
                assert_eq!(prior.as_ref(), expected);
            }
            snapshots.push((Arc::clone(&result.plan), result.plan.as_ref().clone()));
        }
    }

    #[test]
    fn planner_replaces_old_views_without_retaining_their_tile_lists() {
        let dimensions = [[600.0, 800.0]; 4];
        let clip = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::new(1000.0, 700.0));
        let view = view(Fit::Custom(8.0));
        let mut planner = RenderPlanner::default();
        let first = planner.plan(
            PlanMode::Single,
            view,
            &[(0, clip)],
            &dimensions,
            clip,
            96 << 20,
        );
        let weak = Arc::downgrade(&first.plan);
        drop(first);
        planner.plan(
            PlanMode::Single,
            view,
            &[(1, clip)],
            &dimensions,
            clip,
            96 << 20,
        );
        assert!(weak.upgrade().is_none());
        let result = planner.plan(PlanMode::Continuous, view, &[], &[], clip, 96 << 20);
        assert!(result.plan.is_empty());
        assert!(!result.detailed);
    }

    #[test]
    fn high_zoom_tiles_cover_the_viewport_at_native_density_within_the_shared_budget() {
        let dimensions = [[600.0, 800.0], [1200.0, 600.0]];
        let budget = 96 * 1024 * 1024;
        for rotation in 0..4 {
            let view = RenderView {
                rotation,
                ..view(Fit::Custom(8.0))
            };
            let clip = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::new(1000.0, 700.0));
            let page = Rect::from_min_size(
                egui_pos(-1777.0, -2321.0),
                if rotation % 2 == 0 {
                    Vec2::new(4800.0, 6400.0)
                } else {
                    Vec2::new(6400.0, 4800.0)
                },
            );
            let plan = view
                .detail_plan(&[(0, page)], &dimensions, clip, budget)
                .unwrap();
            let tiles: Vec<_> = plan.iter().filter_map(|key| key.tile).collect();
            assert!(!tiles.is_empty());
            assert!(tiles.iter().all(|tile| tile.scale() == 16.0));
            let bytes: usize = plan
                .iter()
                .map(|key| {
                    key.tile.map_or_else(
                        || {
                            RasterSize::new(dimensions[key.page], key.width)
                                .unwrap()
                                .bytes()
                        },
                        Tile::bytes,
                    )
                })
                .sum();
            assert!(bytes <= budget);
            // Every visible pixel is covered, including tile boundaries after panning.
            for y in [0.0, 0.5, 200.0, 699.5] {
                for x in [0.0, 0.5, 270.0, 999.5] {
                    let p = (egui_pos(x, y) - page.min) * 2.0;
                    assert!(tiles.iter().any(|tile| p.x >= tile.x as f32
                        && p.y >= tile.y as f32
                        && p.x < tile.x as f32 + f32::from(tile.size[0])
                        && p.y < tile.y as f32 + f32::from(tile.size[1])));
                }
            }
            let shifted = view
                .detail_plan(
                    &[(0, page.translate(Vec2::new(-1024.0, 0.0)))],
                    &dimensions,
                    clip,
                    budget,
                )
                .unwrap();
            assert_ne!(plan, shifted);
        }
    }

    fn egui_pos(x: f32, y: f32) -> eframe::egui::Pos2 {
        eframe::egui::pos2(x, y)
    }

    #[test]
    fn extreme_detail_requests_stay_bounded_and_cover_the_view_at_a_lower_density() {
        let dimensions = [[1_000_000.0, 1_000_000.0]];
        let view = RenderView {
            pixels_per_point: 64.0,
            viewport: [1_000_000.0; 2],
            ..view(Fit::Custom(8.0))
        };
        let clip = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::splat(1_000_000.0));
        let page = Rect::from_min_size(
            egui_pos(-2_000_000.0, -3_000_000.0),
            Vec2::splat(8_000_000.0),
        );
        // Exercise the byte limit and the independent entry limit. Exhaustively
        // building the first density would require billions of tile entries.
        for budget in [96 << 20, usize::MAX] {
            let plan = view
                .detail_plan(&[(0, page)], &dimensions, clip, budget)
                .unwrap();
            assert!(plan.len() <= MAX_DETAIL_KEYS);
            assert!(plan.keys.capacity() <= MAX_DETAIL_KEYS);
            assert!(plan[0].tile.is_none(), "The preview still has priority");
            let bytes: usize = plan
                .iter()
                .map(|key| {
                    key.tile.map_or_else(
                        || RasterSize::new(dimensions[0], key.width).unwrap().bytes(),
                        Tile::bytes,
                    )
                })
                .sum();
            assert!(bytes <= budget);
            let tiles: Vec<_> = plan.iter().filter_map(|key| key.tile).collect();
            assert!(!tiles.is_empty());
            let density = tiles[0].scale() / view.scale(dimensions[0]);
            assert!(density < view.pixels_per_point);
            assert!(tiles.iter().all(|tile| tile.scale() == tiles[0].scale()));
            for y in [0.0, 500_000.0, 999_999.0] {
                for x in [0.0, 500_000.0, 999_999.0] {
                    let point = (egui_pos(x, y) - page.min) * density;
                    assert!(tiles.iter().any(|tile| point.x >= tile.x as f32
                        && point.y >= tile.y as f32
                        && point.x < tile.x as f32 + f32::from(tile.size[0])
                        && point.y < tile.y as f32 + f32::from(tile.size[1])));
                }
            }
        }
        let visible = vec![(0, page); MAX_DETAIL_KEYS + 1];
        assert!(
            view.detail_plan(&visible, &dimensions, clip, 96 << 20)
                .is_none()
        );
    }

    #[test]
    fn details_handle_multiple_visible_pages_large_windows_and_page_edges() {
        let dimensions = [[600.0, 800.0]; 2];
        let budget = 96 * 1024 * 1024;
        let view = view(Fit::Custom(8.0));
        for size in [Vec2::new(1000.0, 700.0), Vec2::new(7680.0, 4320.0)] {
            let clip = Rect::from_min_size(egui_pos(0.0, 0.0), size);
            let pages = [
                (
                    0,
                    Rect::from_min_size(egui_pos(0.0, -6200.0), Vec2::new(4800.0, 6400.0)),
                ),
                (
                    1,
                    Rect::from_min_size(egui_pos(0.0, 224.0), Vec2::new(4800.0, 6400.0)),
                ),
            ];
            let plan = view.detail_plan(&pages, &dimensions, clip, budget).unwrap();
            assert_eq!(
                plan[..2].iter().map(|key| key.page).collect::<Vec<_>>(),
                [0, 1]
            );
            let bytes: usize = plan
                .iter()
                .map(|key| {
                    key.tile.map_or_else(
                        || {
                            RasterSize::new(dimensions[key.page], key.width)
                                .unwrap()
                                .bytes()
                        },
                        Tile::bytes,
                    )
                })
                .sum();
            assert!(bytes <= budget);
            for key in plan.iter().filter(|key| key.tile.is_some()) {
                let tile = key.tile.unwrap();
                assert!(!tile.size.contains(&0));
                assert!(tile.x + u32::from(tile.size[0]) <= (600.0 * tile.scale()).ceil() as u32);
                assert!(tile.y + u32::from(tile.size[1]) <= (800.0 * tile.scale()).ceil() as u32);
            }
        }
        let rect = Rect::from_min_size(egui_pos(0.0, 0.0), Vec2::new(600.0, 800.0));
        assert!(
            view.detail_plan(&[(0, rect)], &dimensions, rect, 4096)
                .is_none()
        );
        assert!(
            super::tests::view(Fit::Page)
                .detail_plan(&[(0, rect)], &dimensions, rect, budget)
                .is_none()
        );
    }

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
