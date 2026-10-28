use eframe::egui;

/// Match egui's text tooltip layout while preparing text only when it is shown.
pub(super) trait LazyTooltip {
    fn on_hover_text_lazy(self, text: impl FnOnce() -> String) -> Self;
}

impl LazyTooltip for egui::Response {
    fn on_hover_text_lazy(self, text: impl FnOnce() -> String) -> Self {
        self.on_hover_ui(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.add(egui::Label::new(text()));
        })
    }
}
