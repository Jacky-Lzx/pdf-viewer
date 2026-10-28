// Frozen notice layout/paint helpers from a1c9de0; only visibility changed.
use eframe::egui;

/// Recompute the height when messages or the viewport change. Bottom panels
/// otherwise keep a previous height and can clip newly wrapped contents.
pub fn notice_panel(
    context: &egui::Context,
    id: &'static str,
    text: &str,
    budget: f32,
) -> (egui::TopBottomPanel, f32, bool) {
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
    let body = egui::TextStyle::Body.resolve(&style);
    let button = egui::TextStyle::Button.resolve(&style);
    let width = (context.available_rect().width() - 32.0).max(1.0);
    let (row, desired) = context.fonts_mut(|fonts| {
        let row = (fonts.row_height(&button) + padding * 2.0).max(style.spacing.interact_size.y);
        let desired = fonts
            .layout(text.to_owned(), body, style.visuals.error_fg_color, width)
            .size()
            .y;
        (row, desired)
    });
    let height = desired.min((budget - row - gap - f32::from(margin) * 2.0).clamp(1.0, 160.0));
    let panel = egui::TopBottomPanel::bottom(id)
        .frame(
            egui::Frame::NONE
                .fill(style.visuals.panel_fill)
                .inner_margin(egui::Margin::symmetric(8, margin)),
        )
        .exact_height(height + row + gap + f32::from(margin) * 2.0);
    (panel, height, compact)
}

pub fn compact_spacing(ui: &mut egui::Ui, compact: bool) {
    if compact {
        ui.spacing_mut().button_padding.y = 2.0;
        ui.spacing_mut().item_spacing.y = 2.0;
    }
}

pub fn copy_button(ui: &mut egui::Ui) -> egui::Response {
    let response = ui.button(if ui.max_rect().width() < 360.0 {
        "Copy"
    } else {
        "Copy error"
    });
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Copy error")
    });
    response.on_hover_text("Copy the complete error message")
}

pub fn message(ui: &mut egui::Ui, id: &'static str, text: &str, height: f32) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).color(ui.visuals().error_fg_color))
                    .wrap(),
            );
        });
}
