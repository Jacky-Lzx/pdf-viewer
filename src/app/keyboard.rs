use super::{MarkAction, Pagewing, input_dialogs, set_theme, shortcuts, zoom};
use crate::input_actions::{self, ReaderAction};
use crate::{links::LinkTarget, navigation::Fit};
use eframe::egui::{self, Key, Vec2};

#[cfg(test)]
mod tests;

impl Pagewing {
    pub(super) fn keys(&mut self, context: &egui::Context) {
        // Refresh a held Undo before dispatching its activation. A quiet window
        // may have spent longer than the notice timeout between input frames.
        self.update_notice_interaction(context);
        let search_input_focused = self.search_open
            && (self.search_focus
                || context
                    .memory(|memory| memory.has_focus(egui::Id::new("pagewing-search-input"))));
        // Apply button actions before routing this frame's keyboard input.
        // Otherwise a click that releases search focus can send following text
        // to reader commands before the new input has opened.
        match self.input_buttons.clicked(context) {
            Some(shortcuts::InputButton::CurrentChapter) => self.show_current_chapter(context),
            Some(shortcuts::InputButton::ReadingControls) => self.toggle_reading_controls(context),
            Some(shortcuts::InputButton::Help) => {
                if self.help {
                    self.close_help(context);
                } else {
                    self.open_help(context);
                }
            }
            Some(shortcuts::InputButton::Find) => self.find(),
            Some(shortcuts::InputButton::Menu(id)) => {
                if let Some(response) = context.read_response(id) {
                    egui::Popup::toggle_id(context, egui::Popup::default_response_id(&response));
                }
            }
            Some(shortcuts::InputButton::History(forward)) => {
                self.toolbar_history(context, forward)
            }
            Some(shortcuts::InputButton::Search(action)) => {
                self.search_button(action);
                if action == shortcuts::SearchButton::Results {
                    self.focus_search_results(context);
                }
                if matches!(
                    action,
                    shortcuts::SearchButton::Copy
                        | shortcuts::SearchButton::Stop
                        | shortcuts::SearchButton::Restart
                ) && search_input_focused
                {
                    // Reading button responses can release text focus before
                    // TextEdit consumes queued edits. Apply those edits before
                    // copying results or stopping/restarting the latest query.
                    self.search_focus = true;
                }
            }
            Some(shortcuts::InputButton::Comments) => {
                self.set_comments_open(context, !self.comments.open);
            }
            Some(shortcuts::InputButton::CloseComments) => self.set_comments_open(context, false),
            Some(shortcuts::InputButton::Page) => self.open_goto(),
            Some(shortcuts::InputButton::Zoom) => self.open_zoom(context),
            Some(shortcuts::InputButton::RestoreDeleted) => self.restore_deleted(),
            Some(shortcuts::InputButton::ReloadChanged) => {
                if self.file_changed {
                    self.reload(context);
                }
            }
            Some(shortcuts::InputButton::DismissChange) => {
                self.file_changed = false;
                self.auto_reload_pending = false;
            }
            Some(shortcuts::InputButton::RetryError) => self.retry_error(context),
            Some(shortcuts::InputButton::DismissError) => self.error = None,
            Some(shortcuts::InputButton::CopyError) => {
                if let Some(error) = &self.error {
                    context.copy_text(error.message.clone());
                }
            }
            Some(shortcuts::InputButton::DismissConfig) => self.configuration_error = None,
            Some(shortcuts::InputButton::CopyConfig) => {
                if let Some(error) = &self.configuration_error {
                    context.copy_text(error.clone());
                }
            }
            None => {}
        }
        if context.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary)) {
            for (visible, name) in [
                (self.search_open, "pagewing-search-input"),
                (self.recent_picker.is_some(), "pagewing-recent-input"),
                (self.page_input.is_some(), "pagewing-page-input"),
                (self.zoom_input.is_some(), "pagewing-zoom-input"),
                (self.password_input.is_some(), "pagewing-password-input"),
                (self.help, "pagewing-help-input"),
                (
                    self.comments.open && self.comments.editor.is_some(),
                    "pagewing-comment-input",
                ),
                (self.comments.open, "pagewing-comment-filter"),
                (
                    self.library.sidebar && self.nav.count > 0 && !self.outline.is_empty(),
                    "pagewing-outline-input",
                ),
            ] {
                if visible && shortcuts::focus_clicked_input(context, egui::Id::new(name)) {
                    // These dialogs also need to recognize Enter when focus
                    // was gained and released within this very same frame.
                    match name {
                        "pagewing-search-input" => self.search_focus = true,
                        "pagewing-page-input" => {
                            if let Some(input) = &mut self.page_input {
                                input.focus = true;
                            }
                        }
                        "pagewing-password-input" => {
                            if let Some(input) = &mut self.password_input {
                                input.focus = true;
                            }
                        }
                        "pagewing-zoom-input" => {
                            if let Some(input) = &mut self.zoom_input {
                                input.focus = true;
                            }
                        }
                        _ => {}
                    }
                    break;
                }
            }
        }
        // Preserve action order, including side-button history mixed with keys.
        // Text, paste, IME and primary pointer events stay in egui.
        let mut actions = std::mem::take(&mut self.reader_actions);
        context.input(|input| actions.extend(input_actions::reader_actions(input)));
        for event in actions.drain(..) {
            // Opening/closing an input changes how later keys in this same
            // frame must be routed, before its widget has been drawn.
            let typing = (self.search_open && self.search_focus)
                || (self.help && self.help_filter.focus)
                || self.outline.focus
                || self.page_input.is_some()
                || self.zoom_input.is_some()
                || self.password_input.is_some()
                || self.recent_picker.is_some()
                || (self.comments.open && self.comments.editor.is_some())
                || (self.comments.open
                    && context.memory(|memory| {
                        ["pagewing-comment-input", "pagewing-comment-filter"]
                            .iter()
                            .any(|name| memory.has_focus(egui::Id::new(name)))
                    }))
                || context.memory(|memory| {
                    [
                        "pagewing-search-input",
                        "pagewing-recent-input",
                        "pagewing-page-input",
                        "pagewing-zoom-input",
                        "pagewing-password-input",
                        "pagewing-help-input",
                        "pagewing-outline-input",
                    ]
                    .iter()
                    .any(|name| memory.has_focus(egui::Id::new(name)))
                });
            let popup_open = egui::Popup::is_any_open(context);
            if let ReaderAction::History { forward } = event {
                if !typing && !self.help && !self.properties_open && !popup_open {
                    self.nav.clear_prefix();
                    self.mark_action = None;
                    self.history(forward);
                }
                continue;
            }
            if event == ReaderAction::Copy
                && !typing
                && !popup_open
                && !self.help
                && !self.properties_open
                && super::copy_menu::request_copy(context)
            {
                continue;
            }
            if event == ReaderAction::Copy && !typing && !popup_open && self.selection.active {
                self.copy_selection(context);
                context.input_mut(|input| {
                    input
                        .events
                        .retain(|event| !matches!(event, egui::Event::Copy))
                });
                continue;
            }
            let ReaderAction::Key {
                key,
                modifiers,
                text: key_text,
            } = event
            else {
                continue;
            };
            let symbol = key_text.character();
            let raw_key = key;
            let raw_modifiers = modifiers;
            if !typing && self.result_navigation_key(context, key, modifiers) {
                continue;
            }
            if !typing
                && !popup_open
                && !self.help
                && !self.properties_open
                && super::copy_menu::open(context, key, modifiers)
            {
                self.nav.clear_prefix();
                self.hints.cancel();
                self.mark_action = None;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.nav.count == 0
                && !typing
                && !(self.help || self.properties_open || popup_open)
                && !modifiers.command
                && !modifiers.ctrl
                && !modifiers.alt
                && (key == Key::Tab || (!modifiers.shift && matches!(key, Key::Enter | Key::Space)))
            {
                // Welcome controls use normal focus traversal and activation,
                // including when the reader's sidebar binding is disabled.
                continue;
            }
            if key == Key::Tab && self.search_control_focused(context) {
                // Tab traverses the search controls while they own focus.
                // In the reader it retains its sidebar-toggle binding.
                continue;
            }
            if !popup_open
                && !self.help
                && !self.properties_open
                && !modifiers.command
                && !modifiers.ctrl
                && !modifiers.alt
                && (key == Key::Tab
                    || (!typing && !modifiers.shift && matches!(key, Key::Enter | Key::Space)))
                && (self.comment_control_focused(context)
                    || self.sidebar_control_focused(context)
                    || self.toolbar_control_focused(context)
                    || self.status_control_focused(context)
                    || self.search_control_focused(context))
            {
                // Reader bars and sidebars own focus traversal and button
                // activation, leaving the reading view and shortcuts alone.
                continue;
            }
            // Adapt configured bindings to the existing reader commands, but
            // consume the original physical press so text cannot leak to egui.
            // Sequences and text fields retain their own keyboard vocabulary.
            let reader_ready = !typing
                && !self.help
                && !self.properties_open
                && !popup_open
                && !self.hints.active
                && self.mark_action.is_none()
                && !self.nav.pending_z;
            let binding = crate::settings::Binding::from_event(key, modifiers, symbol);
            let (key, modifiers, key_text) =
                match self.library.shortcuts.resolve(&binding, !reader_ready) {
                    crate::settings::Resolution::Mapped(key, modifiers) => {
                        (key, modifiers, shortcuts::KeyText::None)
                    }
                    crate::settings::Resolution::Disabled => {
                        shortcuts::consume_reader_shortcut(context, raw_key, raw_modifiers);
                        continue;
                    }
                    crate::settings::Resolution::Unmatched => (key, modifiers, key_text),
                };
            if popup_open && key == Key::Escape {
                egui::Popup::close_all(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if popup_open
                && modifiers.command
                && (matches!(key, Key::O | Key::F | Key::P | Key::R | Key::W | Key::Q)
                    || (key == Key::Z && modifiers.alt && !modifiers.shift)
                    || (key == Key::T && modifiers.shift))
            {
                egui::Popup::close_all(context);
            }
            if (typing || !(self.help || self.properties_open || popup_open))
                && matches!(
                    raw_key,
                    Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
                )
            {
                // egui scheduled directional focus before dispatch. Arrow
                // keys belong to text editing or reader navigation here.
                context.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            }
            if self.search_open
                && !popup_open
                && !self.help
                && !self.properties_open
                && self.password_input.is_none()
                && self.page_input.is_none()
                && modifiers.alt
                && !modifiers.command
                && !modifiers.shift
                && matches!(key, Key::C | Key::W)
            {
                if key == Key::C {
                    self.library.search_options.match_case =
                        !self.library.search_options.match_case;
                } else {
                    self.library.search_options.whole_word =
                        !self.library.search_options.whole_word;
                }
                self.dirty = true;
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.change_search();
                continue;
            }
            if !popup_open
                && self.zoom_input.is_none()
                && key == Key::M
                && modifiers.command
                && modifiers.shift
            {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.set_comments_open(context, !self.comments.open);
                continue;
            }
            if !popup_open
                && self.zoom_input.is_none()
                && modifiers.command
                && modifiers.shift
                && key == Key::H
                && self.nav.count > 0
            {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.add_comment(context);
                continue;
            }
            if self.comments.open
                && self.zoom_input.is_none()
                && self.page_input.is_none()
                && self.password_input.is_none()
                && modifiers.command
                && key == Key::Enter
                && self.comments.editor.is_some()
            {
                self.comments.save_requested = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.zoom_input.is_some() && key == Key::Escape {
                self.close_zoom(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if let Some(input) = &mut self.zoom_input
                && key == Key::Enter
                && modifiers.is_none()
                && (input.focus || context.memory(|memory| memory.has_focus(zoom::input_id())))
            {
                input.confirm = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.page_input.is_some() && key == Key::Escape {
                self.close_page_input(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.password_input.is_some() && key == Key::Escape {
                self.close_password_input(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if let Some(input) = &mut self.page_input
                && key == Key::Enter
                && modifiers.is_none()
                && (input.focus
                    || context.memory(|memory| memory.has_focus(input_dialogs::page_id())))
            {
                input.confirm = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if let Some(input) = &mut self.password_input
                && key == Key::Enter
                && modifiers.is_none()
                && (input.focus
                    || context.memory(|memory| memory.has_focus(input_dialogs::password_id())))
            {
                input.confirm = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            // Floating reader windows sit above the comment sidebar. Dismiss
            // the front window first and keep the underlying draft visible.
            if key == Key::Escape
                && self.comments.open
                && !self.help
                && !self.properties_open
                && self.recent_picker.is_none()
            {
                self.set_comments_open(context, false);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if key == Key::Escape {
                self.outline.cancel_input();
                self.hints.cancel();
                self.selection.cancel();
                self.mark_action = None;
                self.close_help(context);
                self.properties_open = false;
                self.recent_picker = None;
                self.page_input = None;
                self.search_open = false;
                self.search_focus = false;
                self.search_navigation.clear();
                self.password_input = None;
                self.nav.clear_prefix();
                context.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if modifiers.command && key == Key::O {
                self.choose_file();
                continue;
            }
            if modifiers.command && key == Key::F {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.find();
                continue;
            }
            if modifiers.command && modifiers.alt && !modifiers.shift && key == Key::Z {
                shortcuts::consume_reader_shortcut(context, raw_key, raw_modifiers);
                // Repeating the shortcut keeps any in-progress value intact.
                if self.zoom_input.is_none() {
                    self.open_zoom(context);
                }
                continue;
            }
            if modifiers.command && key == Key::P {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.toggle_recent(context);
                continue;
            }
            if modifiers.command && key == Key::R {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.reload(context);
                continue;
            }
            if modifiers.command && key == Key::W {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.close_document(context);
                continue;
            }
            if modifiers.command && modifiers.shift && key == Key::T {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.reopen_closed(context);
                continue;
            }
            if modifiers.command && key == Key::Q {
                context.input_mut(|input| {
                    input.consume_key(raw_modifiers, raw_key);
                });
                context.send_viewport_cmd(egui::ViewportCommand::Close);
                continue;
            }
            if popup_open {
                continue;
            }
            if let Some(picker) = &mut self.recent_picker {
                let filter_focused = picker.focus
                    || context
                        .memory(|memory| memory.has_focus(egui::Id::new("pagewing-recent-input")));
                // Tab can focus a result or footer action. Leave their activation
                // keys for egui instead of treating every Enter as filter confirm.
                if key == Key::Enter && filter_focused {
                    picker.confirm = true;
                    context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                } else if filter_focused
                    && let Some(motion) = shortcuts::filter_motion(key, modifiers)
                {
                    picker.motions.push(motion);
                    context.input_mut(|input| {
                        shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                    });
                }
                continue;
            }
            if self.comment_filter_focused(context) {
                if key == Key::Enter && modifiers.is_none() {
                    self.comments.filter_confirm = true;
                    context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                } else if let Some(motion) = shortcuts::filter_motion(key, modifiers) {
                    self.comments.filter_motions.push(motion);
                    context.input_mut(|input| {
                        shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                    });
                }
                continue;
            }
            if !self.search_focus
                && self.zoom_input.is_none()
                && self.page_input.is_none()
                && self.password_input.is_none()
                && !self.help
                && (self.outline.focus
                    || context
                        .memory(|memory| memory.has_focus(egui::Id::new("pagewing-outline-input"))))
            {
                if key == Key::Enter {
                    self.outline.confirm = true;
                    context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                } else if let Some(motion) = shortcuts::filter_motion(key, modifiers) {
                    self.outline.motions.push(motion);
                    context.input_mut(|input| {
                        shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                    });
                }
                continue;
            }
            if self.properties_open {
                if key == Key::Y && !modifiers.command && !modifiers.alt && !modifiers.ctrl {
                    self.copy_properties(context);
                }
                continue;
            }
            if typing || self.help || self.password_input.is_some() || self.page_input.is_some() {
                continue;
            }
            if modifiers.command && key == Key::A && self.nav.count > 0 {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.nav.clear_prefix();
                self.hints.cancel();
                self.mark_action = None;
                if self.selection_page != self.nav.page {
                    self.selection.start();
                }
                self.selection.select_all();
                self.selection_page = self.nav.page;
                if !self.selection.ready() {
                    self.worker.inspect_all(self.nav.page);
                }
                continue;
            }
            if modifiers.command && key == Key::C && self.selection.active {
                self.copy_selection(context);
                context.input_mut(|input| {
                    input.consume_key(raw_modifiers, raw_key);
                });
                continue;
            }
            shortcuts::consume_reader_shortcut(context, raw_key, raw_modifiers);
            if let Some(action) = shortcuts::zoom_action(key, modifiers) {
                self.nav.clear_prefix();
                self.mark_action = None;
                match action {
                    shortcuts::ZoomAction::In => self.zoom(1.2),
                    shortcuts::ZoomAction::Out => self.zoom(1.0 / 1.2),
                    shortcuts::ZoomAction::ActualSize => self.actual_size(),
                }
                continue;
            }
            if let Some(action) = self.mark_action.take() {
                if !modifiers.ctrl && !modifiers.command && !modifiers.alt {
                    let name = key.name().to_ascii_lowercase();
                    if matches!(action, MarkAction::Jump)
                        && !modifiers.shift
                        && matches!(key, Key::Quote | Key::Backtick)
                    {
                        if let Some(position) = self.history.previous_jump(self.nav.count) {
                            self.restore_position(position, true);
                        } else {
                            self.notify("No previous jump position");
                        }
                    } else if name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase() {
                        self.mark(name.as_bytes()[0] as char, action);
                    } else {
                        self.notify("Use a letter a–z for a mark");
                    }
                }
                continue;
            }
            if self.hints.active {
                if modifiers.ctrl || modifiers.command || modifiers.alt {
                    continue;
                }
                if key == Key::Backspace {
                    self.hints.prefix.pop();
                    continue;
                }
                let name = key.name().to_ascii_lowercase();
                if name.len() == 1
                    && let Some(index) = self.hints.press(name.as_bytes()[0] as char)
                    && let Some(target) = self
                        .links
                        .get(&self.nav.page)
                        .and_then(|links| {
                            if self.copy_link_hints {
                                self.visible_links
                                    .indices
                                    .iter()
                                    .filter_map(|&index| links.get(index))
                                    .filter(|link| matches!(&link.target, LinkTarget::Url(_)))
                                    .nth(index)
                            } else {
                                self.visible_links
                                    .indices
                                    .get(index)
                                    .and_then(|&index| links.get(index))
                            }
                        })
                        .map(|link| link.target.clone())
                {
                    if self.copy_link_hints {
                        if let LinkTarget::Url(url) = target {
                            context.copy_text(url);
                            self.notify("Copied link address");
                        }
                    } else {
                        self.follow_link(target, context);
                    }
                }
                continue;
            }
            if modifiers.ctrl {
                let n = self.nav.take_count() as f32;
                match key {
                    Key::E => self.scroll(Vec2::new(0.0, 60.0 * n), false),
                    Key::Y => self.scroll(Vec2::new(0.0, -60.0 * n), false),
                    Key::D => self.scroll(Vec2::new(0.0, self.viewport.y * 0.5 * n), true),
                    Key::U => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.5 * n), true),
                    Key::F => self.scroll(Vec2::new(0.0, self.viewport.y * 0.9 * n), true),
                    Key::B => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.9 * n), true),
                    Key::I => {
                        for _ in 0..n as usize {
                            if !self.history(true) {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if modifiers.alt {
                let count = self.nav.take_count();
                if matches!(key, Key::Equals | Key::Minus) && !modifiers.shift {
                    self.adjust_text_embolden(key == Key::Equals, count);
                } else if matches!(key, Key::ArrowLeft | Key::ArrowRight) {
                    for _ in 0..count {
                        if !self.history(key == Key::ArrowRight) {
                            break;
                        }
                    }
                } else if matches!(key, Key::J | Key::K) {
                    self.search_navigation.clear();
                    if let Some(page) = crate::navigation::bookmark_target(
                        &self.bookmark_pages,
                        self.nav.page,
                        key == Key::K,
                        count,
                    ) {
                        self.jump(page);
                    } else {
                        self.notify(format!(
                            "No bookmarks yet · {} to add one",
                            self.library.shortcuts.label("bookmark")
                        ));
                    }
                } else if !modifiers.shift && matches!(key, Key::ArrowDown | Key::ArrowUp) {
                    self.jump_chapter(key == Key::ArrowUp, count);
                }
                continue;
            }
            if modifiers.command {
                self.nav.clear_prefix();
                continue;
            }
            if key == Key::Backspace {
                self.nav.backspace();
                continue;
            }
            let digit = shortcuts::count_digit(key, modifiers.shift, key_text);
            if self.nav.pending_z {
                let count = self.nav.take_count() as f32;
                if symbol == Some('$') {
                    self.align_axis(0, 1.0);
                } else if digit == Some('0') {
                    self.align_axis(0, 0.0);
                } else if modifiers.shift && matches!(key, Key::H | Key::L) {
                    let direction = if key == Key::H { -1.0 } else { 1.0 };
                    self.scroll(
                        Vec2::new(self.viewport.x * 0.5 * count * direction, 0.0),
                        false,
                    );
                } else if !modifiers.shift {
                    match key {
                        Key::T => self.align_axis(1, 0.0),
                        Key::Z => self.align_axis(1, 0.5),
                        Key::B => self.align_axis(1, 1.0),
                        _ => {}
                    }
                }
                continue;
            }
            if symbol == Some('%') {
                let percentage = self.nav.prefix.parse::<usize>().ok();
                self.nav.clear_prefix();
                if let Some(percentage) = percentage {
                    self.jump(crate::navigation::percentage_page(
                        self.nav.count,
                        percentage,
                    ));
                } else {
                    self.notify("Type a percentage first, e.g. 50%");
                }
                continue;
            }
            if symbol == Some('$') {
                self.nav.clear_prefix();
                continue;
            }
            if let Some(digit) = digit {
                self.nav.digit(digit);
                continue;
            }
            if let Some(action) = shortcuts::reader_zoom_action(key, modifiers.shift, key_text) {
                let count = self.nav.take_count();
                self.zoom_steps(count, matches!(action, shortcuts::ZoomAction::Out));
                continue;
            }
            if key != Key::G {
                self.nav.pending_g = false;
            }
            match key {
                Key::J if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(n as isize);
                }
                Key::K if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(-(n as isize));
                }
                Key::J | Key::ArrowDown => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(0.0, 60.0 * n as f32), true);
                }
                Key::K | Key::ArrowUp => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(0.0, -60.0 * n as f32), true);
                }
                Key::H if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(-(n as isize));
                }
                Key::L if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(n as isize);
                }
                Key::H | Key::ArrowLeft => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(-60.0 * n as f32, 0.0), false);
                }
                Key::L | Key::ArrowRight => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(60.0 * n as f32, 0.0), false);
                }
                Key::PageDown | Key::CloseBracket => {
                    let n = self.nav.take_count();
                    self.step(n as isize);
                }
                Key::PageUp | Key::OpenBracket => {
                    let n = self.nav.take_count();
                    self.step(-(n as isize));
                }
                Key::Space => {
                    let n = self.nav.take_count();
                    self.scroll(
                        Vec2::new(
                            0.0,
                            self.viewport.y * if modifiers.shift { -0.9 } else { 0.9 } * n as f32,
                        ),
                        true,
                    );
                }
                Key::G if modifiers.shift => {
                    let page = if self.nav.prefix.is_empty() {
                        self.nav.count.saturating_sub(1)
                    } else {
                        self.nav.take_count() - 1
                    };
                    self.nav.clear_prefix();
                    self.jump(page);
                }
                Key::G if self.nav.pending_g => {
                    let page = self.nav.take_count() - 1;
                    self.jump(page);
                }
                Key::G => self.nav.pending_g = true,
                Key::Z if !modifiers.shift => self.nav.pending_z = true,
                Key::Home => self.jump(0),
                Key::End => self.jump(self.nav.count.saturating_sub(1)),
                Key::Enter if !self.nav.prefix.is_empty() => {
                    let page = self.nav.take_count() - 1;
                    self.jump(page);
                }
                Key::A => self.actual_size(),
                Key::W => {
                    self.reading.fit = Fit::Width;
                    self.dirty = true;
                }
                Key::E => {
                    self.reading.fit = Fit::Height;
                    self.dirty = true;
                }
                Key::P => {
                    self.reading.fit = Fit::Page;
                    self.pan = Vec2::ZERO;
                    self.dirty = true;
                }
                Key::R => {
                    let count = self.nav.take_count();
                    self.rotate_page(modifiers.shift, count);
                }
                Key::Slash => self.find(),
                Key::C if modifiers.shift => self.toggle_continuous(),
                Key::C => self.focus_contents(),
                Key::S => self.show_properties(context),
                Key::F => {
                    self.copy_link_hints = modifiers.shift;
                    let count = if self.copy_link_hints {
                        self.links.get(&self.nav.page).map_or(0, |links| {
                            self.visible_links
                                .indices
                                .iter()
                                .filter_map(|&index| links.get(index))
                                .filter(|link| matches!(&link.target, LinkTarget::Url(_)))
                                .count()
                        })
                    } else {
                        self.visible_links.indices.len()
                    };
                    self.hints.start(count);
                    if !self.hints.active {
                        self.notify(if self.copy_link_hints {
                            "No web links visible on this page"
                        } else {
                            "No links visible on this page"
                        });
                    } else if self.copy_link_hints {
                        self.notify("Copy link address · type a label · Escape to cancel");
                    }
                }
                Key::N | Key::F3 => {
                    let count = self.nav.take_count();
                    self.next_hits(modifiers.shift, count);
                }
                Key::M => {
                    self.nav.clear_prefix();
                    self.mark_action = Some(MarkAction::Set);
                }
                Key::Quote | Key::Backtick => {
                    self.nav.clear_prefix();
                    self.mark_action = Some(MarkAction::Jump);
                }
                Key::B => self.bookmark(),
                Key::Tab => {
                    self.hints.cancel();
                    self.library.sidebar = !self.library.sidebar;
                    self.dirty = true;
                }
                Key::T => {
                    self.hints.cancel();
                    self.library.thumbnails = !self.library.thumbnails;
                    self.library.sidebar = true;
                    self.reveal_page = Some(self.nav.page);
                    self.reveal_page_section = true;
                    self.dirty = true;
                }
                Key::D => {
                    self.library.dark = !self.library.dark;
                    set_theme(context, self.library.dark);
                    self.dirty = true;
                }
                Key::I => {
                    self.library.night = !self.library.night;
                    self.dirty = true;
                }
                Key::V if self.nav.count > 0 => {
                    self.hints.cancel();
                    if self.selection.active {
                        self.selection.cancel();
                    } else {
                        self.selection.start();
                        self.selection_page = self.nav.page;
                        self.worker.inspect(self.nav.page);
                        self.notify(
                            "Drag to select · double-click word / triple-click line · y to copy · Escape to exit",
                        );
                    }
                }
                Key::Y if self.selection.active => self.copy_selection(context),
                Key::Y => self.worker.copy(self.nav.page),
                Key::O => self.choose_file(),
                Key::Colon => self.open_goto(),
                Key::Questionmark | Key::F1 => self.open_help(context),
                Key::F11 => {
                    let fullscreen =
                        context.input(|input| input.viewport().fullscreen.unwrap_or(false));
                    context.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
                Key::F9 => self.toggle_reading_controls(context),
                _ => self.nav.clear_prefix(),
            }
            if !matches!(key, Key::G | Key::Z) {
                self.nav.clear_prefix();
            }
        }
        input_actions::recycle(&mut actions);
        self.reader_actions = actions;
    }
}
