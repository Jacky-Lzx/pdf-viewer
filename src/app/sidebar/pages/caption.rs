use eframe::egui::{self, Color32, Painter, Rect, text::LayoutJob};

pub(super) fn paint(painter: &Painter, card: Rect, text: String, color: Color32) {
    // Move the owned caption into layout; Painter::text would copy it again
    // through ToString. Keep color live when painting the cached egui layout.
    let mut job =
        LayoutJob::simple_singleline(text, egui::FontId::proportional(13.0), Color32::PLACEHOLDER);
    job.wrap.max_width = (card.width() - 8.0).max(0.0);
    job.wrap.max_rows = 1;
    let galley = painter.layout_job(job);
    let bounds = egui::Align2::CENTER_BOTTOM.anchor_size(
        egui::pos2(card.center().x, card.bottom() - 8.0),
        galley.size(),
    );
    painter
        .with_clip_rect(painter.clip_rect().intersect(card.shrink(4.0)))
        .galley(bounds.min, galley, color);
}
