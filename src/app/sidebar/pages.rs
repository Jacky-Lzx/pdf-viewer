use super::{Pagewing, appearance, reveal_offset};
use crate::app::list_focus::ListFocus;
use crate::{navigation::Fit, thumbnails::PreviewLayout};
use eframe::egui::{self, Color32, Vec2};

mod caption;

#[derive(Clone, Copy)]
struct PageViewport {
    document: u64,
    size: Vec2,
    row_height: f32,
    previews: bool,
    selected_visible: bool,
    active: bool,
}

impl Pagewing {
    pub(super) fn pages(&mut self, ui: &mut egui::Ui, viewport: Vec2, other_focus: bool) {
        ui.add_space(20.0);
        let heading = ui
            .horizontal_wrapped(|ui| {
                appearance::section(ui, "PAGES");
                let changed = ui
                    .selectable_value(&mut self.library.thumbnails, false, "List")
                    .changed()
                    | ui.selectable_value(&mut self.library.thumbnails, true, "Previews")
                        .changed();
                if changed {
                    self.dirty = true;
                    self.reveal_page = Some(self.nav.page);
                    self.reveal_page_section = true;
                }
            })
            .response
            .rect;
        let row_height = Self::sidebar_row_height(ui);
        let height = (viewport.y - heading.height() - 3.0 * ui.spacing().item_spacing.y).clamp(
            row_height.max(48.0),
            if self.library.thumbnails {
                400.0
            } else {
                300.0
            },
        );
        let layout = PreviewLayout::new(
            ui.available_width() - ui.spacing().scroll.allocated_width(),
            height,
        );
        let row_height = if self.library.thumbnails {
            layout.row_height
        } else {
            row_height
        };
        let viewport_id = ui.make_persistent_id("page-viewport");
        let previous = ui
            .ctx()
            .data(|data| data.get_temp::<PageViewport>(viewport_id))
            .filter(|previous| previous.document == self.id);
        let resized = previous.is_some_and(|previous| {
            previous.active
                && previous.selected_visible
                && (previous.size != viewport
                    || previous.row_height != row_height
                    || previous.previews != self.library.thumbnails)
        });
        let explicit = self.reveal_page_section;
        let mut reveal = self.reveal_page;
        // A default page list can share the viewport with bookmarks. Only a
        // section the user opened or activated should move the outer sidebar.
        let reveal_section = explicit
            || (!other_focus
                && (resized
                    || (reveal.is_some() && previous.is_some_and(|previous| previous.active))));
        if resized && !other_focus {
            reveal = Some(self.nav.page);
        }
        if !ui.is_sizing_pass() {
            self.reveal_page = None;
            self.reveal_page_section = false;
        }
        let salt = if self.library.thumbnails {
            "page-previews"
        } else {
            "page-list"
        };
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(salt)
            .min_scrolled_height(height)
            .max_height(height)
            .animated(false);
        let row_resized = previous.filter(|previous| {
            previous.previews == self.library.thumbnails && previous.row_height != row_height
        });
        if reveal.is_some() || row_resized.is_some() {
            let id = ui.make_persistent_id(egui::Id::new(salt));
            let mut offset =
                egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |state| state.offset.y);
            if let Some(previous) = row_resized {
                // Keep the same page under the viewport when responsive cards
                // change height, including after deliberate manual scrolling.
                offset *= (row_height + ui.spacing().item_spacing.y)
                    / (previous.row_height + ui.spacing().item_spacing.y);
            }
            if let Some(page) = reveal {
                let top = page as f32 * (row_height + ui.spacing().item_spacing.y);
                offset = reveal_offset(offset, top, row_height, height);
            }
            scroll = scroll.vertical_scroll_offset(offset);
        }
        let page_before = self.nav.page;
        let mut focus = ListFocus::new(ui, salt, row_height, height);
        let scroll = focus.scroll(ui, scroll);
        let output = if self.library.thumbnails {
            self.page_previews(ui, layout, scroll, &mut focus)
        } else {
            self.page_list(ui, row_height, scroll, &mut focus)
        };
        let bottom = ui.min_rect().bottom();
        if reveal_section {
            ui.scroll_to_rect(
                egui::Rect::from_min_max(heading.min, egui::pos2(heading.right(), bottom)),
                None,
            );
        }
        if !ui.is_sizing_pass() {
            let clip = ui.clip_rect().intersect(egui::Rect::from_min_max(
                output.inner_rect.min,
                egui::pos2(output.inner_rect.right(), bottom),
            ));
            let top = output.inner_rect.top()
                + self.nav.page as f32 * (row_height + ui.spacing().item_spacing.y)
                - output.state.offset.y;
            let selected_visible = top >= clip.top() && top + row_height <= clip.bottom();
            let active = explicit
                || (!other_focus
                    && (self.nav.page != page_before
                        || previous.map_or(self.library.thumbnails, |previous| previous.active)));
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    viewport_id,
                    PageViewport {
                        document: self.id,
                        size: viewport,
                        row_height,
                        previews: self.library.thumbnails,
                        selected_visible,
                        active,
                    },
                )
            });
        }
        focus.finish(ui);
    }

    fn page_list(
        &mut self,
        ui: &mut egui::Ui,
        row_height: f32,
        scroll: egui::ScrollArea,
        focus: &mut ListFocus,
    ) -> egui::scroll_area::ScrollAreaOutput<()> {
        scroll.show_rows(ui, row_height, self.nav.count, |ui, range| {
            for page in range {
                let response = ui
                    .add_sized(
                        Vec2::new(ui.available_width(), row_height),
                        egui::Button::selectable(
                            page == self.nav.page,
                            self.labels.sidebar_caption(page),
                        )
                        .truncate(),
                    )
                    .on_hover_ui(|ui| {
                        ui.set_max_width(ui.spacing().tooltip_width);
                        ui.label(self.labels.sidebar_tooltip(page));
                    });
                focus.observe(&response, page);
                if response.clicked() {
                    self.jump(page);
                    self.reveal_page = None;
                }
            }
        })
    }

    fn page_previews(
        &mut self,
        ui: &mut egui::Ui,
        layout: crate::thumbnails::PreviewLayout,
        scroll: egui::ScrollArea,
        focus: &mut ListFocus,
    ) -> egui::scroll_area::ScrollAreaOutput<()> {
        let mut target = None;
        let output = scroll.show_rows(ui, layout.row_height, self.nav.count, |ui, range| {
            for page in range {
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), layout.row_height),
                    egui::Sense::click(),
                );
                focus.observe(&response, page);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        page == self.nav.page,
                        self.labels.sidebar_tooltip(page),
                    )
                });
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let dimensions = self.dimensions[page];
                let key = crate::thumbnails::key(
                    page,
                    dimensions,
                    self.reading.rotation,
                    self.library.night,
                    ui.ctx().pixels_per_point(),
                    layout.image_size,
                );
                let texture = self.thumbnails.want(key);
                let painter = ui.painter();
                if page == self.nav.page {
                    painter.rect_filled(rect, 8.0, ui.visuals().selection.bg_fill);
                    painter.rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(1.0_f32, appearance::accent(ui)),
                        egui::StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                }
                let box_size = layout.image_size;
                let center = egui::pos2(rect.center().x, rect.top() + box_size[1] * 0.5 + 4.0);
                let rotated = if key.rotation % 2 == 1 {
                    [dimensions[1], dimensions[0]]
                } else {
                    dimensions
                };
                let scale = Fit::Page.scale(rotated, box_size);
                let image_rect = egui::Rect::from_center_size(
                    center,
                    Vec2::new(rotated[0] * scale, rotated[1] * scale),
                );
                painter.rect_filled(
                    image_rect,
                    0.0,
                    self.page_colors.palette(key.night).background.color(),
                );
                if let Some(texture) = texture {
                    painter.image(
                        texture,
                        image_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    painter.text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        if self.thumbnails.failed(key) {
                            "Unavailable"
                        } else {
                            "Loading…"
                        },
                        egui::FontId::proportional(12.0),
                        self.page_colors.palette(key.night).foreground.color(),
                    );
                }
                caption::paint(
                    painter,
                    rect,
                    self.labels.sidebar_caption(page),
                    ui.visuals().text_color(),
                );
                if response.has_focus() {
                    // Keep keyboard focus distinct from the current page's
                    // selection, and paint it above the image and caption.
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        4.0,
                        egui::Stroke::new(2.0_f32, ui.style().interact(&response).fg_stroke.color),
                        egui::StrokeKind::Inside,
                    );
                }
                if response
                    .on_hover_ui(|ui| {
                        ui.set_max_width(ui.spacing().tooltip_width);
                        ui.label(self.labels.sidebar_tooltip(page));
                    })
                    .clicked()
                {
                    target = Some(page);
                }
            }
        });
        if let Some(page) = target {
            self.jump(page);
            // The clicked row is already visible. Keep the preview scroll stable
            // so adjacent pages remain under the pointer after navigation.
            self.reveal_page = None;
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{destination::Destination, navigation::Navigation, outline::OutlineEntry};
    use lopdf::dictionary;

    fn labels_with_prefix(prefix: &str) -> crate::page_labels::PageLabels {
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../../../assets/pagewing-guide.pdf"))
                .unwrap();
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend(prefix.encode_utf16().flat_map(u16::to_be_bytes));
        let labels = document.add_object(lopdf::dictionary! {
            "Nums" => vec![lopdf::Object::Integer(0), lopdf::dictionary! {
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
        crate::page_labels::PageLabels::read(&hayro::hayro_syntax::Pdf::new(bytes).unwrap())
    }

    fn preview_app(outline: bool) -> (tempfile::TempDir, egui::Context, Pagewing) {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(80, 79);
        app.dimensions = vec![[600.0, 800.0]; 80];
        if outline {
            app.outline.set_entries(
                (0..80)
                    .map(|page| OutlineEntry {
                        title: format!("Chapter {page:03}"),
                        destination: Some(Destination::page(page)),
                        depth: 0,
                    })
                    .collect(),
            );
            app.reading.bookmarks.extend(0..60);
            app.refresh_bookmarks();
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
                app.sidebar(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        )
    }

    fn settle(app: &mut Pagewing, context: &egui::Context, size: Vec2) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        for _ in 0..6 {
            output = frame(app, context, size, Vec::new());
        }
        output
    }

    #[test]
    fn keyboard_preview_focus_is_visible_and_names_its_target_without_navigating() {
        let (_directory, context, mut app) = preview_app(false);
        let size = egui::vec2(1180.0, 820.0);
        app.nav.jump(0);
        app.library.thumbnails = true;
        app.outline.set_entries(vec![OutlineEntry {
            title: "Keyboard chapter".into(),
            destination: Some(Destination::page(0)),
            depth: 0,
        }]);
        app.focus_contents();
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
        let mut named = false;
        for _ in 0..20 {
            let output = frame(&mut app, &context, size, key(egui::Key::Tab));
            named |= output.platform_output.events.iter().any(|event| {
                matches!(event, egui::output::OutputEvent::FocusGained(info)
                    if info.typ == egui::WidgetType::Button
                        && info.label.as_deref() == Some("Go to page 2")
                        && info.selected == Some(false))
            });
            let output = settle(&mut app, &context, size);
            let label = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Page 2" => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            });
            if let Some(id) = context.memory(|memory| memory.focused())
                && context.read_response(id).is_some_and(|response| {
                    response.rect.height() <= crate::thumbnails::ROW_HEIGHT
                        && label.is_some_and(|label| response.rect.contains_rect(label))
                })
            {
                focused = Some(id);
                break;
            }
        }
        let focused = focused.expect("Tab must reach the second preview card");
        for dark in [false, true] {
            context.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            for size in [size, egui::vec2(240.0, 240.0)] {
                let output = settle(&mut app, &context, size);
                assert_eq!(app.nav.page, 0, "focus must not change the reading page");
                assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
                let response = context.read_response(focused).unwrap();
                assert!(
                    output.shapes.iter().any(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) => {
                            rect.rect == response.rect.shrink(1.0)
                                && rect.stroke.width >= 2.0
                                && rect.stroke.color
                                    == context.style().visuals.widgets.active.fg_stroke.color
                                && shape.clip_rect.contains_rect(rect.rect)
                        }
                        _ => false,
                    }),
                    "the focused preview must have a visible border at {size:?}, dark={dark}"
                );
            }
        }
        assert!(
            named,
            "focus feedback must identify the preview's page and selection state"
        );
        frame(&mut app, &context, size, key(egui::Key::Space));
        assert_eq!(
            app.nav.page, 1,
            "Space activates the visibly focused preview"
        );
        frame(&mut app, &context, size, key(egui::Key::Escape));
        let output = settle(&mut app, &context, size);
        assert!(
            !output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.width >= 2.0)
            }),
            "returning to the reader removes the preview's focus border"
        );
    }

    #[test]
    fn resizing_a_virtual_list_preserves_the_focused_page_without_navigating() {
        let (_directory, context, mut app) = preview_app(false);
        let size = egui::vec2(1180.0, 820.0);
        app.nav.jump(0);
        app.outline.set_entries(vec![OutlineEntry {
            title: "Keyboard chapter".into(),
            destination: Some(Destination::page(0)),
            depth: 0,
        }]);
        app.focus_contents();
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
        for _ in 0..50 {
            frame(&mut app, &context, size, key(egui::Key::Tab));
            let output = settle(&mut app, &context, size);
            let label = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Page 25" => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            });
            if let Some(id) = context.memory(|memory| memory.focused())
                && context.read_response(id).is_some_and(|response| {
                    label.is_some_and(|label| response.rect.contains_rect(label))
                })
            {
                focused = Some(id);
                break;
            }
        }
        let focused = focused.expect("Tab must reach a later page row");
        for (resized, font_size) in [(egui::vec2(240.0, 240.0), 14.0), (size, 28.0), (size, 14.0)] {
            context.style_mut(|style| {
                style.text_styles.insert(
                    egui::TextStyle::Button,
                    egui::FontId::proportional(font_size),
                );
            });
            settle(&mut app, &context, resized);
            assert_eq!(app.nav.page, 0, "resizing must preserve the reading page");
            assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
            let response = context.read_response(focused).unwrap();
            assert!(
                response
                    .interact_rect
                    .contains_rect(response.rect.shrink(1.0)),
                "the focused page must be visible at {resized:?}"
            );
        }
        frame(&mut app, &context, size, key(egui::Key::Space));
        assert_eq!(app.nav.page, 24, "Space follows the retained page button");
        let labels = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with("Page ") => {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        shape
                            .clip_rect
                            .contains_rect(rect)
                            .then(|| (text.galley.text().to_owned(), rect))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let before = labels(&settle(&mut app, &context, size));
        let pos = context.read_response(focused).unwrap().rect.center();
        frame(
            &mut app,
            &context,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -180.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::PointerMoved(egui::pos2(1100.0, 60.0))],
        );
        let after = labels(&settle(&mut app, &context, size));
        assert_ne!(after, before, "manual scrolling must move the list");
        assert_eq!(
            labels(&settle(&mut app, &context, size)),
            after,
            "ordinary frames must preserve manual scrolling"
        );
        assert_eq!(app.nav.page, 24);
    }

    fn preview_key(app: &mut Pagewing, context: &egui::Context, size: Vec2) {
        frame(
            app,
            context,
            size,
            vec![egui::Event::Key {
                key: egui::Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.library.thumbnails);
    }

    fn assert_preview_fits(context: &egui::Context, output: &egui::FullOutput, size: Vec2) {
        let label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Page 80" => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("the current preview caption must be drawn");
        let card = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.fill == context.style().visuals.selection.bg_fill
                        && rect.rect.contains_rect(label) =>
                {
                    Some((shape.clip_rect, rect.rect))
                }
                _ => None,
            })
            .expect("the current preview card must be drawn");
        assert!(
            card.0.contains_rect(card.1),
            "{size:?}: card {:?} clipped by {:?}",
            card.1,
            card.0
        );
        let image = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.fill == Color32::WHITE && card.1.contains(rect.rect.center()) =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .expect("the current preview image must be drawn");
        assert!(
            card.1.contains_rect(image),
            "{size:?}: image {image:?} exceeds card {:?}",
            card.1
        );
        assert!(card.0.contains_rect(image));
    }

    #[test]
    fn narrow_and_short_windows_keep_the_whole_preview_and_caption_visible() {
        let (_directory, context, mut app) = preview_app(false);
        let size = Vec2::new(1180.0, 820.0);
        preview_key(&mut app, &context, size);
        for size in crate::app::dialog::TEST_SIZES {
            let output = settle(&mut app, &context, size);
            assert_preview_fits(&context, &output, size);
        }
    }

    #[test]
    fn long_pdf_labels_keep_physical_numbers_visible_in_lists_and_previews() {
        for previews in [false, true] {
            let (_directory, context, mut app) = preview_app(false);
            app.nav = Navigation::new(6, 5);
            app.dimensions.truncate(6);
            app.labels = labels_with_prefix("Long appendix 中文😀 title\nsecond line ");
            app.library.sidebar = true;
            app.library.thumbnails = previews;
            app.reveal_page = Some(app.nav.page);
            app.reveal_page_section = true;
            for size in crate::app::dialog::TEST_SIZES {
                let output = settle(&mut app, &context, size);
                let caption = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.job.text.starts_with("Page ")
                                && (text.galley.job.text.ends_with(" · 6")
                                    || text.galley.job.text.starts_with("Page 6")) =>
                        {
                            Some((shape.clip_rect, text))
                        }
                        _ => None,
                    })
                    .expect("the selected page caption must be drawn");
                let bounds = egui::Rect::from_min_size(caption.1.pos, caption.1.galley.size());
                assert!(
                    caption.0.contains_rect(bounds),
                    "caption {bounds:?} exceeds {:?}",
                    caption.0
                );
                assert_eq!(caption.1.galley.rows.len(), 1);
                let visible: String = caption.1.galley.rows[0]
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.chr)
                    .collect();
                assert!(
                    visible.starts_with("Page 6") || visible.ends_with(" · 6"),
                    "{size:?}, previews={previews}: physical page number must remain visible: {visible}"
                );
            }
            let size = Vec2::new(860.0, 540.0);
            let output = settle(&mut app, &context, size);
            let position = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.starts_with("Page 6") => {
                        Some(text.pos + egui::vec2(5.0, 5.0))
                    }
                    _ => None,
                })
                .unwrap();
            frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::PointerMoved(position)],
            );
            let output = settle(&mut app, &context, size);
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Text(text) => text
                        .galley
                        .job
                        .text
                        .contains("PDF page label: Long appendix 中文😀 title\nsecond line 6"),
                    _ => false,
                }),
                "hover must display the full retained label, including the suffix omitted from the caption"
            );
            if !previews {
                let output = frame(&mut app, &context, size, vec![egui::Event::PointerGone]);
                let position = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.job.text.starts_with("Page 5 ·")
                                && shape.clip_rect.contains(text.pos) =>
                        {
                            Some(text.pos + egui::vec2(5.0, 5.0))
                        }
                        _ => None,
                    })
                    .expect("the preceding page row must be visible");
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        &context,
                        size,
                        vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::PointerButton {
                                pos: position,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                assert_eq!(app.nav.page, 4, "the labeled list row must navigate");
            }
        }
    }

    #[test]
    fn preview_key_reveals_the_current_page_below_long_contents_and_bookmarks() {
        let (_directory, context, mut app) = preview_app(true);
        let size = Vec2::new(860.0, 540.0);
        preview_key(&mut app, &context, size);
        let output = settle(&mut app, &context, size);
        assert_preview_fits(&context, &output, size);
    }

    fn visible_captions(output: &egui::FullOutput) -> Vec<usize> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if shape
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())) =>
                {
                    text.galley.text().strip_prefix("Page ")?.parse().ok()
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn resizing_after_manual_preview_scrolling_preserves_the_visible_pages() {
        let (_directory, context, mut app) = preview_app(false);
        let size = Vec2::new(1180.0, 820.0);
        preview_key(&mut app, &context, size);
        let output = settle(&mut app, &context, size);
        let caption = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Page 80" => Some(text.pos),
                _ => None,
            })
            .unwrap();
        frame(
            &mut app,
            &context,
            size,
            vec![
                egui::Event::PointerMoved(caption),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, 1500.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let output = settle(&mut app, &context, size);
        let before = visible_captions(&output);
        assert!(!before.contains(&80));
        assert!(!before.is_empty());
        let size = Vec2::new(240.0, 400.0);
        let output = settle(&mut app, &context, size);
        let after = visible_captions(&output);
        assert!(
            !after.contains(&80),
            "resizing must preserve deliberate scrolling away"
        );
        assert!(
            after
                .iter()
                .any(|page| before.iter().any(|previous| page.abs_diff(*previous) <= 1)),
            "same pages should remain visible: {before:?} -> {after:?}"
        );
        app.jump(79);
        let output = settle(&mut app, &context, size);
        assert_preview_fits(&context, &output, size);
    }

    #[test]
    fn switching_back_to_list_reveals_the_current_page_in_a_short_window() {
        let (_directory, context, mut app) = preview_app(true);
        let size = Vec2::new(240.0, 240.0);
        preview_key(&mut app, &context, size);
        settle(&mut app, &context, size);
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::Key {
                key: egui::Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.library.thumbnails);
        let output = settle(&mut app, &context, size);
        assert!(visible_captions(&output).contains(&80));
        assert_eq!(app.nav.page, 79);
    }

    #[test]
    fn resizing_the_default_list_keeps_bookmarks_in_view() {
        let (_directory, context, mut app) = preview_app(false);
        app.library.sidebar = true;
        app.reading.bookmarks.extend(0..80);
        app.refresh_bookmarks();
        app.jump(79);
        for size in [Vec2::new(1180.0, 820.0), Vec2::new(860.0, 540.0)] {
            let output = settle(&mut app, &context, size);
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "BOOKMARKS" => shape
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())),
                    _ => false,
                }),
                "the default page list must not scroll the bookmark section away"
            );
        }
    }

    #[test]
    fn clicking_bookmarks_and_chapters_keeps_their_section_visible_after_using_previews() {
        let (_directory, context, mut app) = preview_app(true);
        let size = Vec2::new(1180.0, 820.0);
        preview_key(&mut app, &context, size);
        for (label, page) in [("Page 1", 0), ("Chapter 001", 1)] {
            app.reveal_page = Some(app.nav.page);
            app.reveal_page_section = true;
            let output = settle(&mut app, &context, size);
            let heading = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "PAGES" => Some(text.pos),
                    _ => None,
                })
                .unwrap();
            frame(
                &mut app,
                &context,
                size,
                vec![
                    egui::Event::PointerMoved(heading + egui::vec2(5.0, 5.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0.0, 6000.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            let output = settle(&mut app, &context, size);
            let position = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.text() == label && shape.clip_rect.contains(text.pos) =>
                    {
                        Some(text.pos + egui::vec2(5.0, 5.0))
                    }
                    _ => None,
                })
                .expect("navigation target must be visible");
            frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::PointerMoved(position)],
            );
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &context,
                    size,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let output = settle(&mut app, &context, size);
            assert_eq!(app.nav.page, page);
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => shape
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())),
                    _ => false,
                }),
                "page previews must not scroll the clicked {label} away"
            );
        }
    }
}
