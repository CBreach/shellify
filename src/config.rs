use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::Deserialize;
use toml_edit::{DocumentMut, Value};

use crate::app::settings::Appearance;
use crate::app::visualizer::VizStyle;
use crate::ui::icons::IconPack;
use crate::ui::layout::PaneSizes;
use crate::ui::theme::{ColorMode, ThemeConfig};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Key binding overrides: key spec -> command, e.g. `"ctrl-n" = "next"`.
    /// An empty command unbinds the key.
    pub keys: HashMap<String, String>,
    pub theme: ThemeConfig,
    pub ui: UiConfig,
    pub providers: ProvidersConfig,
}

/// `[providers]`
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProvidersConfig {
    /// The provider in use (e.g. `youtube-music`); none means the demo library.
    pub active: Option<String>,
    #[serde(rename = "youtube-music")]
    pub youtube_music: GoogleClientConfig,
}

/// `[providers.youtube-music]`: the user's own Google OAuth client. The
/// client secret is kept in the keychain, not here.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GoogleClientConfig {
    pub client_id: Option<String>,
}

/// `[ui]`: display options that aren't colors.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub color: ColorMode,
    pub icons: IconPack,
    pub mouse: bool,
    pub resize_cursor: bool,
    pub visualizer: bool,
    pub visualizer_style: VizStyle,
    pub visualizer_fade: bool,
    /// Side pane widths in percent of the window (see `PaneSizes`).
    pub library_width: Option<u16>,
    pub queue_width: Option<u16>,
}

impl Config {
    /// Loads the config file, falling back to defaults when it doesn't exist.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }
}

/// `$XDG_CONFIG_HOME/shellify/config.toml`, else `~/.config/shellify/config.toml`
/// (also on macOS, where terminal users expect it rather than ~/Library).
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| BaseDirs::new().map(|d| d.home_dir().join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("shellify").join("config.toml")
}

/// Writes the Settings tab's choices into `[theme]` and `[ui]`, leaving the
/// rest of the file (key bindings, comments, formatting) untouched. Values
/// equal to the default are removed rather than written out.
pub fn save_appearance(path: &Path, appearance: &Appearance) -> Result<()> {
    let mut doc = read_doc(path)?;

    let t = &appearance.theme;
    let theme_values = [
        ("preset", t.preset.as_deref()),
        ("accent", t.accent.as_deref()),
        ("text", t.text.as_deref()),
        ("muted", t.muted.as_deref()),
        ("error", t.error.as_deref()),
        ("selection_fg", t.selection_fg.as_deref()),
    ];
    let panes = appearance.panes;
    let default_panes = PaneSizes::default();
    let ui_values = [
        (
            "color",
            (appearance.color != ColorMode::default()).then(|| appearance.color.label().into()),
        ),
        (
            "icons",
            (appearance.icons != IconPack::default()).then(|| appearance.icons.label().into()),
        ),
        ("mouse", appearance.mouse.then(|| true.into())),
        (
            "resize_cursor",
            appearance.resize_cursor.then(|| true.into()),
        ),
        ("visualizer", appearance.visualizer.then(|| true.into())),
        (
            "visualizer_style",
            (appearance.visualizer_style != VizStyle::default())
                .then(|| appearance.visualizer_style.label().into()),
        ),
        (
            "visualizer_fade",
            appearance.visualizer_fade.then(|| true.into()),
        ),
        (
            "library_width",
            (panes.library != default_panes.library).then(|| i64::from(panes.library).into()),
        ),
        (
            "queue_width",
            (panes.queue != default_panes.queue).then(|| i64::from(panes.queue).into()),
        ),
    ];
    let theme_values = theme_values.map(|(k, v)| (k, v.map(Value::from)));
    set_table(&mut doc, "theme", &theme_values);
    set_table(&mut doc, "ui", &ui_values);

    write_doc(path, &doc)
}

/// Saves the provider in use to `[providers] active` (`None` removes it),
/// leaving the rest of the file untouched.
pub fn save_provider(path: &Path, id: Option<&str>) -> Result<()> {
    let mut doc = read_doc(path)?;
    set_table(&mut doc, "providers", &[("active", id.map(Value::from))]);
    write_doc(path, &doc)
}

/// Saves the Google OAuth client ID to `[providers.youtube-music]`
/// (`None` removes it).
pub fn save_google_client_id(path: &Path, id: Option<&str>) -> Result<()> {
    let mut doc = read_doc(path)?;
    let providers = doc
        .entry("providers")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .context("[providers] isn't a table")?;
    // No bare `[providers]` header when it only holds the subtable.
    providers.set_implicit(true);
    let google = providers
        .entry("youtube-music")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .context("[providers.youtube-music] isn't a table")?;
    match id {
        Some(id) => google["client_id"] = toml_edit::value(id),
        None => {
            google.remove("client_id");
        }
    }
    if google.is_empty() {
        providers.remove("youtube-music");
    }
    if providers.is_empty() {
        doc.remove("providers");
    }
    write_doc(path, &doc)
}

fn read_doc(path: &Path) -> Result<DocumentMut> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    text.parse()
        .with_context(|| format!("parsing {}", path.display()))
}

/// Writes to a temp file and renames it, so a crash never leaves a
/// half-written config.
fn write_doc(path: &Path, doc: &DocumentMut) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Sets or removes each key in `[name]`, dropping the table if it ends up empty.
fn set_table(doc: &mut DocumentMut, name: &str, values: &[(&str, Option<Value>)]) {
    let item = doc.entry(name).or_insert(toml_edit::table());
    let Some(table) = item.as_table_mut() else {
        return;
    };
    for (key, value) in values {
        match value {
            Some(v) => table[key] = toml_edit::value(v.clone()),
            None => {
                table.remove(key);
            }
        }
    }
    if table.is_empty() {
        doc.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_preserves_keys_and_comments_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original =
            "# my bindings\n[keys]\n\"ctrl-n\" = \"next\" # skip\n\n[theme]\naccent = \"red\"\n";
        std::fs::write(&path, original).unwrap();

        let mut appearance = Appearance {
            theme: ThemeConfig {
                preset: Some("nord".into()),
                muted: Some("#445566".into()),
                ..Default::default()
            },
            icons: IconPack::Ascii,
            mouse: true,
            resize_cursor: true,
            visualizer: true,
            visualizer_style: VizStyle::Wave,
            visualizer_fade: true,
            panes: PaneSizes {
                library: 30,
                queue: 28,
            },
            ..Default::default()
        };
        save_appearance(&path, &appearance).unwrap();

        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# my bindings"));
        assert!(saved.contains("\"ctrl-n\" = \"next\" # skip"));
        assert!(
            !saved.contains("accent"),
            "cleared override is removed:\n{saved}"
        );
        assert!(
            !saved.contains("color ="),
            "default color mode is not written"
        );

        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.theme, appearance.theme);
        assert_eq!(loaded.ui.icons, IconPack::Ascii);
        assert!(loaded.ui.mouse);
        assert!(saved.contains("mouse = true"), "{saved}");
        assert!(saved.contains("resize_cursor = true"), "{saved}");
        assert!(loaded.ui.resize_cursor);
        assert!(loaded.ui.visualizer);
        assert_eq!(loaded.ui.visualizer_style, VizStyle::Wave);
        assert!(saved.contains("visualizer_style = \"wave\""), "{saved}");
        assert!(saved.contains("visualizer_fade = true"), "{saved}");
        assert!(loaded.ui.visualizer_fade);
        assert!(saved.contains("library_width = 30"), "{saved}");
        assert!(
            !saved.contains("queue_width"),
            "default width isn't written"
        );
        assert_eq!(loaded.ui.library_width, Some(30));
        assert_eq!(loaded.keys["ctrl-n"], "next");

        // Back to defaults: the [theme] and [ui] tables disappear entirely.
        appearance = Appearance::default();
        save_appearance(&path, &appearance).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            !saved.contains("[theme]") && !saved.contains("[ui]"),
            "{saved}"
        );
    }

    #[test]
    fn saves_and_clears_the_provider_keeping_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# mine\n[ui]\nmouse = true\n").unwrap();

        save_provider(&path, Some("youtube-music")).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.providers.active.as_deref(), Some("youtube-music"));
        assert!(loaded.ui.mouse);
        assert!(std::fs::read_to_string(&path).unwrap().contains("# mine"));

        save_provider(&path, None).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("[providers]"), "{saved}");
        assert!(Config::load(&path).unwrap().providers.active.is_none());
    }

    #[test]
    fn saves_the_google_client_id_next_to_the_active_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save_google_client_id(&path, Some("fake-id.apps.googleusercontent.com")).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("[providers.youtube-music]"), "{saved}");
        assert!(!saved.contains("[providers]\n"), "no empty header: {saved}");

        save_provider(&path, Some("youtube-music")).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.providers.active.as_deref(), Some("youtube-music"));
        assert_eq!(
            loaded.providers.youtube_music.client_id.as_deref(),
            Some("fake-id.apps.googleusercontent.com")
        );

        save_provider(&path, None).unwrap();
        save_google_client_id(&path, None).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("providers"), "{saved}");
    }

    #[test]
    fn save_creates_missing_file_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/shellify/config.toml");
        let appearance = Appearance {
            color: ColorMode::Never,
            ..Default::default()
        };
        save_appearance(&path, &appearance).unwrap();
        assert_eq!(Config::load(&path).unwrap().ui.color, ColorMode::Never);
    }
}
