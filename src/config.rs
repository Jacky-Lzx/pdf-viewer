//! Human-edited preferences, kept separate from automatic reading-state saves.
use crate::{
    navigation::{Fit, MAX_SCALE, MIN_SCALE},
    settings::{Binding, DisplaySettings, Layout, Shortcuts},
    state::Library,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

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
        file.write_all(b"# Pagewing configuration. Edit this file and restart the reader.\n# fit: \"page\", \"width\", \"height\", or a zoom factor (0.05-8.0).\n# Shortcut example: next_page = [\"X\"] under [shortcuts]; [] disables an action.\n\n")
            .and_then(|()| file.write_all(text.as_bytes())).map_err(|error| error.to_string())?;
        match file.persist_noclobber(path) {
            Ok(_) => Ok(config),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                parse(&std::fs::read_to_string(path).map_err(|error| error.to_string())?)
            }
            Err(error) => Err(error.to_string()),
        }
    };
    load().map_err(|error| {
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
}
