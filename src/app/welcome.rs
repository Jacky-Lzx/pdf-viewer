use super::{ACCENT, Pagewing, appearance, shortcuts};
use eframe::egui::{self, Color32, RichText, Vec2};

impl Pagewing {
    pub(super) fn welcome(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("welcome")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = (ui.available_width() - 40.0).clamp(160.0, 460.0);
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.10).clamp(24.0, 80.0));
                    appearance::logo(ui, 68.0);
                    ui.add_space(18.0);
                    appearance::section(ui, "P A G E W I N G");
                    ui.add_space(10.0);
                    ui.label(RichText::new("A little room to read.").size(32.0).strong());
                    ui.add_space(8.0);
                    ui.weak("Your documents. A quieter place to focus.");
                    ui.add_space(24.0);
                    if self.loading {
                        ui.spinner();
                        ui.label("Opening document…");
                    } else if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Open a document")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(ACCENT)
                            .corner_radius(10)
                            .min_size(Vec2::new(200.0, 44.0)),
                        )
                        .clicked()
                    {
                        self.choose_file();
                    }
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!(
                            "Drop a PDF here  ·  {}",
                            self.library.shortcuts.label("open")
                        ))
                        .small()
                        .weak(),
                    );
                    ui.add_space(36.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            if self.library.recent.is_empty() {
                                ui.separator();
                                ui.add_space(12.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.weak("Made for your keyboard");
                                    ui.label(
                                        RichText::new("j / k")
                                            .monospace()
                                            .color(appearance::accent(ui)),
                                    );
                                    ui.weak("scroll");
                                    ui.label(
                                        RichText::new("/")
                                            .monospace()
                                            .color(appearance::accent(ui)),
                                    );
                                    ui.weak("find");
                                    ui.label(
                                        RichText::new("?")
                                            .monospace()
                                            .color(appearance::accent(ui)),
                                    );
                                    ui.weak("shortcuts");
                                });
                                return;
                            }
                            appearance::section(ui, "CONTINUE READING");
                            ui.add_space(6.0);
                            let mut open = None;
                            for (index, item) in self.library.recent.iter().take(4).enumerate() {
                                let name =
                                    item.path.file_name().unwrap_or_default().to_string_lossy();
                                let row = egui::Frame::new()
                                    .fill(ui.visuals().panel_fill)
                                    .corner_radius(10)
                                    .inner_margin(14)
                                    .show(ui, |ui| {
                                        ui.set_width((width - 28.0).max(0.0));
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(
                                                Vec2::new(28.0, 32.0),
                                                egui::Sense::hover(),
                                            );
                                            let color = appearance::accent(ui);
                                            ui.painter().rect_stroke(
                                                rect.shrink2(Vec2::new(5.0, 2.0)),
                                                3.0,
                                                egui::Stroke::new(1.3_f32, color),
                                                egui::StrokeKind::Inside,
                                            );
                                            for offset in [11.0, 16.0, 21.0] {
                                                ui.painter().line_segment(
                                                    [
                                                        rect.min + Vec2::new(10.0, offset),
                                                        rect.min + Vec2::new(18.0, offset),
                                                    ],
                                                    egui::Stroke::new(1.0_f32, color),
                                                );
                                            }
                                            ui.vertical(|ui| {
                                                ui.set_width((width - 80.0).max(0.0));
                                                ui.add(
                                                    egui::Label::new(RichText::new(name).strong())
                                                        .truncate(),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Page {}  ·  Resume reading",
                                                        item.page + 1
                                                    ))
                                                    .small()
                                                    .weak(),
                                                );
                                            });
                                        });
                                    });
                                let response = ui
                                    .interact(
                                        row.response.rect,
                                        ui.id().with(("recent", index)),
                                        egui::Sense::click(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(item.path.display().to_string());
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        item.path.display().to_string(),
                                    )
                                });
                                if response.hovered() || response.has_focus() {
                                    ui.painter().rect_stroke(
                                        row.response.rect,
                                        10.0,
                                        egui::Stroke::new(1.0_f32, appearance::accent(ui)),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                if response.clicked() {
                                    open = Some(item.path.clone());
                                }
                            }
                            if let Some(path) = open {
                                self.open(path, ui.ctx());
                            }
                            ui.add_space(6.0);
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "All recent documents  ·  {}",
                                        self.library.shortcuts.label("recent")
                                    ))
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.toggle_recent(ui.ctx());
                            }
                        },
                    );
                    ui.add_space(30.0);
                });
                shortcuts::reveal_focused_control(ui, ui.clip_rect().size());
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ReadingState;

    fn app() -> (tempfile::TempDir, egui::Context, Pagewing) {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        for index in 0..4 {
            let path = directory
                .path()
                .join(format!("Recent document {index}.pdf"));
            std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
            app.library.recent.push(ReadingState {
                path,
                page: index,
                ..Default::default()
            });
        }
        (directory, context, app)
    }

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: Vec2,
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
                app.canvas(context);
            },
        )
    }

    fn settle(app: &mut Pagewing, context: &egui::Context, size: Vec2) {
        for _ in 0..6 {
            frame(app, context, size, Vec::new());
        }
    }

    fn key(key: egui::Key, shift: bool) -> Vec<egui::Event> {
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            })
            .collect()
    }

    fn focused(context: &egui::Context) -> egui::Response {
        let id = context.memory(|memory| memory.focused()).unwrap();
        context.read_response(id).unwrap()
    }

    fn assert_focused_visible(context: &egui::Context, size: Vec2) -> egui::Response {
        let response = focused(context);
        assert!(
            response
                .interact_rect
                .contains_rect(response.rect.shrink(1.0)),
            "{size:?}: focused control {:?} clipped by {:?}",
            response.rect,
            response.interact_rect
        );
        assert!(context.content_rect().contains_rect(response.rect));
        response
    }

    fn tab_to_all_recent(app: &mut Pagewing, context: &egui::Context, size: Vec2) -> egui::Id {
        settle(app, context, size);
        for _ in 0..20 {
            let output = frame(app, context, size, key(egui::Key::Tab, false));
            let reached_all_recent = output.platform_output.events.iter().any(|event| {
                matches!(event, egui::output::OutputEvent::FocusGained(info)
                    if info.label.as_deref().is_some_and(|label| label.starts_with("All recent documents")))
            });
            settle(app, context, size);
            let response = assert_focused_visible(context, size);
            if reached_all_recent {
                return response.id;
            }
        }
        panic!("all welcome actions must be reachable");
    }

    #[test]
    fn tab_reveals_welcome_actions_and_opens_the_focused_recent_document() {
        for (disabled_sidebar, activation) in [(false, egui::Key::Enter), (true, egui::Key::Space)]
        {
            let (_directory, context, mut app) = app();
            if disabled_sidebar {
                app.library
                    .shortcuts
                    .overrides
                    .insert("sidebar".into(), Vec::new());
            }
            let size = Vec2::new(240.0, 240.0);
            let expected = app.library.recent[3].path.clone();
            tab_to_all_recent(&mut app, &context, size);
            assert!(
                !app.library.sidebar,
                "welcome Tab must not toggle the sidebar"
            );
            frame(&mut app, &context, size, key(egui::Key::Tab, true));
            settle(&mut app, &context, size);
            assert_focused_visible(&context, size);
            frame(&mut app, &context, size, key(activation, false));
            assert_eq!(app.reading.path, expected);
            assert_eq!(app.reading.page, 3);
        }
    }

    #[test]
    fn welcome_focus_survives_resize_without_overriding_manual_scroll() {
        let (_directory, context, mut app) = app();
        let target = tab_to_all_recent(&mut app, &context, Vec2::new(1180.0, 820.0));
        for size in super::super::dialog::TEST_SIZES {
            settle(&mut app, &context, size);
            assert_eq!(assert_focused_visible(&context, size).id, target);
        }
        let size = Vec2::new(240.0, 240.0);
        settle(&mut app, &context, size);
        let position = assert_focused_visible(&context, size).rect.center();
        frame(
            &mut app,
            &context,
            size,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, 2000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..2 {
            settle(&mut app, &context, size);
            let response = focused(&context);
            assert_eq!(response.id, target);
            assert!(!response.interact_rect.contains_rect(response.rect));
        }
        let size = Vec2::new(240.0, 400.0);
        settle(&mut app, &context, size);
        assert_eq!(assert_focused_visible(&context, size).id, target);
    }
}
