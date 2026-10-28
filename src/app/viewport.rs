use super::Pagewing;
use eframe::egui::Vec2;

impl Pagewing {
    pub(super) fn resize_viewport(&mut self, viewport: Vec2) {
        if self.viewport != viewport {
            // Preserve the page-relative position before resolve_view changes
            // scale and overflow. Explicit history/search/link targets retain
            // precedence over the old viewport's scroll position.
            if self.pending_offset.is_none()
                && self.pending_focus.is_none()
                && self.pending_destination.is_none()
            {
                self.pending_offset = Some(self.current_offset());
            }
            self.viewport = viewport;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{Fit, Navigation};
    use eframe::egui::{Context, Pos2, RawInput, Rect};

    fn frame(app: &mut Pagewing, context: &Context, size: Vec2) {
        let _ = context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                ..Default::default()
            },
            |context| {
                app.toolbar(context);
                app.search_bar(context);
                app.status(context);
                app.canvas(context);
            },
        );
    }

    fn assert_offset(app: &Pagewing, expected: [f32; 2]) {
        for (actual, expected) in app.current_offset().into_iter().zip(expected) {
            assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
        }
    }

    #[test]
    fn resizing_real_canvases_preserves_position_and_explicit_history_targets() {
        for (continuous, facing) in [(false, false), (true, false), (false, true)] {
            for fit in [Fit::Width, Fit::Custom(3.0)] {
                let root = tempfile::tempdir().unwrap();
                let context = Context::default();
                let mut app =
                    Pagewing::with_context(&context, None, None, root.path().join("library.json"));
                app.nav = Navigation::new(6, 2);
                app.dimensions = vec![[600.0, 800.0]; 6];
                app.reading.fit = fit;
                app.set_display_mode(continuous, facing);
                let wide = Vec2::new(1180.0, 820.0);
                frame(&mut app, &context, wide);
                app.pending_offset = Some([0.3, 0.6]);
                frame(&mut app, &context, wide);
                let expected = app.current_offset();
                for size in [Vec2::splat(240.0), wide, Vec2::splat(240.0), wide] {
                    frame(&mut app, &context, size);
                    assert_eq!(app.nav.page, 2);
                    assert_offset(&app, expected);
                }
                // A search bar also changes the viewport; it must not shift
                // the page while typing or override an explicit restoration.
                app.search_open = true;
                frame(&mut app, &context, wide);
                assert_offset(&app, expected);
                let target = crate::state::ReadingMark {
                    page: 0,
                    fit,
                    rotation: 0,
                    continuous,
                    facing,
                    offset: [0.7, 0.8],
                };
                app.restore_position(target, false);
                frame(&mut app, &context, Vec2::splat(240.0));
                assert_eq!(app.nav.page, 0);
                let expected = [if fit == Fit::Width { 0.0 } else { 0.7 }, 0.8];
                assert_offset(&app, expected);
            }
        }
    }
}
