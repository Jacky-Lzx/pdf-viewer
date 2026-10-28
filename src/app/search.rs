use super::{Pagewing, appearance, shortcuts, toolbar};
use eframe::egui::{self, Key};
use std::time::{Duration, Instant};

impl Pagewing {
    pub(super) fn search_button(&mut self, action: shortcuts::SearchButton) {
        use shortcuts::SearchButton;
        match action {
            SearchButton::Case | SearchButton::Word => {
                let option = if action == SearchButton::Case {
                    &mut self.library.search_options.match_case
                } else {
                    &mut self.library.search_options.whole_word
                };
                *option = !*option;
                self.dirty = true;
                self.search_focus = true;
                self.change_search();
            }
            SearchButton::Previous => self.next_hit(true),
            SearchButton::Next => self.next_hit(false),
            SearchButton::Results => {
                self.library.sidebar = true;
                self.reveal_hit = Some(self.hit_index.unwrap_or(0));
                self.dirty = true;
            }
            SearchButton::Close => {
                self.search_open = false;
                self.pending_search_jump = None;
            }
        }
    }

    pub(super) fn restart_search(&mut self) {
        self.revision += 1;
        self.hits.clear();
        self.result_text.clear();
        self.hit_index = None;
        self.reveal_hit = None;
        self.scanned = 0;
        self.scan_total = self.nav.count;
        self.search_complete = self.query.trim().is_empty();
        self.search_limited = false;
        self.search_due = (!self.search_complete).then_some(Instant::now());
    }

    pub(super) fn change_search(&mut self) {
        self.pending_search_jump = None;
        self.restart_search();
        self.worker
            .search(self.revision, String::new(), self.library.search_options);
        self.search_due =
            (!self.search_complete).then_some(Instant::now() + Duration::from_millis(180));
    }

    pub(super) fn find(&mut self) {
        self.zoom_input = None;
        self.outline.cancel_input();
        let selected = self
            .selection
            .selected_text()
            .map(str::trim)
            .filter(|text| !text.is_empty() && !text.contains(['\n', '\r']))
            .map(str::to_owned);
        self.recent_picker = None;
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.help = false;
        self.properties_open = false;
        self.page_input = None;
        self.password_input = None;
        self.search_open = true;
        self.search_focus = true;
        if let Some(query) = selected {
            self.query = query;
            self.change_search();
        }
    }

    pub(super) fn next_hit(&mut self, reverse: bool) {
        self.next_hits(reverse, 1);
    }

    pub(super) fn next_hits(&mut self, reverse: bool, count: usize) {
        let count = count.max(1);
        if self.hits.is_empty() {
            self.pending_search_jump =
                (!self.search_complete || self.search_due.is_some()).then_some((reverse, count));
            return;
        }
        let (start, steps) = match self.hit_index {
            Some(index) => (index, count),
            None => match crate::text::initial_match(
                &self.hits,
                self.nav.page,
                reverse,
                self.scanned,
                self.search_complete,
            ) {
                Some(index) => (index, count - 1),
                None => {
                    self.pending_search_jump = (!self.search_complete).then_some((reverse, count));
                    return;
                }
            },
        };
        let Some(index) = crate::text::advance_match(
            start,
            self.hits.len(),
            reverse,
            steps,
            self.search_complete,
        ) else {
            self.pending_search_jump = (!self.search_complete).then_some((reverse, count));
            return;
        };
        self.pending_search_jump = None;
        self.select_hit(index);
    }

    pub(super) fn select_hit(&mut self, index: usize) {
        let current = self.current_position();
        self.pending_search_jump = None;
        self.hit_index = Some(index);
        self.reveal_hit = Some(index);
        let page = self.hits[index].page;
        let rect = self.hits[index].rects.first().copied();
        self.jump(page);
        self.history.record(current);
        self.pending_focus = rect;
    }

    pub(super) fn search_bar(&mut self, context: &egui::Context) {
        if !self.search_open {
            return;
        }
        egui::TopBottomPanel::top("search")
            .frame(appearance::panel(context, 4))
            .exact_height(42.0)
            .show(context, |ui| {
                toolbar::horizontal_bar(ui, "search-controls", |ui| {
                    ui.add_space(16.0);
                    ui.label("Find");
                    let (response, focused_now) = shortcuts::text_input(
                        ui,
                        egui::Id::new("pagewing-search-input"),
                        &mut self.search_focus,
                        &mut self.deferred_input,
                        egui::TextEdit::singleline(&mut self.query)
                            .desired_width((ui.available_width() - 600.0).clamp(100.0, 280.0))
                            .hint_text("Search this document…"),
                    );
                    self.input_buttons.search_case = Some(
                        ui.selectable_label(self.library.search_options.match_case, "Aa")
                            .on_hover_text("Match case · Alt+C")
                            .id,
                    );
                    self.input_buttons.search_word = Some(
                        ui.selectable_label(self.library.search_options.whole_word, "Word")
                            .on_hover_text("Whole words · Alt+W")
                            .id,
                    );
                    if response.changed() {
                        self.change_search();
                    }
                    // egui compares focus with the previous frame. Acquiring
                    // and releasing it with Enter in one frame is not lost_focus.
                    if (response.lost_focus() || focused_now)
                        && let Some(reverse) = ui.input(|input| {
                            input.events.iter().find_map(|event| match event {
                                egui::Event::Key {
                                    key: Key::Enter,
                                    pressed: true,
                                    modifiers,
                                    ..
                                } => Some(modifiers.shift),
                                _ => None,
                            })
                        })
                    {
                        self.next_hit(reverse);
                    }
                    self.input_buttons.search_previous = Some(ui.button("Prev").id);
                    self.input_buttons.search_next = Some(ui.button("Next").id);
                    let index = self.hit_index.map(|i| i + 1).unwrap_or(0);
                    ui.label(format!(
                        "{index} / {}{} matches",
                        self.hits.len(),
                        if self.search_limited { "+" } else { "" }
                    ));
                    if !self.search_complete && !self.query.trim().is_empty() {
                        ui.spinner();
                        if ui.available_width() > 180.0 {
                            ui.weak(format!("{} / {} pages", self.scanned, self.scan_total));
                        }
                    }
                    self.input_buttons.search_results = Some(ui.button("Results").id);
                    self.input_buttons.close_search = Some(ui.button("×").id);
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::super::Pagewing;
    use crate::navigation::{Fit, Navigation};
    use eframe::egui::{self, Event, Key, Modifiers};

    fn key(key: Key) -> Vec<Event> {
        let mut events: Vec<_> = [true, false]
            .into_iter()
            .map(|pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            })
            .collect();
        if key == Key::Space {
            events.insert(1, Event::Text(" ".into()));
        }
        events
    }

    fn frame(app: &mut Pagewing, context: &egui::Context, events: Vec<Event>) {
        sized_frame(app, context, egui::vec2(1180.0, 820.0), events);
    }

    fn sized_frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        events: Vec<Event>,
    ) {
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.toolbar(context);
                app.search_bar(context);
                app.status(context);
                app.canvas(context);
            },
        );
    }

    fn reader(path: std::path::PathBuf) -> (Pagewing, egui::Context) {
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(&context, None, None, path);
        app.nav = Navigation::new(6, 2);
        app.dimensions = vec![[600.0, 800.0]; 6];
        app.reading.fit = Fit::Custom(3.0);
        app.find();
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        app.pan = egui::vec2(40.0, 80.0);
        (app, context)
    }

    #[test]
    fn space_on_search_options_toggles_the_option_without_scrolling_the_page() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let before = app.pan;
        frame(&mut app, &context, key(Key::Tab));
        assert!(context.memory(|memory| memory.has_focus(app.input_buttons.search_case.unwrap())));
        frame(&mut app, &context, key(Key::Space));
        assert!(app.library.search_options.match_case);
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.pan, before);
        assert_eq!(app.query, "");
    }

    #[test]
    fn narrow_search_bar_keeps_each_tabbed_button_visible() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let size = egui::vec2(240.0, 240.0);
        for _ in 0..4 {
            sized_frame(&mut app, &context, size, Vec::new());
        }
        let ids = [
            app.input_buttons.search_case.unwrap(),
            app.input_buttons.search_word.unwrap(),
            app.input_buttons.search_previous.unwrap(),
            app.input_buttons.search_next.unwrap(),
            app.input_buttons.search_results.unwrap(),
            app.input_buttons.close_search.unwrap(),
        ];
        for id in ids {
            sized_frame(&mut app, &context, size, key(Key::Tab));
            for _ in 0..4 {
                sized_frame(&mut app, &context, size, Vec::new());
            }
            assert!(context.memory(|memory| memory.has_focus(id)));
            let rect = context.read_response(id).unwrap().rect;
            assert!(
                rect.left() >= 0.0 && rect.right() <= size.x,
                "focused search button must be visible: {rect:?}"
            );
        }
        let close = app.input_buttons.close_search.unwrap();
        for width in [1180.0, 360.0, 240.0] {
            let size = egui::vec2(width, 240.0);
            for _ in 0..4 {
                sized_frame(&mut app, &context, size, Vec::new());
            }
            let response = context.read_response(close).unwrap();
            assert!(response.has_focus());
            assert!(response.rect.left() >= 0.0 && response.rect.right() <= width);
        }
    }

    #[test]
    fn tab_and_shift_tab_traverse_search_controls_without_toggling_the_sidebar() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let sidebar = app.library.sidebar;
        let ids = [
            app.input_buttons.search_case.unwrap(),
            app.input_buttons.search_word.unwrap(),
            app.input_buttons.search_previous.unwrap(),
            app.input_buttons.search_next.unwrap(),
            app.input_buttons.search_results.unwrap(),
            app.input_buttons.close_search.unwrap(),
        ];
        for id in ids {
            frame(&mut app, &context, key(Key::Tab));
            frame(&mut app, &context, Vec::new());
            assert!(context.memory(|memory| memory.has_focus(id)));
            assert_eq!(app.library.sidebar, sidebar);
        }
        for id in ids[..5]
            .iter()
            .rev()
            .copied()
            .chain([egui::Id::new("pagewing-search-input")])
        {
            let mut events = key(Key::Tab);
            for event in &mut events {
                if let Event::Key { modifiers, .. } = event {
                    *modifiers = Modifiers::SHIFT;
                }
            }
            frame(&mut app, &context, events);
            frame(&mut app, &context, Vec::new());
            assert!(context.memory(|memory| memory.has_focus(id)));
            assert_eq!(app.library.sidebar, sidebar);
        }
    }

    #[test]
    fn every_search_button_handles_enter_and_space_before_reader_commands() {
        use super::shortcuts::SearchButton;
        for activation in [Key::Enter, Key::Space] {
            for action in [
                SearchButton::Case,
                SearchButton::Word,
                SearchButton::Previous,
                SearchButton::Next,
                SearchButton::Results,
                SearchButton::Close,
            ] {
                let directory = tempfile::tempdir().unwrap();
                let (mut app, context) = reader(directory.path().join("library.json"));
                app.query = "Rust".into();
                app.search_complete = true;
                app.search_due = None;
                app.hits = (1..=3)
                    .map(|page| crate::text::SearchHit {
                        page,
                        rects: Vec::new(),
                        snippet: "Rust".into(),
                        snippet_match: 0..4,
                    })
                    .collect();
                app.hit_index = Some(1);
                app.library.sidebar = false;
                let id = match action {
                    SearchButton::Case => app.input_buttons.search_case,
                    SearchButton::Word => app.input_buttons.search_word,
                    SearchButton::Previous => app.input_buttons.search_previous,
                    SearchButton::Next => app.input_buttons.search_next,
                    SearchButton::Results => app.input_buttons.search_results,
                    SearchButton::Close => app.input_buttons.close_search,
                }
                .unwrap();
                context.memory_mut(|memory| memory.request_focus(id));
                // Settle focus before activating, as with a real Tab press.
                frame(&mut app, &context, Vec::new());
                let before = app.pan;
                let revision = app.revision;
                frame(&mut app, &context, key(activation));
                assert_eq!(app.query, "Rust", "{action:?}, {activation:?}");
                let expected_page = match action {
                    SearchButton::Previous => 1,
                    SearchButton::Next => 3,
                    _ => 2,
                };
                assert_eq!(app.nav.page, expected_page, "{action:?}, {activation:?}");
                match action {
                    SearchButton::Case | SearchButton::Word => {
                        assert_eq!(
                            app.library.search_options.match_case,
                            action == SearchButton::Case
                        );
                        assert_eq!(
                            app.library.search_options.whole_word,
                            action == SearchButton::Word
                        );
                        assert_eq!(app.revision, revision + 1);
                        assert!(app.hits.is_empty() && app.hit_index.is_none());
                        assert!(context.memory(|memory| {
                            memory.has_focus(egui::Id::new("pagewing-search-input"))
                        }));
                    }
                    SearchButton::Previous => assert_eq!(app.hit_index, Some(0)),
                    SearchButton::Next => assert_eq!(app.hit_index, Some(2)),
                    SearchButton::Results => assert!(app.library.sidebar),
                    SearchButton::Close => {
                        assert!(!app.search_open);
                        assert!(app.pending_search_jump.is_none());
                    }
                }
                if !matches!(action, SearchButton::Previous | SearchButton::Next) {
                    assert_eq!(app.pan, before, "{action:?}, {activation:?}");
                }
            }
        }
    }

    #[test]
    fn option_click_routes_same_frame_text_and_enter_to_the_new_query() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let pos = context
            .read_response(app.input_buttons.search_word.unwrap())
            .unwrap()
            .rect
            .center();
        let mut events = vec![Event::PointerMoved(pos)];
        events.extend([true, false].map(|pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }));
        events.push(Event::Paste("Rust".into()));
        events.extend(key(Key::Enter));
        let before = app.pan;
        frame(&mut app, &context, events);
        assert!(app.library.search_options.whole_word);
        assert_eq!(app.query, "Rust");
        assert_eq!(app.pending_search_jump, Some((false, 1)));
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.pan, before);
    }

    #[test]
    fn option_click_keeps_query_focus_for_enter_on_the_following_frame() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        app.query = "Rust".into();
        context.memory_mut(|memory| memory.surrender_focus(egui::Id::new("pagewing-search-input")));
        frame(&mut app, &context, Vec::new());
        let pos = context
            .read_response(app.input_buttons.search_word.unwrap())
            .unwrap()
            .rect
            .center();
        let mut events = vec![Event::PointerMoved(pos)];
        events.extend([true, false].map(|pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }));
        frame(&mut app, &context, events);
        assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-search-input"))));
        frame(&mut app, &context, key(Key::Enter));
        assert_eq!(app.pending_search_jump, Some((false, 1)));
        assert_eq!(app.query, "Rust");
    }
}
