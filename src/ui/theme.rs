use std::str::FromStr;

use anyhow::{Result, anyhow, bail};
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

use super::icons::{IconPack, Icons};

/// The `[theme]` table in config.toml: an optional preset plus per-color
/// overrides. Colors are names (`cyan`, `light-blue`), `#rrggbb` or an
/// ANSI index (`208`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub preset: Option<String>,
    pub accent: Option<String>,
    pub text: Option<String>,
    pub muted: Option<String>,
    pub error: Option<String>,
    pub selection_fg: Option<String>,
}

/// `[ui] color`: whether to use color at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Color unless `NO_COLOR` is set (non-empty) or `TERM=dumb`.
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub const ALL: [ColorMode; 3] = [ColorMode::Auto, ColorMode::Always, ColorMode::Never];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    /// Resolves the mode against the environment (see <https://no-color.org>).
    pub fn enabled(self) -> bool {
        let no_color = std::env::var("NO_COLOR").ok();
        let term = std::env::var("TERM").ok();
        self.enabled_with(no_color.as_deref(), term.as_deref())
    }

    fn enabled_with(self, no_color: Option<&str>, term: Option<&str>) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => no_color.is_none_or(str::is_empty) && term != Some("dumb"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Focused borders, selection background, playing track, progress bar.
    pub accent: Color,
    /// Normal text. `Reset` keeps the terminal's own foreground.
    pub text: Color,
    /// Unfocused borders, headers, secondary info.
    pub muted: Color,
    pub error: Color,
    /// Text drawn on top of `accent` (selected rows, mode badge).
    pub selection_fg: Color,
    /// No colors at all: every signal must survive via symbols, weight
    /// (bold, thick borders) and reverse video.
    pub mono: bool,
    pub icons: &'static Icons,
}

pub const PRESETS: &[&str] = &["default", "nord", "gruvbox", "catppuccin"];

impl Theme {
    pub fn preset(name: &str) -> Option<Self> {
        let rgb = |hex: u32| Color::from_u32(hex);
        let theme = match name {
            "default" => Self {
                accent: Color::Cyan,
                text: Color::Reset,
                muted: Color::DarkGray,
                error: Color::Red,
                selection_fg: Color::Black,
                mono: false,
                icons: IconPack::Unicode.icons(),
            },
            "nord" => Self {
                accent: rgb(0x88c0d0),
                text: rgb(0xeceff4),
                muted: rgb(0x4c566a),
                error: rgb(0xbf616a),
                selection_fg: rgb(0x2e3440),
                mono: false,
                icons: IconPack::Unicode.icons(),
            },
            "gruvbox" => Self {
                accent: rgb(0xfabd2f),
                text: rgb(0xebdbb2),
                muted: rgb(0x665c54),
                error: rgb(0xfb4934),
                selection_fg: rgb(0x282828),
                mono: false,
                icons: IconPack::Unicode.icons(),
            },
            "catppuccin" => Self {
                accent: rgb(0xcba6f7),
                text: rgb(0xcdd6f4),
                muted: rgb(0x585b70),
                error: rgb(0xf38ba8),
                selection_fg: rgb(0x1e1e2e),
                mono: false,
                icons: IconPack::Unicode.icons(),
            },
            _ => return None,
        };
        Some(theme)
    }

    /// The theme with every color reset to the terminal default.
    pub fn monochrome(self) -> Self {
        Self {
            accent: Color::Reset,
            text: Color::Reset,
            muted: Color::Reset,
            error: Color::Reset,
            selection_fg: Color::Reset,
            mono: true,
            icons: self.icons,
        }
    }

    /// Selected row / mode badge: accent background, or reverse video in mono.
    pub fn selection(&self) -> Style {
        let style = Style::new().add_modifier(Modifier::BOLD);
        if self.mono {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style.fg(self.selection_fg).bg(self.accent)
        }
    }

    pub fn from_config(config: &ThemeConfig, color: ColorMode, icons: IconPack) -> Result<Self> {
        let theme = Self::from_theme_config(config)?.with_color_mode(color);
        Ok(Self {
            icons: icons.icons(),
            ..theme
        })
    }

    pub fn with_color_mode(self, color: ColorMode) -> Self {
        if color.enabled() {
            self
        } else {
            self.monochrome()
        }
    }

    fn from_theme_config(config: &ThemeConfig) -> Result<Self> {
        let name = config.preset.as_deref().unwrap_or("default");
        let mut theme = Self::preset(name).ok_or_else(|| {
            anyhow!(
                "theme: unknown preset {name:?} (available: {})",
                PRESETS.join(", ")
            )
        })?;
        let overrides = [
            ("accent", &config.accent, &mut theme.accent),
            ("text", &config.text, &mut theme.text),
            ("muted", &config.muted, &mut theme.muted),
            ("error", &config.error, &mut theme.error),
            (
                "selection_fg",
                &config.selection_fg,
                &mut theme.selection_fg,
            ),
        ];
        for (key, value, slot) in overrides {
            if let Some(value) = value {
                *slot = parse_color(value).map_err(|e| anyhow!("theme.{key}: {e}"))?;
            }
        }
        Ok(theme)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::preset("default").expect("default preset exists")
    }
}

pub fn parse_color(value: &str) -> Result<Color> {
    // Accept "light_blue" / "Light Blue" as well as ratatui's "light-blue".
    let normalized = value.trim().to_lowercase().replace(['_', ' '], "-");
    match Color::from_str(&normalized) {
        Ok(color) => Ok(color),
        Err(_) => bail!("invalid color {value:?} (use a name like \"cyan\", \"#rrggbb\" or 0-255)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(preset: Option<&str>, accent: Option<&str>) -> ThemeConfig {
        ThemeConfig {
            preset: preset.map(Into::into),
            accent: accent.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn empty_config_is_default_theme() {
        assert_eq!(
            Theme::from_theme_config(&ThemeConfig::default()).unwrap(),
            Theme::default()
        );
    }

    #[test]
    fn every_listed_preset_exists() {
        for name in PRESETS {
            assert!(Theme::preset(name).is_some(), "{name}");
        }
    }

    #[test]
    fn overrides_apply_on_top_of_preset() {
        let theme = Theme::from_theme_config(&config(Some("nord"), Some("#ff8800"))).unwrap();
        assert_eq!(theme.accent, Color::Rgb(0xff, 0x88, 0x00));
        assert_eq!(theme.error, Theme::preset("nord").unwrap().error);
    }

    #[test]
    fn color_mode_honors_no_color_and_dumb_terminals() {
        assert!(ColorMode::Auto.enabled_with(None, Some("xterm-256color")));
        assert!(ColorMode::Auto.enabled_with(Some(""), None));
        assert!(!ColorMode::Auto.enabled_with(Some("1"), None));
        assert!(!ColorMode::Auto.enabled_with(None, Some("dumb")));
        assert!(ColorMode::Always.enabled_with(Some("1"), Some("dumb")));
        assert!(!ColorMode::Never.enabled_with(None, None));
    }

    #[test]
    fn monochrome_selection_uses_reverse_video() {
        let mono = Theme::default().monochrome();
        assert!(mono.selection().add_modifier.contains(Modifier::REVERSED));
        assert_eq!(mono.selection().bg, None);
        assert_eq!(Theme::default().selection().bg, Some(Color::Cyan));
    }

    #[test]
    fn color_formats() {
        assert_eq!(parse_color("Light_Blue").unwrap(), Color::LightBlue);
        assert_eq!(parse_color("208").unwrap(), Color::Indexed(208));
        assert!(parse_color("not-a-color").is_err());
    }

    #[test]
    fn bad_values_name_the_key() {
        let err = Theme::from_theme_config(&config(None, Some("blurple"))).unwrap_err();
        assert!(err.to_string().contains("theme.accent"));
        assert!(Theme::from_theme_config(&config(Some("vaporwave"), None)).is_err());
    }
}
