use super::{Pagewing, RowAction, appearance, removable_row};
use crate::app::MarkAction;
use eframe::egui;

impl Pagewing {
    pub(super) fn mark_list(&mut self, ui: &mut egui::Ui) -> bool {
        ui.add_space(20.0);
        appearance::section(ui, "MARKS");
        let mut target = None;
        let mut remove = None;
        for (&name, mark) in &self.reading.marks {
            ui.push_id(("mark", name), |ui| {
                match removable_row(
                    ui,
                    mark.page == self.nav.page,
                    format!("'{name}   {}", self.labels.sidebar_caption(mark.page)),
                    "Delete mark",
                    |ui| {
                        ui.label(self.labels.sidebar_tooltip(mark.page));
                        ui.label("Restore page, zoom, rotation and scroll position");
                    },
                ) {
                    Some(RowAction::Jump) => target = Some(name),
                    Some(RowAction::Remove) => remove = Some(name),
                    None => {}
                }
            });
        }
        if let Some(name) = target {
            self.mark(name, MarkAction::Jump);
        }
        if let Some(name) = remove {
            self.reading.marks.remove(&name);
            self.dirty = true;
        }
        target.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        navigation::{Fit, Navigation},
        state::ReadingMark,
    };

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(context.input(|input| input.time) + 0.25),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.toolbar(context);
                app.status(context);
                app.sidebar(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        )
    }

    fn settle(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        for _ in 0..6 {
            output = frame(app, context, size, Vec::new());
        }
        output
    }

    fn text_bounds(output: &egui::FullOutput, label: &str) -> (egui::Rect, egui::Rect) {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with(label) => Some((
                    egui::Rect::from_min_size(text.pos, text.galley.size()),
                    shape.clip_rect,
                )),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label}"))
    }

    fn click(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2, pos: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                app,
                context,
                size,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn mark_actions_fit_narrow_sidebars_and_restore_saved_views() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(80, 0);
        app.dimensions = vec![[600.0, 800.0]; 80];
        app.library.sidebar = true;
        let mark = ReadingMark {
            page: 79,
            fit: Fit::Custom(1.75),
            rotation: 1,
            continuous: true,
            facing: false,
            offset: [0.4, 0.6],
        };
        for size in [
            egui::vec2(1180.0, 820.0),
            egui::vec2(240.0, 400.0),
            egui::vec2(240.0, 820.0),
            egui::vec2(360.0, 540.0),
            egui::vec2(480.0, 420.0),
        ] {
            app.nav.jump(0);
            app.reading.fit = Fit::Width;
            app.reading.rotation = 0;
            app.reading.continuous = false;
            app.reading.facing = true;
            app.pending_offset = None;
            app.reading.marks.insert('z', mark.clone());
            let output = settle(&mut app, &context, size);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let (delete, clip) = text_bounds(&output, "×");
            let button = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if rect.rect.contains_rect(delete) => Some(rect.rect),
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap();
            assert!(screen.contains_rect(button), "{size:?}: {button:?}");
            assert!(
                clip.contains_rect(button),
                "{size:?}: {button:?} clipped by {clip:?}"
            );
            let (label, label_clip) = text_bounds(&output, "'z");
            assert!(label_clip.contains_rect(label));
            assert!(label.right() < button.left());
            click(&mut app, &context, size, label.center());
            assert_eq!(app.nav.page, mark.page);
            assert_eq!(app.reading.fit, mark.fit);
            assert_eq!(app.reading.rotation, mark.rotation);
            assert_eq!(app.reading.continuous, mark.continuous);
            assert_eq!(app.reading.facing, mark.facing);
            assert_eq!(app.pending_offset, Some(mark.offset));
            let output = settle(&mut app, &context, size);
            let (delete, _) = text_bounds(&output, "×");
            app.dirty = false;
            click(&mut app, &context, size, delete.center());
            assert!(app.reading.marks.is_empty());
            assert_eq!(app.nav.page, mark.page, "deletion must not navigate");
            assert_eq!(app.reading.fit, mark.fit);
            assert!(app.dirty, "deletion must be persisted");
        }
    }

    #[test]
    fn marks_show_bounded_captions_and_full_page_labels_on_hover() {
        use lopdf::dictionary;
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(6, 0);
        app.dimensions = vec![[600.0, 800.0]; 6];
        app.library.sidebar = true;
        let prefix = "Long appendix 中文😀 title\nsecond line ";
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../../../assets/pagewing-guide.pdf"))
                .unwrap();
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend(prefix.encode_utf16().flat_map(u16::to_be_bytes));
        let labels = document.add_object(dictionary! {
            "Nums" => vec![lopdf::Object::Integer(0), dictionary! {
                "S" => lopdf::Object::Name(b"D".to_vec()),
                "P" => lopdf::Object::String(bytes, lopdf::StringFormat::Hexadecimal),
            }.into()],
        });
        let catalog = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document
            .get_dictionary_mut(catalog)
            .unwrap()
            .set("PageLabels", labels);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        app.labels =
            crate::page_labels::PageLabels::read(&hayro::hayro_syntax::Pdf::new(bytes).unwrap());
        app.reading.marks.insert(
            'a',
            ReadingMark {
                page: 5,
                ..Default::default()
            },
        );
        let size = egui::vec2(1180.0, 820.0);
        let output = settle(&mut app, &context, size);
        let (caption, _) = text_bounds(&output, "'a   Page 6 · Long appendi…");
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::PointerMoved(caption.center())],
        );
        let output = settle(&mut app, &context, size);
        assert!(output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::Text(text) =>
                text.galley.text() == format!("Go to page 6\nPDF page label: {prefix}6"),
            _ => false,
        }));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "Restore page, zoom, rotation and scroll position")));
    }
}
