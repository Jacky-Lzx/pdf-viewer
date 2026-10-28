//! Bounded comment body scrolling, coordinated with the outer sidebar.
use super::shortcuts;
use eframe::egui;

pub(super) fn show(
    ui: &mut egui::Ui,
    contents: &mut String,
    focus: &mut bool,
    deferred: &mut Vec<egui::Event>,
    height: f32,
    editable: bool,
) -> bool {
    let id = egui::Id::new("pagewing-comment-input");
    let layout = (ui.ctx().content_rect().size(), ui.available_width(), height);
    let resized = ui.data_mut(|data| {
        let previous = data.get_temp::<(egui::Vec2, f32, f32)>(id.with("layout"));
        data.insert_temp(id.with("layout"), layout);
        previous != Some(layout)
    });
    let pending = ui.data(|data| data.get_temp::<bool>(id.with("reveal")).unwrap_or(false));
    let editor = egui::ScrollArea::vertical()
        .id_salt("pagewing-comment-body")
        .max_height(height)
        .min_scrolled_height(80.0)
        .animated(false)
        .show(ui, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                let previous = egui::TextEdit::load_state(ui.ctx(), id)
                    .and_then(|state| state.cursor.char_range());
                let (output, focused_now) = shortcuts::text_input_output(
                    ui,
                    id,
                    focus,
                    deferred,
                    egui::TextEdit::multiline(contents)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY)
                        .hint_text("Write a comment…"),
                );
                let reveal = output.response.has_focus()
                    && (focused_now
                        || output.response.gained_focus()
                        || resized
                        || pending
                        || output.response.changed()
                        || output.cursor_range != previous);
                let cursor = output.cursor_range.filter(|_| reveal).map(|cursor| {
                    output
                        .galley
                        .pos_from_cursor(cursor.primary)
                        .translate(output.galley_pos.to_vec2())
                });
                let pending = cursor.is_some_and(|rect| {
                    ui.scroll_to_rect(rect.expand(2.0), None);
                    !output.text_clip_rect.contains_rect(rect)
                });
                (output.response, cursor, pending)
            })
            .inner
        });
    let changed = editor.inner.0.changed();
    if let Some(cursor) = editor.inner.1 {
        // A minimum window may show only part of the body viewport.
        // Reveal its caret end, rather than the whole oversized area.
        let height = cursor.height().min(editor.inner_rect.height());
        let top = cursor
            .top()
            .clamp(editor.inner_rect.top(), editor.inner_rect.bottom() - height);
        let target =
            egui::Rect::from_min_size(egui::pos2(cursor.left(), top), egui::vec2(0.0, height));
        ui.scroll_to_rect(target.expand(2.0), None);
    }
    ui.data_mut(|data| data.insert_temp(id.with("reveal"), editor.inner.2));
    if editor.inner.2 {
        ui.ctx().request_repaint();
    }
    changed
}
