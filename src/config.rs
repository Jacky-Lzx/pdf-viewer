//! Human-edited preferences, kept separate from automatic reading-state saves.
use crate::{
    navigation::{Fit, MAX_SCALE, MIN_SCALE},
    page_colors::{InvertedPalette, PageColors, PagePalette},
    settings::{Binding, DisplaySettings, Layout, Shortcuts},
    state::Library,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

const SCHEMA: &str = include_str!("../assets/config.schema.json");
const SCHEMA_DIRECTIVE: &str = "#:schema ./config.schema.json\n";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    appearance: Appearance,
    display: Display,
    shortcuts: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Appearance {
    dark: bool,
    night: bool,
    sidebar: bool,
    thumbnails: bool,
    text_embolden: f32,
    white: PagePalette,
    inverted: InvertedPalette,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Display {
    layout: Layout,
    fit: ConfigFit,
    remember_documents: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            layout: Layout::Single,
            fit: ConfigFit::Mode(FitMode::Page),
            remember_documents: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum ConfigFit {
    Mode(FitMode),
    Scale(f32),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FitMode {
    Page,
    Width,
    Height,
}

impl Config {
    fn from_library(library: &Library) -> Self {
        Self {
            appearance: Appearance {
                dark: library.dark,
                night: library.night,
                sidebar: library.sidebar,
                thumbnails: library.thumbnails,
                text_embolden: 0.0,
                white: PagePalette::default(),
                inverted: InvertedPalette::default(),
            },
            display: Display {
                layout: library.display.layout,
                fit: match library.display.fit {
                    Fit::Page => ConfigFit::Mode(FitMode::Page),
                    Fit::Width => ConfigFit::Mode(FitMode::Width),
                    Fit::Height => ConfigFit::Mode(FitMode::Height),
                    Fit::Custom(scale) => ConfigFit::Scale(scale),
                },
                remember_documents: library.display.remember_documents,
            },
            shortcuts: library
                .shortcuts
                .overrides
                .iter()
                .map(|(id, bindings)| {
                    (
                        id.clone(),
                        bindings.iter().map(Binding::config_label).collect(),
                    )
                })
                .collect(),
        }
    }

    pub fn page_colors(&self) -> PageColors {
        PageColors {
            white: self.appearance.white,
            inverted: self.appearance.inverted,
        }
    }

    pub fn render_settings(&self) -> hayro::RenderSettings {
        hayro::RenderSettings {
            text_embolden: self.appearance.text_embolden,
            ..Default::default()
        }
    }

    fn fit(&self) -> Result<Fit, String> {
        Ok(match self.display.fit {
            ConfigFit::Mode(FitMode::Page) => Fit::Page,
            ConfigFit::Mode(FitMode::Width) => Fit::Width,
            ConfigFit::Mode(FitMode::Height) => Fit::Height,
            ConfigFit::Scale(scale)
                if scale.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&scale) =>
            {
                Fit::Custom(scale)
            }
            ConfigFit::Scale(_) => {
                return Err(
                    "display.fit must be page, width, height, or a zoom from 0.05 to 8.0".into(),
                );
            }
        })
    }

    fn bindings(&self) -> Result<Shortcuts, String> {
        let overrides = self
            .shortcuts
            .iter()
            .map(|(id, keys)| {
                let bindings = keys
                    .iter()
                    .map(|key| Binding::parse_config(key))
                    .collect::<Result<_, _>>()?;
                Ok((id.clone(), bindings))
            })
            .collect::<Result<_, String>>()?;
        let shortcuts = Shortcuts { overrides };
        shortcuts.validate()?;
        Ok(shortcuts)
    }

    fn validate(&self) -> Result<(), String> {
        if !self.appearance.text_embolden.is_finite()
            || !(0.0..=1.0).contains(&self.appearance.text_embolden)
        {
            return Err("appearance.text_embolden must be a number from 0.0 to 1.0".into());
        }
        self.fit()?;
        self.bindings()?;
        Ok(())
    }

    pub fn apply(&self, library: &mut Library) {
        library.dark = self.appearance.dark;
        library.night = self.appearance.night;
        library.sidebar = self.appearance.sidebar;
        library.thumbnails = self.appearance.thumbnails;
        library.display = DisplaySettings {
            layout: self.display.layout,
            fit: self.fit().expect("validated configuration"),
            remember_documents: self.display.remember_documents,
        };
        library.shortcuts = self.bindings().expect("validated configuration");
    }
}

pub fn path() -> PathBuf {
    path_in(
        std::env::var_os("XDG_CONFIG_HOME")
            .as_deref()
            .map(Path::new),
        directories::BaseDirs::new()
            .as_ref()
            .map(|dirs| dirs.home_dir()),
    )
}

fn path_in(xdg: Option<&Path>, home: Option<&Path>) -> PathBuf {
    xdg.filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.unwrap_or_else(|| Path::new(".")).join(".config"))
        .join("pagewing/config.toml")
}

fn parse(text: &str) -> Result<Config, String> {
    let config: Config = toml::from_str(text).map_err(|error| error.to_string())?;
    config.validate()?;
    Ok(config)
}

/// This companion file belongs to the application; config.toml belongs to the user.
/// Refresh it atomically after upgrades, without touching it on ordinary launches.
fn write_schema(path: &Path) -> std::io::Result<()> {
    let schema_path = path.with_file_name("config.schema.json");
    if std::fs::read(&schema_path).is_ok_and(|bytes| bytes == SCHEMA.as_bytes()) {
        return Ok(());
    }
    let parent = schema_path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(SCHEMA.as_bytes())?;
    file.persist(schema_path).map_err(|error| error.error)?;
    Ok(())
}

/// Create only when absent; normal operation never rewrites comments or edits.
pub fn load_or_create(path: &Path, library: &Library) -> Result<Config, String> {
    let load = || -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => return parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let config = Config::from_library(library);
        config.validate()?;
        let text = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
        let parent = path
            .parent()
            .ok_or("Configuration path has no parent directory")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        file.write_all(SCHEMA_DIRECTIVE.as_bytes())
            .and_then(|()| file.write_all(b"# Pagewing configuration. Edit this file and restart the reader.\n# fit: \"page\", \"width\", \"height\", or a zoom factor (0.05-8.0).\n# Shortcut example: next_page = [\"X\"] under [shortcuts]; [] disables an action.\n\n"))
            .and_then(|()| file.write_all(text.as_bytes())).map_err(|error| error.to_string())?;
        match file.persist_noclobber(path) {
            Ok(_) => Ok(config),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                parse(&std::fs::read_to_string(path).map_err(|error| error.to_string())?)
            }
            Err(error) => Err(error.to_string()),
        }
    };
    let result = load();
    // Schema support must not prevent reading a valid, read-only configuration.
    // Install it even for invalid TOML so the editor can help repair the file.
    if let Err(error) = write_schema(path) {
        eprintln!(
            "Could not update configuration schema {}: {error}",
            path.with_file_name("config.schema.json").display()
        );
    }
    result.map_err(|error| {
        format!(
            "Could not load configuration {}: {error}. Using built-in defaults.",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_uses_xdg_or_dot_config_on_every_platform() {
        assert_eq!(
            path_in(None, Some(Path::new("/Users/reader"))),
            Path::new("/Users/reader/.config/pagewing/config.toml")
        );
        assert_eq!(
            path_in(Some(Path::new("/tmp/preferences")), None),
            Path::new("/tmp/preferences/pagewing/config.toml")
        );
        for xdg in ["", "relative"] {
            assert_eq!(
                path_in(Some(Path::new(xdg)), Some(Path::new("/home/reader"))),
                Path::new("/home/reader/.config/pagewing/config.toml")
            );
        }
    }

    #[test]
    fn first_launch_imports_legacy_settings_and_later_saves_leave_config_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences/config.toml");
        let mut library = Library {
            dark: true,
            ..Default::default()
        };
        library.display.layout = Layout::Facing;
        library.display.fit = Fit::Custom(1.5);
        library.shortcuts.overrides.insert(
            "next_page".into(),
            vec![Binding::parse_config("Cmd+Comma").unwrap()],
        );
        load_or_create(&path, &library).unwrap();
        let mut text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(SCHEMA_DIRECTIVE));
        assert_eq!(
            std::fs::read_to_string(path.with_file_name("config.schema.json")).unwrap(),
            SCHEMA
        );
        text.push_str("\n# Keep my comments\n");
        std::fs::write(&path, &text).unwrap();
        let config = load_or_create(&path, &Library::default()).unwrap();
        config.apply(&mut library);
        assert!(library.dark);
        assert_eq!(library.display.layout, Layout::Facing);
        assert_eq!(library.display.fit, Fit::Custom(1.5));
        assert_eq!(
            library.shortcuts.overrides["next_page"][0].config_label(),
            "Cmd+Comma"
        );
        library.dark = false;
        library
            .save(&directory.path().join("library.json"))
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        load_or_create(&path, &library).unwrap().apply(&mut library);
        assert!(
            library.dark,
            "the config must take precedence over saved session state"
        );
    }

    #[test]
    fn partial_config_uses_defaults_and_example_is_valid() {
        let mut library = Library::default();
        parse("[appearance]\ndark = true\n[shortcuts]\nnext_page = ['X']\nnight = []\n")
            .unwrap()
            .apply(&mut library);
        assert!(library.dark);
        assert!(library.display.remember_documents);
        assert_eq!(library.shortcuts.label("next_page"), "X");
        assert_eq!(library.shortcuts.label("night"), "Unassigned");
        parse(include_str!("../assets/config.example.toml")).unwrap();
    }

    #[test]
    fn text_embolden_defaults_validates_and_roundtrips() {
        assert_eq!(parse("").unwrap().render_settings().text_embolden, 0.0);
        for value in [0.0, 0.2, 1.0] {
            let config = parse(&format!("[appearance]\ntext_embolden = {value}")).unwrap();
            assert_eq!(config.render_settings().text_embolden, value);
            let restored = parse(&toml::to_string_pretty(&config).unwrap()).unwrap();
            assert_eq!(restored.render_settings().text_embolden, value);
        }
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let field = &schema["properties"]["appearance"]["properties"]["text_embolden"];
        assert_eq!(field["minimum"], 0);
        assert_eq!(field["maximum"], 1);
        assert_eq!(field["default"], 0.0);
        for value in ["-0.1", "1.1", "nan", "inf", "-inf", "true", "'0.2'"] {
            assert!(parse(&format!("[appearance]\ntext_embolden = {value}")).is_err());
        }
    }

    #[test]
    fn page_palettes_use_mode_defaults_and_roundtrip_custom_colors() {
        use crate::page_colors::Rgb;
        let defaults = parse("").unwrap().page_colors();
        assert_eq!(defaults.palette(false), PagePalette::default());
        assert_eq!(defaults.palette(true), PagePalette::inverted());
        let text = "[appearance.white]\nbackground = '#FFF4DC'\n\
                    [appearance.inverted]\nbackground = '#1e1e2e'\ncanvas = '#181825'\n";
        let config = parse(text).unwrap();
        let colors = config.page_colors();
        assert_eq!(colors.palette(false).foreground, Rgb([0; 3]));
        assert_eq!(colors.palette(false).background, Rgb([255, 244, 220]));
        assert_eq!(colors.palette(true).foreground, Rgb([255; 3]));
        assert_eq!(colors.palette(true).background, Rgb([30, 30, 46]));
        assert_eq!(colors.palette(true).canvas, Some(Rgb([24, 24, 37])));
        let restored = parse(&toml::to_string_pretty(&config).unwrap())
            .unwrap()
            .page_colors();
        for night in [false, true] {
            assert_eq!(restored.palette(night), colors.palette(night));
        }
        for value in [
            "red",
            "#fff",
            "#12345678",
            "#gg1234",
            "#１２３",
            "123456",
            "",
        ] {
            for mode in ["white", "inverted"] {
                for field in ["foreground", "background", "canvas"] {
                    let text = format!("[appearance.{mode}]\n{field} = '{value}'");
                    assert!(parse(&text).unwrap_err().contains("expected #RRGGBB"));
                }
            }
        }
        assert!(parse("[appearance.inverted]\nbackgound = '#1e1e2e'").is_err());
    }

    #[test]
    fn invalid_config_reports_errors_and_is_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        for text in [
            "not toml",
            "[appearance]\ndrak = true",
            "[display]\nlayout = 'bogus'",
            "[display]\nfit = 0.0",
            "[display]\nfit = 8.1",
            "[display]\nfit = nan",
            "[shortcuts]\nnext_pgae = ['X']",
            "[shortcuts]\nnext_page = ['Escape']",
            "[shortcuts]\nnext_page = ['P']",
            "[shortcuts]\nnext_page = ['Cmd+Bogus']",
            "[shortcuts]\nnext_page = ['Bogus+X']",
        ] {
            std::fs::write(&path, text).unwrap();
            let error = load_or_create(&path, &Library::default()).unwrap_err();
            assert!(error.contains(&path.display().to_string()), "{error}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
    }

    #[test]
    fn schema_refresh_preserves_existing_config_and_unchanged_schema() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let schema = path.with_file_name("config.schema.json");
        let text = "# My existing preferences\n[appearance]\ndark = true\n";
        std::fs::write(&path, text).unwrap();
        std::fs::write(&schema, "outdated schema").unwrap();
        load_or_create(&path, &Library::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(std::fs::read_to_string(&schema).unwrap(), SCHEMA);
        let modified = std::fs::metadata(&schema).unwrap().modified().unwrap();
        load_or_create(&path, &Library::default()).unwrap();
        assert_eq!(
            std::fs::metadata(&schema).unwrap().modified().unwrap(),
            modified
        );

        // A blocked schema destination must not discard valid preferences.
        std::fs::remove_file(&schema).unwrap();
        std::fs::create_dir(&schema).unwrap();
        let config = load_or_create(&path, &Library::default()).unwrap();
        assert!(config.appearance.dark);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn schema_covers_config_defaults_and_the_command_catalog() {
        use serde_json::{Value, json};
        let schema: Value = serde_json::from_str(SCHEMA).unwrap();
        let defaults = serde_json::to_value(Config::default()).unwrap();
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(properties.len(), defaults.as_object().unwrap().len());
        for (section, values) in defaults.as_object().unwrap() {
            let section_schema = &properties[section];
            assert_eq!(section_schema["additionalProperties"], false);
            assert!(section_schema.get("required").is_none());
            if section == "shortcuts" {
                continue;
            }
            let fields = section_schema["properties"].as_object().unwrap();
            assert_eq!(fields.len(), values.as_object().unwrap().len());
            for (name, default) in values.as_object().unwrap() {
                assert_eq!(&fields[name]["default"], default, "{section}.{name}");
                assert!(fields[name]["description"].as_str().is_some());
            }
        }
        let actions = schema["properties"]["shortcuts"]["properties"]
            .as_object()
            .unwrap();
        assert_eq!(actions.len(), crate::settings::commands().len());
        for command in crate::settings::commands() {
            assert!(
                actions[command.id]["description"]
                    .as_str()
                    .unwrap()
                    .starts_with(command.label)
            );
            // Keep these inline: Taplo 0.10 overflows its stack on root
            // completions when shared shortcut schemas use allOf + $ref.
            assert_eq!(actions[command.id]["type"], "array");
            assert_eq!(actions[command.id]["items"]["type"], "string");
        }
        let display = &properties["display"]["properties"];
        for value in display["layout"]["enum"].as_array().unwrap() {
            serde_json::from_value::<Layout>(value.clone()).unwrap();
        }
        for layout in [Layout::Single, Layout::Continuous, Layout::Facing] {
            assert!(
                display["layout"]["enum"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(layout))
            );
        }
        for value in display["fit"]["anyOf"][0]["enum"].as_array().unwrap() {
            serde_json::from_value::<FitMode>(value.clone()).unwrap();
        }
        assert_eq!(
            display["fit"]["anyOf"][1]["minimum"].as_f64().unwrap() as f32,
            MIN_SCALE
        );
        assert_eq!(
            display["fit"]["anyOf"][1]["maximum"].as_f64().unwrap() as f32,
            MAX_SCALE
        );
    }
}
