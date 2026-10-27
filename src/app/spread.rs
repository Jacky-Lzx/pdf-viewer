use super::Pagewing;
use crate::navigation::Fit;
use eframe::egui::{self, Vec2};
use std::ops::Range;

const GAP: f32 = 24.0;

/// Geometry shared by rendering, panning and zooming both pages together.
pub(super) struct Spread {
    pub pages: Range<usize>,
    pub scale: f32,
    pub size: Vec2,
    first_width: f32,
}

impl Spread {
    pub fn new(
        dimensions: &[[f32; 2]],
        page: usize,
        rotation: u8,
        fit: Fit,
        viewport: Vec2,
    ) -> Self {
        let start = page / 2 * 2;
        let end = (start + 2).min(dimensions.len());
        let rotated = |page: usize| {
            let d = dimensions[page];
            if rotation % 2 == 1 {
                Vec2::new(d[1], d[0])
            } else {
                Vec2::from(d)
            }
        };
        let first = rotated(start);
        let second = (end > start + 1).then(|| rotated(start + 1));
        let gap = if second.is_some() { GAP } else { 0.0 };
        let dimensions = first + Vec2::new(second.map_or(0.0, |size| size.x), 0.0);
        let height = first.y.max(second.map_or(0.0, |size| size.y));
        let scale = fit.scale(
            [dimensions.x, height],
            [(viewport.x - gap).max(1.0), viewport.y],
        );
        Self {
            pages: start..end,
            scale,
            size: Vec2::new(dimensions.x * scale + gap, height * scale),
            first_width: first.x * scale,
        }
    }

    pub fn offset(&self, page: usize) -> Vec2 {
        Vec2::new(
            if page > self.pages.start {
                self.first_width + GAP
            } else {
                0.0
            },
            0.0,
        )
    }

    pub fn origin(&self, viewport: Vec2, pan: Vec2) -> Vec2 {
        (viewport - self.size).max(Vec2::ZERO) * 0.5 - pan
    }
}

impl Pagewing {
    pub(super) fn spread(&self) -> Spread {
        Spread::new(
            &self.dimensions,
            self.nav.page,
            self.reading.rotation,
            self.reading.fit,
            self.viewport,
        )
    }

    pub(super) fn set_display_mode(&mut self, continuous: bool, facing: bool) {
        self.resolve_view();
        self.reading.continuous = continuous;
        self.reading.facing = facing;
        if !continuous && !facing {
            self.continuous_geometry = crate::page_geometry::PageGeometry::default();
        }
        self.page_changed();
        self.continuous_link_request = None;
        self.resolve_view();
    }

    pub(super) fn zoom_spread(&mut self, target: f32, pivot: Vec2) {
        let before = self.spread();
        let local = pivot - before.origin(self.viewport, self.pan);
        // The gutter stays at a fixed UI width as the pages scale.
        let gutter = if local.x > before.first_width {
            GAP
        } else {
            0.0
        };
        self.reading.fit = Fit::Custom(target);
        let after = self.spread();
        let scaled = Vec2::new(
            (local.x - gutter) * after.scale / before.scale + gutter,
            local.y * after.scale / before.scale,
        );
        self.pan = after.origin(self.viewport, Vec2::ZERO) + scaled - pivot;
        self.resolve_view();
        self.dirty = true;
    }

    pub(super) fn spread_rect(&self, page: usize, viewport: egui::Rect) -> egui::Rect {
        let spread = self.spread();
        let dimensions = self.dimensions[page];
        let size = if self.reading.rotation % 2 == 1 {
            Vec2::new(dimensions[1], dimensions[0])
        } else {
            Vec2::from(dimensions)
        } * spread.scale;
        egui::Rect::from_min_size(
            viewport.min + spread.origin(viewport.size(), self.pan) + spread.offset(page),
            size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(config: std::path::PathBuf) -> (Pagewing, egui::Context) {
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(&context, None, None, config);
        app.dimensions = vec![[600.0, 800.0], [900.0, 300.0], [600.0, 800.0]];
        app.nav = crate::navigation::Navigation::new(3, 0);
        app.viewport = Vec2::new(1000.0, 700.0);
        app.set_display_mode(false, true);
        (app, context)
    }

    #[test]
    fn spread_navigation_restores_modes_and_positions_through_history_and_marks() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, _) = reader(directory.path().join("library.json"));
        app.step(1);
        assert_eq!(app.nav.page, 2);
        app.step(1);
        assert_eq!(app.nav.page, 2);
        app.step(-1);
        assert_eq!(app.nav.page, 0);
        app.jump(1);
        assert_eq!(app.spread().pages, 0..2);
        app.step(isize::MAX);
        assert_eq!(app.nav.page, 2);
        app.step(isize::MIN);
        assert_eq!(app.nav.page, 0);

        app.actual_size();
        app.pan = Vec2::new(250.0, 50.0);
        let mark = app.current_position();
        assert!(mark.facing);
        app.set_display_mode(true, false);
        assert!(app.reading.continuous && !app.reading.facing);
        app.restore_position(mark.clone(), true);
        app.resolve_view();
        assert!(app.reading.facing && !app.reading.continuous);
        assert_eq!(app.pan, Vec2::new(250.0, 50.0));
        assert!(app.history(false));
        assert!(app.reading.continuous && !app.reading.facing);
        let json = serde_json::to_string(&mark).unwrap();
        let saved: crate::state::ReadingMark = serde_json::from_str(&json).unwrap();
        assert!(saved.facing);
        assert_eq!(saved.offset, mark.offset);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer_on_either_page_in_every_rotation() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, _) = reader(directory.path().join("library.json"));
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, app.viewport);
        for rotation in 0..4 {
            for page in 0..2 {
                app.reading.rotation = rotation;
                app.reading.fit = Fit::Page;
                app.pan = Vec2::ZERO;
                app.resolve_view();
                let before = app.spread_rect(page, viewport);
                let pivot = before.center().to_vec2();
                let point = (pivot - before.min.to_vec2()) / app.scale;
                app.zoom_at(4.0, pivot);
                let after = app.spread_rect(page, viewport);
                let restored = (pivot - after.min.to_vec2()) / app.scale;
                assert!(
                    (point - restored).length() < 0.001,
                    "rotation {rotation}, page {page}"
                );
            }
        }
    }

    #[test]
    fn canvas_requests_both_visible_pages_and_one_page_on_the_final_spread() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        for (page, expected) in [(0, vec![0, 1]), (2, vec![2])] {
            app.jump(page);
            let _ = context.run(egui::RawInput::default(), |context| app.canvas(context));
            let plan = app.requested.as_ref().unwrap();
            assert_eq!(
                plan.iter()
                    .take(expected.len())
                    .map(|key| key.page)
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(!app.reading.continuous);
        }
    }

    #[test]
    fn clearing_requests_retries_a_cached_plan_in_every_display_mode() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        for (continuous, facing) in [(false, false), (true, false), (false, true)] {
            app.set_display_mode(continuous, facing);
            for _ in 0..3 {
                let _ = context.run(egui::RawInput::default(), |context| app.canvas(context));
            }
            let requested = app.requested.take().unwrap();
            let _ = context.run(egui::RawInput::default(), |context| app.canvas(context));
            assert!(std::sync::Arc::ptr_eq(
                &requested,
                app.requested.as_ref().unwrap()
            ));
            app.clear_document(&context);
            assert!(app.requested.is_none());
            app.dimensions = vec![[600.0, 800.0]; 3];
            app.nav = crate::navigation::Navigation::new(3, 0);
        }
    }

    #[test]
    fn search_and_rectangular_destinations_use_the_second_pages_position_before_clamping() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, _) = reader(directory.path().join("library.json"));
        app.jump(1);
        app.actual_size();
        app.pending_focus = Some(hayro::kurbo::Rect::new(10.0, 50.0, 50.0, 70.0));
        app.resolve_view();
        assert_eq!(app.pan.x, 334.0); // 624 + 10 - 30% of viewport width.

        let destination = crate::destination::Destination {
            fit: Some(crate::destination::DestinationFit::Rectangle(
                hayro::kurbo::Rect::new(10.0, 50.0, 50.0, 70.0),
            )),
            ..crate::destination::Destination::page(1)
        };
        app.pending_destination = Some((destination, [0.0; 2]));
        app.resolve_view();
        assert_eq!(app.pan.x, 154.0); // Center 624 + 30 in the viewport.
    }

    #[test]
    fn mixed_pages_fit_together_at_the_same_scale_and_keep_a_fixed_gutter() {
        let dimensions = [[600.0, 800.0], [900.0, 300.0], [600.0, 800.0]];
        let viewport = Vec2::new(1000.0, 700.0);
        for rotation in 0..4 {
            let spread = Spread::new(&dimensions, 1, rotation, Fit::Page, viewport);
            assert_eq!(spread.pages, 0..2);
            assert!(spread.size.x <= viewport.x + 0.001);
            assert!(spread.size.y <= viewport.y + 0.001);
            assert_eq!(spread.offset(1).x - spread.first_width, GAP);
        }
        let width = Spread::new(&dimensions, 0, 0, Fit::Width, viewport);
        assert!((width.size.x - viewport.x).abs() < 0.001);
        let height = Spread::new(&dimensions, 0, 0, Fit::Height, viewport);
        assert!((height.size.y - viewport.y).abs() < 0.001);
        let last = Spread::new(&dimensions, 2, 0, Fit::Page, viewport);
        assert_eq!(last.pages, 2..3);
        assert_eq!(last.size.x, 600.0 * last.scale);
        assert!(last.origin(viewport, Vec2::ZERO).x > 0.0);
    }
}
