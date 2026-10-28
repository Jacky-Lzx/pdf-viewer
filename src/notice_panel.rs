//! Measure and paint one notice with the same egui text layout.
//! Galleys stay in egui's font-aware cache; this view retains no message state.
use eframe::egui::{self, text::Galley};
use std::sync::Arc;

pub struct NoticePanel {
    panel: egui::TopBottomPanel,
    galley: Arc<Galley>,
    height: f32,
    compact: bool,
}

impl NoticePanel {
    pub fn new(context: &egui::Context, id: &'static str, text: &str, budget: f32) -> Self {
        let style = context.style();
        let compact = budget < 100.0;
        let margin = if compact { 2 } else { 6 };
        let gap = if compact {
            2.0
        } else {
            style.spacing.item_spacing.y
        };
        let padding = if compact {
            2.0
        } else {
            style.spacing.button_padding.y
        };
        let button = egui::FontSelection::Default
            .resolve_with_fallback(&style, egui::TextStyle::Button.into());
        // Reserve the scrollbar's full width before wrapping so its animation
        // cannot clip text. Floating bars reserve no width by default.
        let width =
            (context.available_rect().width() - 16.0 - style.spacing.scroll.allocated_width())
                .max(1.0);
        let mut job = Arc::unwrap_or_clone(
            egui::WidgetText::from(egui::RichText::new(text).color(style.visuals.error_fg_color))
                .into_layout_job(&style, egui::FontSelection::Default, egui::Align::Center),
        );
        job.wrap.max_width = width;
        let (row, galley) = context.fonts_mut(|fonts| {
            let row =
                (fonts.row_height(&button) + padding * 2.0).max(style.spacing.interact_size.y);
            (row, fonts.layout_job(job))
        });
        let height = galley
            .size()
            .y
            .min((budget - row - gap - f32::from(margin) * 2.0).clamp(1.0, 160.0));
        let panel = egui::TopBottomPanel::bottom(id)
            .frame(
                egui::Frame::NONE
                    .fill(style.visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(8, margin)),
            )
            .exact_height(height + row + gap + f32::from(margin) * 2.0);
        Self {
            panel,
            galley,
            height,
            compact,
        }
    }

    pub fn show<R>(
        self,
        context: &egui::Context,
        message_id: &'static str,
        actions: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        self.panel.show(context, |ui| {
            if self.compact {
                ui.spacing_mut().button_padding.y = 2.0;
                ui.spacing_mut().item_spacing.y = 2.0;
            }
            egui::ScrollArea::vertical()
                .id_salt(message_id)
                .max_height(self.height)
                .min_scrolled_height(self.height)
                .auto_shrink([false, false])
                .show(ui, |ui| ui.add(egui::Label::new(self.galley)));
            actions(ui)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_and_painted_notices_follow_live_fonts_colors_width_and_scrollbars() {
        let context = egui::Context::default();
        for (width, dark, font_size, pixels, floating) in [
            (1180.0, false, 14.0, 1.0, true),
            (240.0, true, 18.0, 2.0, false),
            (360.0, false, 23.0, 1.0, true),
            (1180.0, true, 14.0, 1.5, false),
        ] {
            context.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            context.style_mut(|style| {
                style.override_font_id = Some(egui::FontId::proportional(font_size));
                style.spacing.scroll = if floating {
                    egui::style::ScrollStyle::floating()
                } else {
                    egui::style::ScrollStyle::solid()
                };
            });
            let text = format!("PDF failure — 长路径 📝 {font_size} ").repeat(40);
            for _ in 0..4 {
                let mut measured = None;
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 820.0),
                    )),
                    ..Default::default()
                };
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(pixels);
                let output = context.run(input, |context| {
                    let panel = NoticePanel::new(context, "notice", &text, 300.0);
                    let wrap_width = panel.galley.job.wrap.max_width;
                    measured = Some(panel.galley.clone());
                    panel.show(context, "message", |ui| {
                        let _ = ui.button("Dismiss");
                    });
                    // Compare with normal Label layout at the same width,
                    // using live egui style/font handling as the reference.
                    egui::CentralPanel::default().show(context, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(wrap_width, 1.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&text)
                                            .color(ui.visuals().error_fg_color),
                                    )
                                    .wrap(),
                                );
                            },
                        );
                    });
                });
                let measured = measured.unwrap();
                let painted: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(shape) if shape.galley.text() == text => {
                            Some(&shape.galley)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(painted.len(), 2);
                assert!(
                    Arc::ptr_eq(&measured, painted[0]),
                    "height and paint share one layout"
                );
                assert_eq!(measured.size(), painted[1].size());
                assert_eq!(measured.rows.len(), painted[1].rows.len());
                assert_eq!(measured.job.sections, painted[1].job.sections);
                assert_eq!(measured.pixels_per_point, context.pixels_per_point());
                assert!(
                    measured.size().x <= width - 16.0,
                    "width {width}, pixels {pixels}, wrap {}, size {:?}, screen {:?}",
                    measured.job.wrap.max_width,
                    measured.size(),
                    context.content_rect()
                );
            }
        }
    }
}
