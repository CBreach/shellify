//! User theme files.
//!
//! Custom themes live next to the config, in `~/.config/shellify/themes/`, one
//! TOML file per theme:
//!
//! ```toml
//! name = "Tokyo Night"     # optional; defaults to the file name
//! base = "nord"            # optional built-in preset to start from
//! accent = "#7aa2f7"       # any color left out comes from `base`
//! text = "#c0caf5"
//! muted = "#565f89"
//! error = "#f7768e"
//! selection_fg = "#1a1b26"
//! ```
//!
//! `:theme import <file>` validates a file and copies it in. It also accepts
//! base16 schemes (`.yaml`), the format most popular terminal themes are
//! published in, converting them to the format above.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::ui::theme::{PRESETS, parse_color};

/// A theme file as written on disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_fg: Option<String>,
}

/// A validated custom theme, selectable by `id` like a built-in preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTheme {
    /// What `[theme] preset` and `:theme <id>` refer to: the file name.
    pub id: String,
    /// Display name (from the file, else the id).
    pub name: String,
    pub file: ThemeFile,
    pub path: PathBuf,
}

/// `themes/` next to the config file.
pub fn themes_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("themes")
}

/// Loads every `*.toml` in `dir`, sorted by id. Files that fail to parse
/// are skipped and reported, so one bad file never breaks startup.
pub fn load_dir(dir: &Path) -> (Vec<UserTheme>, Vec<String>) {
    let mut themes = Vec::new();
    let mut problems = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (themes, problems),
        Err(e) => return (themes, vec![format!("{}: {e}", dir.display())]),
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        match load_file(&path) {
            Ok(theme) => themes.push(theme),
            Err(e) => problems.push(format!("{}: {e:#}", file_label(&path))),
        }
    }
    themes.sort_by(|a, b| a.id.cmp(&b.id));
    (themes, problems)
}

fn load_file(path: &Path) -> Result<UserTheme> {
    let text = std::fs::read_to_string(path)?;
    let file: ThemeFile = toml::from_str(&text)?;
    validate(&file)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("theme");
    let id = slug(stem);
    if id.is_empty() {
        bail!("file name must contain letters or digits");
    }
    Ok(UserTheme {
        name: file.name.clone().unwrap_or_else(|| id.clone()),
        id,
        file,
        path: path.to_path_buf(),
    })
}

/// Checks the base preset and every color, naming the offending key.
fn validate(file: &ThemeFile) -> Result<()> {
    if let Some(base) = &file.base
        && !PRESETS.contains(&base.as_str())
    {
        bail!(
            "base: unknown preset {base:?} (built-ins: {})",
            PRESETS.join(", ")
        );
    }
    let colors = [
        ("accent", &file.accent),
        ("text", &file.text),
        ("muted", &file.muted),
        ("error", &file.error),
        ("selection_fg", &file.selection_fg),
    ];
    for (key, value) in colors {
        if let Some(value) = value {
            parse_color(value).map_err(|e| anyhow!("{key}: {e}"))?;
        }
    }
    Ok(())
}

/// Validates `source` (a Shellify `.toml` theme or a base16 `.yaml` scheme)
/// and writes it into `dir` as `<id>.toml`. Returns the new theme's id.
/// Never overwrites an existing theme.
pub fn import(source: &Path, dir: &Path) -> Result<String> {
    let text =
        std::fs::read_to_string(source).with_context(|| format!("reading {}", source.display()))?;
    let is_yaml = source
        .extension()
        .is_some_and(|ext| ext == "yaml" || ext == "yml");
    let (file, origin) = if is_yaml {
        (from_base16(&text)?, "a base16 scheme")
    } else {
        let file: ThemeFile = toml::from_str(&text)
            .with_context(|| format!("{} isn't a Shellify theme file", file_label(source)))?;
        (file, "a Shellify theme")
    };
    validate(&file)?;

    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let id = slug(file.name.as_deref().unwrap_or(stem));
    if id.is_empty() {
        bail!("can't make a theme name from {}", file_label(source));
    }
    let dest = dir.join(format!("{id}.toml"));
    if dest.exists() {
        bail!(
            "a theme called {id:?} already exists ({}); delete or rename it first",
            dest.display()
        );
    }
    let body = toml::to_string(&file).context("serializing theme")?;
    let contents = format!(
        "# Shellify theme, imported from {} ({origin}).\n# Edit freely; run `:theme reload` to apply changes.\n\n{body}",
        file_label(source)
    );
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&dest, contents).with_context(|| format!("writing {}", dest.display()))?;
    Ok(id)
}

/// Converts a base16 scheme. Both the classic flat layout (`base0D: "7aa2f7"`)
/// and the newer tinted-theming one (`palette:` block, `#`-prefixed values)
/// work, since this only looks for `baseXX` keys and the name.
fn from_base16(text: &str) -> Result<ThemeFile> {
    let mut name = None;
    let mut colors: [Option<String>; 16] = Default::default();
    for line in text.lines() {
        let line = line.split(" #").next().unwrap_or(line).trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(['"', '\'']);
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if (key == "scheme" || key == "name") && !value.is_empty() {
            name = Some(value.to_string());
            continue;
        }
        let Some(index) = key
            .strip_prefix("base0")
            .filter(|digit| digit.len() == 1)
            .and_then(|digit| u8::from_str_radix(digit, 16).ok())
        else {
            continue;
        };
        let hex = value.trim_start_matches('#');
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("{key}: expected a 6-digit hex color, got {value:?}");
        }
        colors[usize::from(index)] = Some(format!("#{}", hex.to_lowercase()));
    }
    // base16's roles: 00 background, 03 comments, 05 foreground, 08 red, 0D blue.
    let pick = |i: usize| {
        colors[i]
            .clone()
            .ok_or_else(|| anyhow!("not a base16 scheme: base0{i:X} is missing"))
    };
    Ok(ThemeFile {
        name,
        base: None,
        accent: Some(pick(0x0D)?),
        text: Some(pick(0x05)?),
        muted: Some(pick(0x03)?),
        error: Some(pick(0x08)?),
        selection_fg: Some(pick(0x00)?),
    })
}

/// `"Tokyo Night"` -> `tokyo-night`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKYO: &str = r##"
name = "Tokyo Night"
base = "nord"
accent = "#7aa2f7"
error = "#f7768e"
"##;

    // Classic base16 layout (values without '#').
    const BASE16: &str = r##"
scheme: "Ocean Breeze"
author: "Someone"
base00: "1a1b26" # background
base01: "16161e"
base03: "565f89"
base05: "c0caf5"
base08: "f7768e"
base0D: "7aa2f7"
"##;

    // Newer tinted-theming layout.
    const TINTED: &str = r##"
system: "base16"
name: "Paper Night"
palette:
  base00: "#101010"
  base03: "#555555"
  base05: "#d0d0d0"
  base08: "#ff5555"
  base0D: "#5599ff"
"##;

    #[test]
    fn loads_theme_files_and_skips_broken_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Tokyo Night.toml"), TOKYO).unwrap();
        std::fs::write(dir.path().join("broken.toml"), "accent = \"blurple\"").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a theme").unwrap();

        let (themes, problems) = load_dir(dir.path());
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "tokyo-night");
        assert_eq!(themes[0].name, "Tokyo Night");
        assert_eq!(themes[0].file.base.as_deref(), Some("nord"));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("broken.toml") && problems[0].contains("accent"));
    }

    #[test]
    fn missing_dir_is_not_an_error() {
        let (themes, problems) = load_dir(Path::new("/nonexistent/shellify/themes"));
        assert!(themes.is_empty() && problems.is_empty());
    }

    #[test]
    fn rejects_unknown_base_and_unknown_keys() {
        let bad_base = ThemeFile {
            base: Some("vaporwave".into()),
            ..Default::default()
        };
        assert!(
            validate(&bad_base)
                .unwrap_err()
                .to_string()
                .contains("base")
        );
        assert!(toml::from_str::<ThemeFile>("accnet = \"#fff\"").is_err());
    }

    #[test]
    fn converts_base16_schemes_in_both_layouts() {
        let t = from_base16(BASE16).unwrap();
        assert_eq!(t.name.as_deref(), Some("Ocean Breeze"));
        assert_eq!(t.accent.as_deref(), Some("#7aa2f7"));
        assert_eq!(t.text.as_deref(), Some("#c0caf5"));
        assert_eq!(t.muted.as_deref(), Some("#565f89"));
        assert_eq!(t.error.as_deref(), Some("#f7768e"));
        assert_eq!(t.selection_fg.as_deref(), Some("#1a1b26"));

        let t = from_base16(TINTED).unwrap();
        assert_eq!(t.name.as_deref(), Some("Paper Night"));
        assert_eq!(t.accent.as_deref(), Some("#5599ff"));

        assert!(
            from_base16("base00: \"101010\"").is_err(),
            "incomplete scheme"
        );
        assert!(from_base16("base0D: \"zzzzzz\"").is_err(), "bad hex");
    }

    #[test]
    fn import_writes_a_normalized_theme_and_never_overwrites() {
        let src = tempfile::tempdir().unwrap();
        let themes = tempfile::tempdir().unwrap();
        let yaml = src.path().join("ocean.yaml");
        std::fs::write(&yaml, BASE16).unwrap();

        let id = import(&yaml, themes.path()).unwrap();
        assert_eq!(id, "ocean-breeze");
        let (loaded, problems) = load_dir(themes.path());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(loaded[0].id, "ocean-breeze");
        assert_eq!(loaded[0].file.accent.as_deref(), Some("#7aa2f7"));
        let written = std::fs::read_to_string(themes.path().join("ocean-breeze.toml")).unwrap();
        assert!(written.starts_with("# Shellify theme, imported from ocean.yaml"));

        let err = import(&yaml, themes.path()).unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
    }

    #[test]
    fn import_validates_before_writing() {
        let src = tempfile::tempdir().unwrap();
        let themes = tempfile::tempdir().unwrap();
        let bad = src.path().join("bad.toml");
        std::fs::write(&bad, "accent = \"blurple\"").unwrap();
        assert!(import(&bad, themes.path()).is_err());
        assert!(std::fs::read_dir(themes.path()).unwrap().next().is_none());

        let toml_theme = src.path().join("tokyo.toml");
        std::fs::write(&toml_theme, TOKYO).unwrap();
        assert_eq!(import(&toml_theme, themes.path()).unwrap(), "tokyo-night");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Tokyo Night"), "tokyo-night");
        assert_eq!(slug("  Rosé Pine (Moon) "), "ros-pine-moon");
        assert_eq!(slug("!!!"), "");
    }
}
