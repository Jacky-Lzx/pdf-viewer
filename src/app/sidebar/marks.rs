use super::{Pagewing, RowAction, appearance, removable_row};
use crate::app::MarkAction;
use eframe::egui;

impl Pagewing {
    pub(super) fn mark_list(&mut self, ui: &mut egui::Ui) -> bool {
        let requested =
            self.removal_focus
                .target(ui.ctx(), self.id, crate::app::removal_focus::List::Marks);
        let focus_name = requested
            .and_then(|key| char::from_u32(key as u32))
            .and_then(|name| {
                self.reading
                    .marks
                    .range(name..)
                    .next()
                    .or_else(|| self.reading.marks.last_key_value())
                    .map(|(&name, _)| name)
            });
        ui.add_space(20.0);
        appearance::section(ui, "MARKS");
        let mut target = None;
        let mut remove = None;
        let mut removal_origin = None;
        for (&name, mark) in &self.reading.marks {
            // Names keep their button IDs when earlier marks are edited.
            ui.scope_builder(
                egui::UiBuilder::new().id(("sidebar-mark", self.id, name)),
                |ui| {
                    let response = removable_row(
                        ui,
                        mark.page == self.nav.page,
                        format!("'{name}   {}", self.labels.sidebar_caption(mark.page)),
                        "Delete mark",
                        |ui| {
                            ui.label(self.labels.sidebar_tooltip(mark.page));
                            ui.label("Restore page, zoom, rotation and scroll position");
                        },
                    );
                    self.removal_focus.register(&response.remove);
                    if focus_name == Some(name) {
                        self.removal_focus.apply(ui, &response.jump);
                        response
                            .jump
                            .scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                    }
                    match response.action {
                        Some(RowAction::Jump) => target = Some(name),
                        Some(RowAction::Remove) => {
                            remove = Some(name);
                            removal_origin = response.keyboard_removal;
                        }
                        None => {}
                    }
                },
            );
        }
        if let Some(name) = target {
            self.mark(name, MarkAction::Jump);
        }
        if let Some(name) = remove {
            if let Some(origin) = removal_origin {
                let neighbor = self
                    .reading
                    .marks
                    .range((std::ops::Bound::Excluded(name), std::ops::Bound::Unbounded))
                    .next()
                    .or_else(|| self.reading.marks.range(..name).next_back())
                    .map(|(&name, _)| name as usize);
                self.removal_focus.request(
                    self.id,
                    crate::app::removal_focus::List::Marks,
                    neighbor,
                    origin,
                );
                if neighbor.is_none() {
                    ui.memory_mut(|memory| memory.surrender_focus(origin));
                }
            }
            self.remove_mark(name);
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
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(context.input(|input| input.time) + 0.25),
            events,
            ..Default::default()
        };
        app.prepare_native_input(context, &mut input);
        context.run(input, |context| {
            app.keys(context);
            app.toolbar(context);
            app.status(context);
            app.sidebar(context);
            egui::CentralPanel::default().show(context, |_| {});
        })
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

    fn key(key: egui::Key) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .into()
    }

    #[test]
    fn keyboard_deletion_restores_neighbor_focus_before_following_its_saved_view() {
        let size = egui::vec2(1180.0, 820.0);
        for (removed, neighbor, batch) in [
            ('a', 'm', false),
            ('m', 'z', false),
            ('z', 'm', false),
            ('m', 'z', true),
            ('z', 'm', true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let context = egui::Context::default();
            let mut app =
                Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
            app.nav = Navigation::new(80, 0);
            app.dimensions = vec![[600.0, 800.0]; 80];
            app.library.sidebar = true;
            app.reading.bookmarks = [0, 4].into_iter().collect();
            app.refresh_bookmarks();
            let mark = ReadingMark {
                page: 40,
                fit: Fit::Custom(1.75),
                rotation: 1,
                continuous: true,
                facing: false,
                offset: [0.4, 0.6],
            };
            for name in ['a', 'm', 'z'] {
                app.reading.marks.insert(name, mark.clone());
            }
            app.outline.set_entries(vec![crate::outline::OutlineEntry {
                title: "Keyboard chapter".into(),
                destination: Some(crate::destination::Destination::page(0)),
                depth: 0,
            }]);
            app.focus_contents();
            settle(&mut app, &context, size);
            let mut reached = false;
            for _ in 0..24 {
                frame(&mut app, &context, size, key(egui::Key::Tab));
                let output = settle(&mut app, &context, size);
                let (label, _) = text_bounds(&output, &format!("'{removed}"));
                if context.memory(|memory| memory.focused()).is_some_and(|id| {
                    context.read_response(id).is_some_and(|response| {
                        response.rect.y_range().intersects(label.y_range())
                            && response.rect.left() > label.right()
                    })
                }) {
                    reached = true;
                    break;
                }
            }
            assert!(reached, "Tab must reach mark {removed}'s delete button");
            app.pan = egui::vec2(37.0, 53.0);
            let mut events = key(egui::Key::Space);
            if batch {
                events.extend(key(egui::Key::Enter));
            }
            let size = if batch {
                egui::vec2(240.0, 240.0)
            } else {
                size
            };
            // A resize can have its own native prefix before the activation.
            // Its previous response may then be clipped until the next layout.
            if batch {
                frame(&mut app, &context, size, Vec::new());
            }
            frame(&mut app, &context, size, events);
            let output = settle(&mut app, &context, size);
            assert!(!app.reading.marks.contains_key(&removed));
            assert_eq!(app.reading.marks.len(), 2);
            let id = context
                .memory(|memory| memory.focused())
                .expect("neighbor must gain focus");
            let response = context.read_response(id).unwrap();
            let (label, _) = text_bounds(&output, &format!("'{neighbor}"));
            assert!(response.rect.contains_rect(label));
            if !batch {
                assert_eq!(app.nav.page, 0);
                assert_eq!(app.pan, egui::vec2(37.0, 53.0));
                assert_eq!(app.reading.rotation, 0);
                frame(&mut app, &context, size, key(egui::Key::Enter));
            }
            assert_eq!(app.nav.page, mark.page);
            assert_eq!(app.reading.fit, mark.fit);
            assert_eq!(app.reading.rotation, mark.rotation);
            assert_eq!(app.reading.continuous, mark.continuous);
            assert_eq!(app.reading.facing, mark.facing);
            assert_eq!(app.pending_offset, Some(mark.offset));
            assert!(app.focus_input.is_empty());
        }
    }

    #[test]
    fn editing_other_marks_retains_focus_and_activates_the_saved_view() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(80, 0);
        app.dimensions = vec![[600.0, 800.0]; 80];
        app.library.sidebar = true;
        let mark = ReadingMark {
            page: 40,
            fit: Fit::Custom(1.75),
            rotation: 1,
            continuous: true,
            facing: false,
            offset: [0.4, 0.6],
        };
        app.reading.marks.insert('a', ReadingMark::default());
        app.reading.marks.insert('m', mark.clone());
        app.outline.set_entries(vec![crate::outline::OutlineEntry {
            title: "Keyboard chapter".into(),
            destination: Some(crate::destination::Destination::page(0)),
            depth: 0,
        }]);
        app.focus_contents();
        let size = egui::vec2(1180.0, 820.0);
        settle(&mut app, &context, size);
        let key = |key| {
            [true, false]
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .into()
        };
        let mut focused = None;
        for _ in 0..16 {
            frame(&mut app, &context, size, key(egui::Key::Tab));
            let output = settle(&mut app, &context, size);
            let (label, _) = text_bounds(&output, "'m");
            if let Some(id) = context.memory(|memory| memory.focused())
                && context.read_response(id).is_some_and(|response| {
                    response.rect.contains_rect(label)
                        && response.rect.height() < 2.0 * label.height()
                })
            {
                focused = Some(id);
                break;
            }
        }
        let focused = focused.expect("Tab must reach the named position jump button");
        app.remove_mark('a');
        settle(&mut app, &context, size);
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        app.restore_deleted();
        settle(&mut app, &context, size);
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        app.mark('b', MarkAction::Set);
        settle(&mut app, &context, size);
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        frame(&mut app, &context, size, key(egui::Key::Space));
        assert_eq!(app.nav.page, mark.page);
        assert_eq!(app.reading.fit, mark.fit);
        assert_eq!(app.reading.rotation, mark.rotation);
        assert_eq!(app.reading.continuous, mark.continuous);
        assert_eq!(app.reading.facing, mark.facing);
        assert_eq!(app.pending_offset, Some(mark.offset));
        app.remove_mark('m');
        settle(&mut app, &context, size);
        assert_ne!(context.memory(|memory| memory.focused()), Some(focused));
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
