use super::{Pagewing, SELECTION_COLOR, selection_scroll};
use crate::{
    navigation::Fit,
    render_plan::{PlanMode, RenderView},
    selection::SelectionUnit,
};
use eframe::egui::{self, Color32, Vec2};
use std::time::Duration;

impl Pagewing {
    pub(super) fn clear_multi_page_buffers(&mut self) {
        self.continuous_geometry = crate::page_geometry::PageGeometry::default();
        self.other_visible_links = crate::link_visibility::LinkVisibility::default();
    }

    fn continuous_view(&self, pixels_per_point: f32) -> RenderView {
        RenderView {
            fit: if self.reading.facing {
                Fit::Custom(self.scale)
            } else {
                self.reading.fit
            },
            viewport: [self.viewport.x, self.viewport.y],
            pixels_per_point,
            rotation: self.reading.rotation,
            night: self.library.night,
            backwards: self.nav.direction < 0,
        }
    }

    pub(super) fn update_continuous_layout(&mut self) {
        let view = self.continuous_view(1.0);
        self.continuous_layout.update(&self.dimensions, view);
    }

    pub(super) fn toggle_continuous(&mut self) {
        self.set_display_mode(!self.reading.continuous, false);
    }

    fn continuous_position(&mut self, page: usize, offset: f32) {
        if self.nav.jump(page) {
            self.links.set_page(page);
            self.reveal_page = Some(page);
            self.visible_links.clear();
            self.hints.cancel();
            self.requested = None;
            self.search_navigation.clear();
        }
        self.pan.y = offset;
        self.resolve_view();
        self.dirty = true;
    }

    pub(super) fn scroll_continuous(&mut self, delta: Vec2) {
        let (page, offset) = self
            .continuous_layout
            .position(self.nav.page, self.pan.y, delta.y);
        self.pan.x = (self.pan.x + delta.x).clamp(0.0, self.overflow.x);
        self.continuous_position(page, offset);
    }

    pub(super) fn zoom_continuous(&mut self, target: f32, pivot: Vec2) {
        let (anchor, local) = self
            .continuous_layout
            .position(self.nav.page, self.pan.y, pivot.y);
        let before = self.continuous_layout.pages[anchor].scale;
        let current = self.continuous_layout.pages[self.nav.page];
        let old_margin = (self.viewport.x - current.size[0]).max(0.0) * 0.5;
        let local_x = (self.pan.x + pivot.x - old_margin) / current.scale;
        self.reading.fit = Fit::Custom(target);
        self.update_continuous_layout();
        let after = self.continuous_layout.pages[anchor].scale;
        let (page, offset) =
            self.continuous_layout
                .position(anchor, local * after / before, -pivot.y);
        let current = self.continuous_layout.pages[page];
        let new_margin = (self.viewport.x - current.size[0]).max(0.0) * 0.5;
        self.pan.x = (local_x * current.scale + new_margin - pivot.x).max(0.0);
        self.continuous_position(page, offset);
    }

    fn continuous_rect(&self, page: usize, viewport: egui::Rect) -> egui::Rect {
        if self.reading.facing {
            return self.spread_rect(page, viewport);
        }
        let layout = self.continuous_layout.pages[page];
        let current = self.continuous_layout.pages[self.nav.page];
        let x = viewport.left() + (viewport.width() - layout.size[0]).max(0.0) * 0.5 - self.pan.x;
        let y = viewport.top() + (layout.top - current.top - f64::from(self.pan.y)) as f32;
        egui::Rect::from_min_size(egui::pos2(x, y), Vec2::from(layout.size))
    }

    fn visible_pages(&self) -> std::ops::Range<usize> {
        if self.reading.facing {
            self.spread().pages
        } else {
            self.continuous_layout
                .visible(self.nav.page, self.pan.y, 24.0)
        }
    }

    pub(super) fn continuous_canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect().shrink(24.0);
        self.viewport = rect.size().max(Vec2::splat(1.0));
        if self.resolve_view().is_none() {
            return;
        }
        let response = ui.interact(
            ui.max_rect(),
            ui.id().with("canvas"),
            egui::Sense::click_and_drag(),
        );
        if response.dragged() && !self.selection.active {
            self.scroll(-response.drag_delta(), false);
        }
        if response.hovered() {
            let (scroll, zoom) = ui.input(|input| (input.smooth_scroll_delta, input.zoom_delta()));
            if (zoom - 1.0).abs() > 0.001 {
                let pivot = ui
                    .input(|input| input.pointer.hover_pos())
                    .map_or(rect.size() * 0.5, |pos| pos - rect.min)
                    .clamp(Vec2::ZERO, rect.size());
                self.zoom_at(zoom, pivot);
            } else {
                self.scroll(-scroll, false);
            }
        }
        self.resolve_view();
        self.continuous_selection(ui, &response, rect);
        let visible = self.visible_pages();
        let mut geometry = std::mem::take(&mut self.continuous_geometry);
        geometry.update(visible.clone(), |page| self.continuous_rect(page, rect));
        let pages = geometry.pages();
        let view = self.continuous_view(ui.ctx().pixels_per_point());
        let planned = self.render_planner.plan(
            PlanMode::Continuous,
            view,
            pages,
            &self.dimensions,
            ui.max_rect(),
            self.cache.budget(),
        );
        let plan = planned.plan;
        self.cache.protect_many(if planned.detailed {
            &plan
        } else {
            &plan[..visible.len()]
        });
        if !self.loading && self.requested.as_ref() != Some(&plan) {
            let keys = plan
                .iter()
                .copied()
                .filter(|key| !self.cache.contains(key))
                .collect();
            self.requested = Some(plan.clone());
            self.worker.render(keys);
        }
        if !self.loading
            && self.continuous_link_request.is_none()
            && let Some(page) = visible.clone().find(|page| !self.links.contains_key(page))
        {
            self.continuous_link_request = Some(page);
            self.worker.links(page);
        }
        let offset = self.current_offset();
        if self.reading.offset != offset {
            self.reading.offset = offset;
            self.dirty = true;
        }
        let active_scale = self.scale;
        let active_page = self.nav.page;
        let painter = ui.painter_at(ui.max_rect());
        for (&(page, page_rect), key) in pages
            .iter()
            .zip(plan.iter().filter(|key| key.tile.is_none()))
        {
            let dimensions = self.dimensions[page];
            if !self.reading.facing {
                self.scale = self.continuous_layout.pages[page].scale;
            }
            self.synctex_click(ui, page, page_rect);
            painter.add(
                egui::epaint::Shadow {
                    offset: [0, 3],
                    blur: 16,
                    spread: 0,
                    color: Color32::from_black_alpha(28),
                }
                .as_shape(page_rect, 0),
            );
            painter.rect_filled(
                page_rect,
                0.0,
                self.page_colors
                    .palette(self.library.night)
                    .background
                    .color(),
            );
            if let Some(texture) = self.cache.get(key).or_else(|| self.cache.fallback(*key)) {
                painter.image(
                    texture,
                    page_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                self.paint_tiles(&painter, &plan, page, page_rect, self.scale);
                self.highlights(&painter, page, page_rect, dimensions);
                if self.selection.active && self.selection_page == page {
                    let clip = painter.clip_rect().intersect(page_rect);
                    painter.extend(self.selection.rects.iter().filter_map(|rect| {
                        let rect = self
                            .link_rect(*rect, page_rect, dimensions)
                            .intersect(page_rect);
                        (rect.is_positive() && clip.intersects(rect))
                            .then(|| egui::Shape::rect_filled(rect, 1.0, SELECTION_COLOR))
                    }));
                }
                self.paint_synctex(&painter, page, page_rect, dimensions);
                if !ui.input(|i| i.modifiers.command) {
                    self.link_overlays(ui, page, page_rect, dimensions);
                    self.comment_overlays(ui, page, page_rect, dimensions);
                }
                if self.nav.page != active_page || self.pending_destination.is_some() {
                    // A clicked internal link changes the layout and pending destination.
                    break;
                }
            } else {
                painter.text(
                    page_rect.intersect(ui.max_rect()).center(),
                    egui::Align2::CENTER_CENTER,
                    "Rendering…",
                    egui::FontId::proportional(15.0),
                    Color32::GRAY,
                );
            }
        }
        self.scale = active_scale;
        self.continuous_geometry = geometry;
    }

    fn continuous_selection(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        viewport: egui::Rect,
    ) {
        if !self.selection.active {
            return;
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(if self.selection.ready() {
                egui::CursorIcon::Text
            } else {
                egui::CursorIcon::Progress
            });
        }
        let clicked = response.clicked_by(egui::PointerButton::Primary);
        if (response.drag_started_by(egui::PointerButton::Primary) || clicked)
            && let Some(position) = ui
                .input(|input| input.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            let page = self
                .visible_pages()
                .find(|&page| self.continuous_rect(page, viewport).contains(position));
            if let Some(page) = page {
                if self.selection_page != page {
                    self.selection.start();
                    self.selection_page = page;
                }
                if self.selection.needs_preparation() {
                    self.worker.inspect(page);
                }
                let point = self.continuous_point(page, position, viewport);
                if clicked {
                    let unit = if response.triple_clicked() {
                        SelectionUnit::Line
                    } else if response.double_clicked() {
                        SelectionUnit::Word
                    } else {
                        SelectionUnit::Glyph
                    };
                    self.selection.select(point, unit);
                } else {
                    self.selection.begin(point);
                }
            } else {
                self.selection.clear_range();
            }
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && self.selection.has_anchor()
            && let Some(position) = response.interact_pointer_pos()
        {
            let delta = selection_scroll::delta(
                ui.max_rect(),
                position,
                ui.input(|input| input.unstable_dt),
            );
            if delta != Vec2::ZERO {
                let before = (self.nav.page, self.pan);
                self.scroll(delta, false);
                if before != (self.nav.page, self.pan) {
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
            }
        }
        if (response.dragged_by(egui::PointerButton::Primary)
            || response.drag_stopped_by(egui::PointerButton::Primary))
            && self.selection.has_anchor()
            && let Some(position) = response.interact_pointer_pos()
        {
            self.selection
                .extend(self.continuous_point(self.selection_page, position, viewport));
        }
    }

    fn continuous_point(
        &self,
        page: usize,
        position: egui::Pos2,
        viewport: egui::Rect,
    ) -> hayro::kurbo::Point {
        let relative = position - self.continuous_rect(page, viewport).min;
        let scale = f64::from(if self.reading.facing {
            self.scale
        } else {
            self.continuous_layout.pages[page].scale
        });
        crate::geometry::unrotate_point(
            hayro::kurbo::Point::new(f64::from(relative.x) / scale, f64::from(relative.y) / scale),
            self.dimensions[page],
            self.reading.rotation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Navigation;

    #[test]
    fn neighboring_link_overlays_reuse_storage_but_refresh_replaced_metadata() {
        use crate::links::{LinkTarget, PageLink};
        use hayro::kurbo::Rect as PdfRect;
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.dimensions = vec![[600.0, 500.0]; 3];
        app.nav = Navigation::new(3, 0);
        app.reading.fit = Fit::Custom(1.0);
        app.set_display_mode(true, false);
        app.scale = 1.0;
        let links = |count, offset| {
            (0..count)
                .map(|index| PageLink {
                    rect: PdfRect::new(
                        offset,
                        40.0 + index as f64 * 30.0,
                        offset + 50.0,
                        60.0 + index as f64 * 30.0,
                    ),
                    target: LinkTarget::Url("https://example.com".into()),
                })
                .collect()
        };
        for (page, count) in [(0, 1), (1, 2), (2, 1)] {
            app.links.insert(page, links(count, 40.0), 0);
        }
        let draw = |app: &mut Pagewing, page| {
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 600.0),
                    )),
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        app.link_overlays(
                            ui,
                            page,
                            egui::Rect::from_min_size(
                                egui::pos2(20.0, 30.0),
                                egui::vec2(600.0, 500.0),
                            ),
                            [600.0, 500.0],
                        );
                    });
                },
            );
        };
        draw(&mut app, 0);
        assert_eq!(app.visible_links.indices, [0]);
        draw(&mut app, 1);
        assert_eq!(app.other_visible_links.indices, [0, 1]);
        let indices = app.other_visible_links.indices.as_ptr();
        let rects = app.other_visible_links.rects.as_ptr();
        draw(&mut app, 2);
        assert_eq!(app.other_visible_links.indices, [0]);
        assert_eq!(app.other_visible_links.indices.as_ptr(), indices);
        assert_eq!(app.other_visible_links.rects.as_ptr(), rects);
        assert_eq!(app.visible_links.indices, [0]);
        let before = app.other_visible_links.rects[0];
        app.links.insert(2, links(1, 140.0), 0);
        draw(&mut app, 2);
        assert_eq!(
            app.other_visible_links.rects[0],
            before.translate(egui::vec2(100.0, 0.0))
        );
        app.links.insert(2, Vec::new(), 0);
        draw(&mut app, 2);
        assert!(app.other_visible_links.indices.is_empty());
        draw(&mut app, 1);
        assert_eq!(app.other_visible_links.indices, [0, 1]);
        assert_eq!(app.other_visible_links.indices.as_ptr(), indices);
        assert_eq!(app.other_visible_links.rects.as_ptr(), rects);
        app.set_display_mode(false, false);
        assert_eq!(app.other_visible_links.indices.capacity(), 0);
        assert_eq!(app.other_visible_links.rects.capacity(), 0);
        let single = app.current_position();
        app.set_display_mode(false, true);
        draw(&mut app, 1);
        assert!(!app.other_visible_links.indices.is_empty());
        app.restore_position(single, false);
        assert_eq!(app.other_visible_links.indices.capacity(), 0);
        assert_eq!(app.other_visible_links.rects.capacity(), 0);
        app.set_display_mode(true, false);
        draw(&mut app, 1);
        app.clear_document(&context);
        assert_eq!(app.other_visible_links.indices.capacity(), 0);
        assert_eq!(app.other_visible_links.rects.capacity(), 0);
    }

    fn frame(app: &mut Pagewing, context: &egui::Context) {
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 600.0),
                )),
                ..Default::default()
            },
            |context| app.canvas(context),
        );
        let mut painted = Vec::new();
        fn white_rects(shape: &egui::Shape, output: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Rect(shape) if shape.fill == Color32::WHITE => output.push(shape.rect),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        white_rects(shape, output);
                    }
                }
                _ => {}
            }
        }
        for shape in &output.shapes {
            white_rects(&shape.shape, &mut painted);
        }
        for &(page, rect) in app.continuous_geometry.pages() {
            assert!(
                painted.contains(&rect),
                "page {page} must paint the planned rectangle {rect:?}"
            );
        }
    }

    #[test]
    fn frame_geometry_tracks_scroll_spreads_and_releases_when_leaving_the_document() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.dimensions = vec![
            [600.0, 120.0],
            [900.0, 200.0],
            [400.0, 100.0],
            [600.0, 800.0],
        ];
        app.nav = Navigation::new(4, 0);
        app.reading.fit = Fit::Custom(1.0);
        app.set_display_mode(true, false);
        frame(&mut app, &context);
        assert_eq!(app.continuous_geometry.pages().len(), 4);
        let allocation = app.continuous_geometry.pages().as_ptr();
        let before = app.continuous_geometry.pages()[0].1;
        app.scroll_continuous(egui::vec2(0.0, 1.0));
        frame(&mut app, &context);
        assert_eq!(app.continuous_geometry.pages().as_ptr(), allocation);
        assert_eq!(
            app.continuous_geometry.pages()[0].1,
            before.translate(egui::vec2(0.0, -1.0))
        );
        app.set_display_mode(false, true);
        frame(&mut app, &context);
        assert_eq!(
            app.continuous_geometry
                .pages()
                .iter()
                .map(|&(page, _)| page)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        let allocation = app.continuous_geometry.pages().as_ptr();
        app.jump(3);
        frame(&mut app, &context);
        assert_eq!(app.continuous_geometry.pages().as_ptr(), allocation);
        assert_eq!(
            app.continuous_geometry
                .pages()
                .iter()
                .map(|&(page, _)| page)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        app.set_display_mode(false, false);
        assert!(app.continuous_geometry.pages().is_empty());
        frame(&mut app, &context);
        assert!(app.continuous_geometry.pages().is_empty());
        let single = app.current_position();
        app.set_display_mode(true, false);
        frame(&mut app, &context);
        assert!(!app.continuous_geometry.pages().is_empty());
        app.restore_position(single, false);
        assert!(app.continuous_geometry.pages().is_empty());
        app.set_display_mode(true, false);
        frame(&mut app, &context);
        app.clear_document(&context);
        assert!(app.continuous_geometry.pages().is_empty());
    }
}
