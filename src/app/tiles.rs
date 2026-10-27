use super::Pagewing;
use crate::render_plan::RenderPlan;
use eframe::egui::{self, Color32, Vec2};

impl Pagewing {
    pub(super) fn paint_tiles(
        &mut self,
        painter: &egui::Painter,
        plan: &RenderPlan,
        page: usize,
        page_rect: egui::Rect,
        scale: f32,
    ) {
        for key in plan.iter().filter(|key| key.page == page) {
            let Some(tile) = key.tile else { continue };
            let Some(texture) = self.cache.get(key) else {
                continue;
            };
            let units = scale / tile.scale();
            let rect = egui::Rect::from_min_size(
                page_rect.min + Vec2::new(tile.x as f32, tile.y as f32) * units,
                Vec2::new(f32::from(tile.size[0]), f32::from(tile.size[1])) * units,
            );
            painter
                .with_clip_rect(painter.clip_rect().intersect(page_rect))
                .image(
                    texture,
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
        }
    }
}
