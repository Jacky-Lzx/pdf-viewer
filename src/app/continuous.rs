use super::{Folio, SELECTION_COLOR, selection_scroll};
use crate::{navigation::Fit, render_plan::RenderView, selection::SelectionUnit};
use eframe::egui::{self, Color32, Vec2};
use std::time::Duration;

impl Folio {
    fn continuous_view(&self, pixels_per_point: f32) -> RenderView {
        RenderView {
            fit: self.reading.fit,
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
        self.resolve_view();
        self.reading.continuous = !self.reading.continuous;
        self.pending_offset = None;
        self.requested = None;
        self.continuous_link_request = None;
        self.hints.cancel();
        self.selection.cancel();
        self.nav.clear_prefix();
        self.resolve_view();
        self.dirty = true;
    }

    fn continuous_position(&mut self, page: usize, offset: f32) {
        if self.nav.jump(page) {
            self.links.set_page(page);
            self.reveal_page = Some(page);
            self.visible_links.clear();
            self.hints.cancel();
            self.requested = None;
            self.pending_search_jump = None;
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
        let layout = self.continuous_layout.pages[page];
        let current = self.continuous_layout.pages[self.nav.page];
        let x = viewport.left() + (viewport.width() - layout.size[0]).max(0.0) * 0.5 - self.pan.x;
        let y = viewport.top() + (layout.top - current.top - f64::from(self.pan.y)) as f32;
        egui::Rect::from_min_size(egui::pos2(x, y), Vec2::from(layout.size))
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
            if response.double_clicked() && !self.selection.active {
                self.reading.fit = Fit::Page;
                self.pan = Vec2::ZERO;
                self.dirty = true;
            }
        }
        self.resolve_view();
        self.continuous_selection(ui, &response, rect);
        let visible = self
            .continuous_layout
            .visible(self.nav.page, self.pan.y, 24.0);
        let pages: Vec<_> = visible
            .clone()
            .map(|page| (page, self.continuous_rect(page, rect)))
            .collect();
        let view = self.continuous_view(ui.ctx().pixels_per_point());
        let detail = view.detail_plan(&pages, &self.dimensions, ui.max_rect(), self.cache.budget());
        let detailed = detail.is_some();
        let plan = detail.unwrap_or_else(|| {
            view.continuous_plan(visible.clone(), &self.dimensions, self.cache.budget())
        });
        self.cache.protect_many(if detailed {
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
        for (page, key) in visible.zip(plan.iter().filter(|key| key.tile.is_none())) {
            let page_rect = self.continuous_rect(page, rect);
            let dimensions = self.dimensions[page];
            self.scale = self.continuous_layout.pages[page].scale;
            painter.rect_filled(
                page_rect.translate(Vec2::new(2.0, 4.0)).expand(2.0),
                1.0,
                Color32::from_black_alpha(25),
            );
            painter.rect_filled(
                page_rect,
                0.0,
                if self.library.night {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                },
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
                self.link_overlays(ui, page, page_rect, dimensions);
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
                .continuous_layout
                .visible(self.nav.page, self.pan.y, 24.0)
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
                self.scroll_continuous(delta);
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
        let scale = f64::from(self.continuous_layout.pages[page].scale);
        crate::geometry::unrotate_point(
            hayro::kurbo::Point::new(f64::from(relative.x) / scale, f64::from(relative.y) / scale),
            self.dimensions[page],
            self.reading.rotation,
        )
    }
}
