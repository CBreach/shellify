use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::Deserialize;

use crate::ui::icons::IconPack;
use crate::ui::theme::{ColorMode, ThemeConfig};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Key binding overrides: key spec -> command, e.g. `"ctrl-n" = "next"`.
    /// An empty command unbinds the key.
    pub keys: HashMap<String, String>,
    pub theme: ThemeConfig,
    pub ui: UiConfig,
}

/// `[ui]`: display options that aren't colors.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub color: ColorMode,
    pub icons: IconPack,
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
