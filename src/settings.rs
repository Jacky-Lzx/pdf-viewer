//! User preferences and portable, logical key bindings. Reader sequences remain
//! in the reader; this catalog describes configurable single-press commands.
use crate::{navigation::Fit, state::ReadingState};
use eframe::egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    #[default]
    #[serde(rename = "single", alias = "Single")]
    Single,
    #[serde(rename = "continuous", alias = "Continuous")]
    Continuous,
    #[serde(rename = "facing", alias = "Facing")]
    Facing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    pub layout: Layout,
    pub fit: Fit,
    pub remember_documents: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            layout: Layout::Single,
            fit: Fit::Page,
            remember_documents: true,
        }
    }
}

impl DisplaySettings {
    pub fn apply(&self, reading: &mut ReadingState) {
        reading.continuous = self.layout == Layout::Continuous;
        reading.facing = self.layout == Layout::Facing;
        reading.fit = match self.fit {
            Fit::Custom(scale) if !scale.is_finite() => Fit::Page,
            Fit::Custom(scale) => Fit::Custom(scale.clamp(0.05, 8.0)),
            fit => fit,
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub key: String,
    #[serde(default)]
    pub command: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
}

impl Binding {
    pub fn from_event(key: Key, modifiers: Modifiers, text: Option<char>) -> Self {
        // Punctuation can carry an implicit Shift, or arrive as the physical
        // base key followed by literal text on a different keyboard layout.
        let key = match text {
            Some('+') => Key::Plus,
            Some('?') => Key::Questionmark,
            Some(':') => Key::Colon,
            Some('−') => Key::Minus,
            _ => key,
        };
        Self {
            key: key.name().into(),
            command: modifiers.command,
            ctrl: modifiers.ctrl && !modifiers.command,
            alt: modifiers.alt,
            shift: modifiers.shift && !matches!(key, Key::Plus | Key::Questionmark | Key::Colon),
        }
    }

    fn parse(text: &str) -> Self {
        Self::parse_config(text).expect("valid built-in binding")
    }

    pub fn parse_config(text: &str) -> Result<Self, String> {
        let mut modifiers = Modifiers::NONE;
        let mut key = text;
        while let Some((prefix, rest)) = key.split_once('+') {
            match prefix {
                "Cmd" => modifiers.command = true,
                "Ctrl" => modifiers.ctrl = true,
                "Alt" => modifiers.alt = true,
                "Shift" => modifiers.shift = true,
                _ => return Err(format!("Unknown shortcut modifier in {text:?}")),
            }
            key = rest;
        }
        // Ctrl and Command are the same physical modifier outside macOS.
        if modifiers.ctrl && !cfg!(target_os = "macos") {
            modifiers.command = true;
        }
        let key = Key::from_name(key).ok_or_else(|| format!("Unknown shortcut key in {text:?}"))?;
        Ok(Self::from_event(key, modifiers, None))
    }

    pub fn config_label(&self) -> String {
        self.label().replace("Ctrl/Cmd", "Cmd")
    }

    pub fn event(&self) -> Option<(Key, Modifiers)> {
        Some((
            Key::from_name(&self.key)?,
            Modifiers {
                command: self.command,
                ctrl: self.ctrl || (self.command && !cfg!(target_os = "macos")),
                mac_cmd: self.command && cfg!(target_os = "macos"),
                alt: self.alt,
                shift: self.shift,
            },
        ))
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.command {
            parts.push("Ctrl/Cmd");
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    pub fn reserved(&self) -> bool {
        let Some(key) = Key::from_name(&self.key) else {
            return true;
        };
        if matches!(key, Key::Escape | Key::Backspace | Key::Enter) {
            return true;
        }
        if self.command && matches!(key, Key::A | Key::C | Key::X | Key::V) {
            return true;
        }
        if self.alt && !self.command && matches!(key, Key::C | Key::W) {
            return true;
        }
        if !self.command && !self.ctrl && !self.alt {
            if !self.shift && matches!(key, Key::G | Key::Z | Key::M | Key::Quote | Key::Backtick) {
                return true;
            }
            if matches!(
                key,
                Key::Num0
                    | Key::Num1
                    | Key::Num2
                    | Key::Num3
                    | Key::Num4
                    | Key::Num5
                    | Key::Num6
                    | Key::Num7
                    | Key::Num8
                    | Key::Num9
            ) {
                return true;
            }
        }
        false
    }
}

pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub global: bool,
    pub canonical: Binding,
    pub defaults: Vec<Binding>,
}

pub fn commands() -> &'static [Command] {
    static COMMANDS: OnceLock<Vec<Command>> = OnceLock::new();
    COMMANDS.get_or_init(|| {
        [
            ("open", "Open PDF", true, "Cmd+O", "Cmd+O|O"),
            ("close", "Close PDF", true, "Cmd+W", "Cmd+W"),
            (
                "reopen",
                "Reopen closed PDF",
                true,
                "Cmd+Shift+T",
                "Cmd+Shift+T",
            ),
            ("recent", "Find recent document", true, "Cmd+P", "Cmd+P"),
            ("reload", "Reload PDF", true, "Cmd+R", "Cmd+R"),
            ("quit", "Quit", true, "Cmd+Q", "Cmd+Q"),
            ("find", "Search document", true, "Cmd+F", "Cmd+F|Slash"),
            (
                "comments",
                "Toggle comments",
                true,
                "Cmd+Shift+M",
                "Cmd+Shift+M",
            ),
            (
                "add_comment",
                "Add highlight comment",
                true,
                "Cmd+Shift+H",
                "Cmd+Shift+H",
            ),
            ("down", "Scroll down", false, "J", "J|ArrowDown"),
            ("up", "Scroll up", false, "K", "K|ArrowUp"),
            ("left", "Scroll left", false, "H", "H|ArrowLeft"),
            ("right", "Scroll right", false, "L", "L|ArrowRight"),
            (
                "next_page",
                "Next page",
                false,
                "Shift+J",
                "Shift+J|Shift+L|PageDown|CloseBracket",
            ),
            (
                "previous_page",
                "Previous page",
                false,
                "Shift+K",
                "Shift+K|Shift+H|PageUp|OpenBracket",
            ),
            ("first_page", "First page", false, "Home", "Home"),
            (
                "last_page",
                "Last page / counted page",
                false,
                "Shift+G",
                "Shift+G|End",
            ),
            ("screen_down", "Screen down", false, "Space", "Space"),
            (
                "screen_up",
                "Screen up",
                false,
                "Shift+Space",
                "Shift+Space|Ctrl+B",
            ),
            ("half_down", "Half screen down", false, "Ctrl+D", "Ctrl+D"),
            ("half_up", "Half screen up", false, "Ctrl+U", "Ctrl+U"),
            (
                "scroll_down",
                "Scroll down without turning pages",
                false,
                "Ctrl+E",
                "Ctrl+E",
            ),
            (
                "scroll_up",
                "Scroll up without turning pages",
                false,
                "Ctrl+Y",
                "Ctrl+Y",
            ),
            (
                "history_back",
                "Jump history back",
                false,
                "Alt+ArrowLeft",
                "Alt+ArrowLeft",
            ),
            (
                "history_forward",
                "Jump history forward",
                false,
                "Alt+ArrowRight",
                "Alt+ArrowRight|Ctrl+I",
            ),
            ("next_bookmark", "Next bookmark", false, "Alt+J", "Alt+J"),
            (
                "previous_bookmark",
                "Previous bookmark",
                false,
                "Alt+K",
                "Alt+K",
            ),
            (
                "zoom_in",
                "Zoom in",
                true,
                "Equals",
                "Equals|Plus|Cmd+Equals|Cmd+Plus",
            ),
            ("zoom_out", "Zoom out", true, "Minus", "Minus|Cmd+Minus"),
            (
                "text_embolden_increase",
                "Increase PDF text weight",
                false,
                "Alt+Equals",
                "Alt+Equals",
            ),
            (
                "text_embolden_decrease",
                "Decrease PDF text weight",
                false,
                "Alt+Minus",
                "Alt+Minus",
            ),
            (
                "zoom",
                "Set an exact zoom percentage",
                true,
                "Cmd+Alt+Z",
                "Cmd+Alt+Z",
            ),
            (
                "actual_size",
                "Actual size (100%)",
                true,
                "Cmd+0",
                "A|Cmd+0",
            ),
            ("fit_page", "Fit page", false, "P", "P"),
            ("fit_width", "Fit width", false, "W", "W"),
            ("fit_height", "Fit height", false, "E", "E"),
            ("rotate", "Rotate clockwise", false, "R", "R"),
            (
                "rotate_back",
                "Rotate counterclockwise",
                false,
                "Shift+R",
                "Shift+R",
            ),
            (
                "continuous",
                "Toggle continuous scrolling",
                false,
                "Shift+C",
                "Shift+C",
            ),
            ("contents", "Find chapter", false, "C", "C"),
            ("properties", "Document information", false, "S", "S"),
            ("links", "Follow visible link", false, "F", "F"),
            (
                "copy_link",
                "Copy visible link address",
                false,
                "Shift+F",
                "Shift+F",
            ),
            ("next_match", "Next search match", false, "N", "N|F3"),
            (
                "previous_match",
                "Previous search match",
                false,
                "Shift+N",
                "Shift+N|Shift+F3",
            ),
            ("bookmark", "Toggle bookmark", false, "B", "B"),
            ("sidebar", "Toggle sidebar", false, "Tab", "Tab|Shift+Tab"),
            ("thumbnails", "Toggle page previews", false, "T", "T"),
            ("dark", "Toggle dark interface", false, "D", "D"),
            ("night", "Invert page colors", false, "I", "I"),
            ("selection", "Toggle text selection", false, "V", "V"),
            ("copy", "Copy selection / page text", false, "Y", "Y"),
            ("goto", "Go to page dialog", false, "Colon", "Colon"),
            ("help", "Keyboard shortcuts", false, "F1", "Questionmark|F1"),
            ("fullscreen", "Toggle fullscreen", false, "F11", "F11"),
        ]
        .into_iter()
        .map(|(id, label, global, canonical, defaults)| Command {
            id,
            label,
            global,
            canonical: Binding::parse(canonical),
            defaults: defaults
                .split('|')
                .map(Binding::parse)
                .chain(
                    (cfg!(target_os = "macos") && id == "screen_down")
                        .then(|| Binding::parse("Ctrl+F")),
                )
                .collect(),
        })
        .collect()
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    // Absent entries inherit defaults; an empty list explicitly disables them.
    pub overrides: BTreeMap<String, Vec<Binding>>,
}

pub enum Resolution {
    Mapped(Key, Modifiers),
    Disabled,
    Unmatched,
}

impl Shortcuts {
    pub fn bindings<'a>(&'a self, command: &'a Command) -> &'a [Binding] {
        self.overrides
            .get(command.id)
            .map_or(&command.defaults, Vec::as_slice)
    }

    pub fn label(&self, id: &str) -> String {
        commands()
            .iter()
            .find(|command| command.id == id)
            .map(|command| {
                let bindings = self.bindings(command);
                if bindings.is_empty() {
                    "Unassigned".into()
                } else {
                    bindings
                        .iter()
                        .map(Binding::label)
                        .collect::<Vec<_>>()
                        .join(" / ")
                }
            })
            .unwrap_or_default()
    }

    pub fn resolve(&self, binding: &Binding, global_only: bool) -> Resolution {
        let eligible = |command: &Command| !global_only || (command.global && binding.command);
        for command in commands().iter().filter(|command| eligible(command)) {
            if self.bindings(command).contains(binding)
                && let Some((key, mut modifiers)) = command.canonical.event()
            {
                // Command zoom cancels unfinished reader prompts; plain zoom
                // retains numeric counts. Preserve that distinction after remapping.
                if matches!(command.id, "zoom_in" | "zoom_out") && binding.command {
                    modifiers.command = true;
                    modifiers.ctrl = !cfg!(target_os = "macos");
                    modifiers.mac_cmd = cfg!(target_os = "macos");
                }
                return Resolution::Mapped(key, modifiers);
            }
        }
        if commands()
            .iter()
            .any(|command| eligible(command) && command.defaults.contains(binding))
        {
            Resolution::Disabled
        } else {
            Resolution::Unmatched
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for id in self.overrides.keys() {
            if !commands().iter().any(|command| command.id == id) {
                return Err(format!("Unknown shortcut action {id:?}"));
            }
        }
        let mut used: Vec<(&Binding, &str)> = Vec::new();
        for command in commands() {
            for binding in self.bindings(command) {
                if binding.reserved() {
                    return Err(format!(
                        "{} is reserved for text editing or reader sequences",
                        binding.label()
                    ));
                }
                if let Some((_, label)) = used.iter().find(|(other, _)| *other == binding) {
                    return Err(format!(
                        "{} is assigned to both {label} and {}",
                        binding.label(),
                        command.label
                    ));
                }
                used.push((binding, command.label));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_every_canonical_key_is_known() {
        Shortcuts::default().validate().unwrap();
        for command in commands() {
            assert!(command.canonical.event().is_some(), "{}", command.id);
            assert!(
                command
                    .defaults
                    .iter()
                    .all(|binding| binding.event().is_some())
            );
        }
    }

    #[test]
    fn overrides_replace_defaults_and_survive_library_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.json");
        let mut library = crate::state::Library::default();
        library.display.layout = Layout::Facing;
        library.display.fit = Fit::Custom(1.5);
        library.display.remember_documents = false;
        library
            .shortcuts
            .overrides
            .insert("next_page".into(), vec![Binding::parse("X")]);
        library.shortcuts.overrides.insert("night".into(), vec![]);
        library.shortcuts.validate().unwrap();
        library.save(&path).unwrap();
        let restored = crate::state::Library::load(&path);
        assert_eq!(restored.display, library.display);
        assert_eq!(restored.shortcuts, library.shortcuts);
        assert!(
            matches!(restored.shortcuts.resolve(&Binding::parse("X"), false), Resolution::Mapped(Key::J, modifiers) if modifiers.shift)
        );
        assert!(matches!(
            restored
                .shortcuts
                .resolve(&Binding::parse("Shift+J"), false),
            Resolution::Disabled
        ));
        assert!(matches!(
            restored.shortcuts.resolve(&Binding::parse("I"), false),
            Resolution::Disabled
        ));
        assert!(matches!(
            restored.shortcuts.resolve(&Binding::parse("X"), true),
            Resolution::Unmatched
        ));
        let legacy: crate::state::Library =
            serde_json::from_str(r#"{"dark":true,"recent":[]}"#).unwrap();
        assert!(legacy.dark);
        assert_eq!(legacy.display, DisplaySettings::default());
        assert_eq!(legacy.shortcuts, Shortcuts::default());
    }

    #[test]
    fn conflicts_and_reserved_sequences_are_rejected() {
        let mut shortcuts = Shortcuts::default();
        shortcuts
            .overrides
            .insert("next_page".into(), vec![Binding::parse("P")]);
        assert!(shortcuts.validate().unwrap_err().contains("Fit page"));
        for key in ["G", "Z", "Escape", "Enter", "Backspace", "5", "Cmd+C"] {
            shortcuts
                .overrides
                .insert("next_page".into(), vec![Binding::parse(key)]);
            assert!(shortcuts.validate().is_err(), "{key}");
        }
        let punctuation = Binding::from_event(Key::Slash, Modifiers::SHIFT, Some('?'));
        assert_eq!(punctuation, Binding::parse("Questionmark"));
    }
}
