//! User preferences and bounded folder installation, independent of the UI.
use super::{Plugin, Registry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Integer,
    Number,
    Boolean,
    Text,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Setting {
    pub fn valid(&self, value: &Value) -> bool {
        match self.kind {
            Kind::Boolean => value.is_boolean(),
            Kind::Text => value.as_str().is_some_and(|s| s.len() <= 4096),
            Kind::Integer | Kind::Number => value.as_f64().is_some_and(|n| {
                n.is_finite()
                    && (!matches!(self.kind, Kind::Integer) || value.is_i64() || value.is_u64())
                    && self.min.is_none_or(|min| n >= min)
                    && self.max.is_none_or(|max| n <= max)
            }),
        }
    }
}

pub fn validate_schema(settings: &[Setting]) -> Result<(), String> {
    if settings.len() > 32 {
        return Err("Too many plugin settings (maximum 32)".into());
    }
    let mut keys = BTreeSet::new();
    for setting in settings {
        if setting.key.is_empty()
            || setting.key.len() > 64
            || !keys.insert(&setting.key)
            || setting.label.trim().is_empty()
            || setting.label.len() > 128
            || setting.min.is_some_and(|n| !n.is_finite())
            || setting.max.is_some_and(|n| !n.is_finite())
            || matches!((setting.min, setting.max), (Some(min), Some(max)) if min > max)
            || !setting.valid(&setting.default)
        {
            return Err(format!("Invalid plugin setting: {}", setting.key));
        }
    }
    Ok(())
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub enabled: Option<bool>,
    pub executable: Option<String>,
    pub settings: BTreeMap<String, Value>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub plugins: BTreeMap<String, Options>,
    pub remember_visual: bool,
    pub last_visual: Option<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            plugins: BTreeMap::new(),
            remember_visual: yes(),
            last_visual: None,
        }
    }
}

impl Preferences {
    pub fn path(root: &Path) -> PathBuf {
        root.with_file_name("plugins.json")
    }

    pub fn load(root: &Path) -> Result<Self, String> {
        let file = match fs::File::open(Self::path(root)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Err("Plugin preferences exceed 1 MiB".into());
        }
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = Self::path(root);
        let parent = path.parent().ok_or("Missing preferences directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut file, self).map_err(|e| e.to_string())?;
        file.write_all(b"\n").map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn apply(&self, registry: &mut Registry) {
        for plugin in &mut registry.plugins {
            if let Some(options) = self.plugins.get(&plugin.id) {
                if let Some(enabled) = options.enabled {
                    plugin.enabled = enabled;
                }
                if let Some(executable) = &options.executable {
                    plugin.command[0] = executable.clone();
                }
                for setting in &plugin.settings_schema {
                    if let Some(value) = options.settings.get(&setting.key) {
                        if setting.valid(value) {
                            plugin.settings.insert(setting.key.clone(), value.clone());
                        } else {
                            registry.errors.push(format!(
                                "{}: invalid saved value for {}",
                                plugin.name, setting.label
                            ));
                        }
                    }
                }
            }
        }
    }
}

impl Plugin {
    pub fn executable(&self) -> Result<PathBuf, String> {
        let program = Path::new(&self.command[0]);
        let candidates = if program.is_absolute() {
            vec![program.to_path_buf()]
        } else if program.components().count() > 1 {
            vec![self.directory.join(program)]
        } else {
            let mut folders: Vec<_> =
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
            // Finder sessions often omit package-manager paths.
            folders.extend(
                ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from),
            );
            folders.into_iter().map(|path| path.join(program)).collect()
        };
        candidates
            .into_iter()
            .find(|path| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    path.metadata()
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                }
                #[cfg(not(unix))]
                {
                    path.is_file()
                }
            })
            .ok_or_else(|| {
                format!(
                    "Cannot find executable '{}'. Choose its location in Settings.",
                    self.command[0]
                )
            })
    }

    pub fn program(&self) -> &str {
        &self.command[0]
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

/// No symlinks or special files; bound both disk work and recursion depth.
fn copy_folder(
    source: &Path,
    destination: &Path,
    count: &mut usize,
    bytes: &mut u64,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("Plugin folders are nested too deeply".into());
    }
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        *count += 1;
        if *count > 2048 {
            return Err("Plugin contains more than 2048 entries".into());
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_folder(&entry.path(), &target, count, bytes, depth + 1)?;
        } else if metadata.is_file() {
            *bytes += metadata.len();
            if *bytes > 64 * 1024 * 1024 {
                return Err("Plugin exceeds 64 MiB".into());
            }
            fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        } else {
            return Err("Plugin folders cannot contain symbolic links or special files".into());
        }
    }
    Ok(())
}

pub fn install(root: &Path, source: &Path) -> Result<String, String> {
    if fs::symlink_metadata(source)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Choose the plugin folder itself, not a symbolic link".into());
    }
    let plugin = Plugin::load(source.to_path_buf())?;
    if root.join(&plugin.id).exists() {
        return Err(format!("{} is already installed", plugin.name));
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    if fs::canonicalize(root)
        .map_err(|e| e.to_string())?
        .starts_with(fs::canonicalize(source).map_err(|e| e.to_string())?)
    {
        return Err("The plugin source cannot contain its installation directory".into());
    }
    if fs::read_dir(root).map_err(|e| e.to_string())?.count() >= super::MAX_PLUGINS {
        return Err("Plugin directory is full".into());
    }
    let stage = tempfile::tempdir_in(root.parent().ok_or("Missing plugins parent")?)
        .map_err(|e| e.to_string())?;
    let destination = stage.path().join(&plugin.id);
    copy_folder(source, &destination, &mut 0, &mut 0, 0)?;
    // Validate the copied content before publishing the folder.
    Plugin::load(destination.clone())?;
    let mut preferences = Preferences::load(root)?;
    preferences
        .plugins
        .entry(plugin.id.clone())
        .or_default()
        .enabled = Some(false);
    if preferences.last_visual.as_deref() == Some(&plugin.id) {
        preferences.last_visual = None;
    }
    preferences.save(root)?;
    fs::rename(destination, root.join(&plugin.id)).map_err(|e| e.to_string())?;
    Ok(plugin.id)
}

pub fn install_example(root: &Path, focus: bool) -> Result<String, String> {
    let source = tempfile::tempdir().map_err(|e| e.to_string())?;
    let id = if focus {
        "focus-reader"
    } else {
        "copy-reference"
    };
    let folder = source.path().join(id);
    fs::create_dir(&folder).map_err(|e| e.to_string())?;
    let (manifest, script) = if focus {
        (
            include_str!("../../examples/plugins/focus-reader/plugin.toml"),
            include_str!("../../examples/plugins/focus-reader/plugin.py"),
        )
    } else {
        (
            include_str!("../../examples/plugins/copy-reference/plugin.toml"),
            include_str!("../../examples/plugins/copy-reference/plugin.py"),
        )
    };
    fs::write(folder.join("plugin.toml"), manifest).map_err(|e| e.to_string())?;
    fs::write(folder.join("plugin.py"), script).map_err(|e| e.to_string())?;
    install(root, &folder)
}

#[derive(Clone)]
pub struct Removed {
    pub id: String,
    pub path: PathBuf,
}

pub fn remove(root: &Path, id: &str) -> Result<Removed, String> {
    let path = managed_path(root, id)?;
    let trash = root.with_file_name("removed-plugins");
    fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix("removed-")
        .tempdir_in(&trash)
        .map_err(|e| e.to_string())?;
    let destination = stage.path().join(id);
    fs::rename(path, &destination).map_err(|e| e.to_string())?;
    let _ = stage.keep();
    Ok(Removed {
        id: id.into(),
        path: destination,
    })
}

pub fn restore(root: &Path, removed: &Removed) -> Result<(), String> {
    if root.join(&removed.id).exists() {
        return Err("A plugin with that folder name is already installed".into());
    }
    fs::rename(&removed.path, root.join(&removed.id)).map_err(|e| e.to_string())
}

fn managed_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    let mut components = Path::new(id).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err("Invalid plugin folder name".into());
    }
    let path = root.join(id);
    if fs::symlink_metadata(&path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Move this linked plugin folder manually".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(parent: &Path, id: &str) -> PathBuf {
        let folder = parent.join(id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("plugin.toml"),
            "api_version=1\nname='Example'\nenabled=true\ncommand=['/bin/sh','run.sh']",
        )
        .unwrap();
        fs::write(folder.join("run.sh"), "echo '{}'").unwrap();
        folder
    }

    #[test]
    fn install_disables_even_enabled_packages_and_undo_preserves_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("config/plugins");
        let folder = source(directory.path(), "example");
        let manifest = fs::read(folder.join("plugin.toml")).unwrap();
        let id = install(&root, &folder).unwrap();
        assert_eq!(
            fs::read(root.join(&id).join("plugin.toml")).unwrap(),
            manifest
        );
        let mut registry = Registry::discover(&root);
        Preferences::load(&root).unwrap().apply(&mut registry);
        assert!(!registry.plugins[0].enabled);
        assert!(
            install(&root, &folder)
                .unwrap_err()
                .contains("already installed")
        );
        let removed = remove(&root, &id).unwrap();
        assert!(!root.join(&id).exists());
        assert!(removed.path.join("run.sh").is_file());
        restore(&root, &removed).unwrap();
        assert_eq!(
            fs::read(root.join(&id).join("plugin.toml")).unwrap(),
            manifest
        );
        assert!(remove(&root, "../example").is_err());
        assert!(
            folder.join("run.sh").exists(),
            "original package must remain untouched"
        );
    }

    #[test]
    fn settings_roundtrip_preserves_manifest_and_rejects_invalid_values() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("config/plugins");
        install_example(&root, true).unwrap();
        let manifest = fs::read(root.join("focus-reader/plugin.toml")).unwrap();
        let mut prefs = Preferences::load(&root).unwrap();
        let options = prefs.plugins.get_mut("focus-reader").unwrap();
        options.enabled = Some(true);
        options.settings.insert("lines".into(), 5.into());
        options.settings.insert("opacity".into(), 0.5.into());
        prefs.last_visual = Some("focus-reader".into());
        prefs.save(&root).unwrap();
        let mut registry = Registry::discover(&root);
        Preferences::load(&root).unwrap().apply(&mut registry);
        let plugin = &registry.plugins[0];
        assert!(plugin.enabled);
        assert_eq!(plugin.settings["lines"], 5);
        assert_eq!(plugin.settings["opacity"], 0.5);
        assert_eq!(
            fs::read(root.join("focus-reader/plugin.toml")).unwrap(),
            manifest
        );
        assert!(plugin.settings_schema[0].valid(&5.into()));
        assert!(!plugin.settings_schema[0].valid(&2.5.into()));
        assert!(!plugin.settings_schema[0].valid(&100.into()));
        prefs
            .plugins
            .get_mut("focus-reader")
            .unwrap()
            .settings
            .insert("lines".into(), 100.into());
        let mut registry = Registry::discover(&root);
        prefs.apply(&mut registry);
        assert_eq!(registry.plugins[0].settings["lines"], 3);
        assert_eq!(registry.errors.len(), 1);
    }

    #[test]
    fn failed_install_does_not_publish_partial_folders() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("config/plugins");
        let folder = source(directory.path(), "oversized");
        fs::File::create(folder.join("huge.bin"))
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(install(&root, &folder).unwrap_err().contains("64 MiB"));
        assert!(!root.join("oversized").exists());
        assert!(
            install(&folder, &folder).is_err(),
            "recursive installation must be rejected"
        );
        fs::write(folder.join("plugin.toml"), "not a valid manifest").unwrap();
        assert!(install(&root, &folder).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_package_files_and_linked_removals_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("config/plugins");
        let folder = source(directory.path(), "linked");
        std::os::unix::fs::symlink(directory.path().join("outside"), folder.join("escape"))
            .unwrap();
        assert!(install(&root, &folder).unwrap_err().contains("symbolic"));
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&folder, root.join("linked")).unwrap();
        assert!(remove(&root, "linked").is_err());
        assert!(folder.exists());
    }

    #[test]
    fn program_override_repairs_missing_runtime_and_invalid_preferences_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("plugins");
        let folder = source(&root, "runtime");
        fs::write(
            folder.join("plugin.toml"),
            "api_version=1\nname='Runtime'\ncommand=['pagewing-missing-program-315984']",
        )
        .unwrap();
        let mut registry = Registry::discover(&root);
        assert!(registry.plugins[0].executable().is_err());
        let mut prefs = Preferences::default();
        prefs
            .plugins
            .entry("runtime".into())
            .or_default()
            .executable = Some("/bin/sh".into());
        prefs.apply(&mut registry);
        assert_eq!(
            registry.plugins[0].executable().unwrap(),
            Path::new("/bin/sh")
        );
        fs::write(Preferences::path(&root), "broken preferences").unwrap();
        assert!(Preferences::load(&root).is_err());
        let other = source(directory.path(), "new-package");
        assert!(install(&root, &other).is_err());
        assert!(!root.join("new-package").exists());
        assert_eq!(
            fs::read_to_string(Preferences::path(&root)).unwrap(),
            "broken preferences"
        );
    }
}
