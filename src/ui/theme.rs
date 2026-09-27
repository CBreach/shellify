use std::str::FromStr;

use anyhow::{Result, anyhow, bail};
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

use super::icons::{IconPack, Icons};
use crate::themes::UserTheme;

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

pub const PRESETS: &[&str] = &[
    "default",
    "nord",
    "gruvbox",
    "catppuccin",
    "youtube-music",
    "spotify",
    "apple-music",
];

/// Every selectable theme id: the built-in presets, then custom themes. A
/// custom theme with a built-in's name replaces it rather than appearing twice.
pub fn theme_ids(user: &[UserTheme]) -> Vec<String> {
    let mut ids: Vec<String> = PRESETS.iter().map(|s| s.to_string()).collect();
    for theme in user {
        if !ids.contains(&theme.id) {
            ids.push(theme.id.clone());
        }
    }
    ids
}

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
            // Provider themes (picked automatically when you choose one on
            // the Providers tab): each service's own colors on a dark UI.
            "youtube-music" => Self {
                accent: rgb(0xff0033),
                text: rgb(0xf1f1f1),
                muted: rgb(0x717171),
                error: rgb(0xffb74d),
                selection_fg: rgb(0xffffff),
                mono: false,
                icons: IconPack::Unicode.icons(),
            },
            "spotify" => Self {
                accent: rgb(0x1ed760),
                text: rgb(0xffffff),
                muted: rgb(0x535353),
                error: rgb(0xf15e6c),
                selection_fg: rgb(0x000000),
                mono: false,
                icons: IconPack::Unicode.icons(),
            },
            "apple-music" => Self {
                accent: rgb(0xff5c8d),
                text: rgb(0xf5f5f7),
                muted: rgb(0x6e6e73),
                error: rgb(0xff9f0a),
                selection_fg: rgb(0xffffff),
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

    /// A theme by id: a custom theme (its `base` preset with its colors on
    /// top) or a built-in preset.
    pub fn named(name: &str, user: &[UserTheme]) -> Option<Self> {
        let Some(custom) = user.iter().find(|t| t.id == name) else {
            return Self::preset(name);
        };
        let f = &custom.file;
        let mut theme = Self::preset(f.base.as_deref().unwrap_or("default"))?;
        let colors = [
            (&f.accent, &mut theme.accent),
            (&f.text, &mut theme.text),
            (&f.muted, &mut theme.muted),
            (&f.error, &mut theme.error),
            (&f.selection_fg, &mut theme.selection_fg),
        ];
        for (value, slot) in colors {
            // Theme files are validated when loaded, so this can't fail.
            if let Some(color) = value.as_deref().and_then(|v: &str| parse_color(v).ok()) {
                *slot = color;
            }
        }
        Some(theme)
    }

    pub fn from_config(
        config: &ThemeConfig,
        color: ColorMode,
        icons: IconPack,
        user: &[UserTheme],
    ) -> Result<Self> {
        let theme = Self::from_theme_config(config, user)?.with_color_mode(color);
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

    fn from_theme_config(config: &ThemeConfig, user: &[UserTheme]) -> Result<Self> {
        let name = config.preset.as_deref().unwrap_or("default");
        let mut theme = Self::named(name, user).ok_or_else(|| {
            anyhow!(
                "theme: unknown preset {name:?} (available: {})",
                theme_ids(user).join(", ")
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
            Theme::from_theme_config(&ThemeConfig::default(), &[]).unwrap(),
            Theme::default()
        );
    }

    #[test]
    fn every_listed_preset_exists() {
        for name in PRESETS {
            assert!(Theme::preset(name).is_some(), "{name}");
        }
        for provider in crate::provider::ProviderKind::ALL {
            assert!(PRESETS.contains(&provider.theme()), "{provider:?}");
        }
    }

    #[test]
    fn overrides_apply_on_top_of_preset() {
        let theme = Theme::from_theme_config(&config(Some("nord"), Some("#ff8800")), &[]).unwrap();
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

    fn custom(id: &str, base: Option<&str>, accent: Option<&str>) -> UserTheme {
        UserTheme {
            id: id.into(),
            name: id.into(),
            file: crate::themes::ThemeFile {
                base: base.map(Into::into),
                accent: accent.map(Into::into),
                ..Default::default()
            },
            path: "/tmp/x.toml".into(),
        }
    }

    #[test]
    fn custom_themes_inherit_their_base_and_can_shadow_builtins() {
        let user = [
            custom("ocean", Some("gruvbox"), Some("#123456")),
            custom("nord", None, Some("#abcdef")),
        ];
        let ocean = Theme::named("ocean", &user).unwrap();
        assert_eq!(ocean.accent, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(ocean.error, Theme::preset("gruvbox").unwrap().error);

        let nord = Theme::named("nord", &user).unwrap();
        assert_eq!(nord.accent, Color::Rgb(0xab, 0xcd, 0xef), "custom wins");
        assert_eq!(nord.error, Theme::default().error, "no base means default");

        assert_eq!(
            theme_ids(&user),
            [
                "default",
                "nord",
                "gruvbox",
                "catppuccin",
                "youtube-music",
                "spotify",
                "apple-music",
                "ocean"
            ],
            "a shadowing theme isn't listed twice"
        );

        // Config overrides still apply on top of a custom theme.
        let config = ThemeConfig {
            preset: Some("ocean".into()),
            error: Some("red".into()),
            ..Default::default()
        };
        let theme = Theme::from_theme_config(&config, &user).unwrap();
        assert_eq!(theme.accent, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(theme.error, Color::Red);
    }

    #[test]
    fn color_formats() {
        assert_eq!(parse_color("Light_Blue").unwrap(), Color::LightBlue);
        assert_eq!(parse_color("208").unwrap(), Color::Indexed(208));
        assert!(parse_color("not-a-color").is_err());
    }

    #[test]
    fn bad_values_name_the_key() {
        let err = Theme::from_theme_config(&config(None, Some("blurple")), &[]).unwrap_err();
        assert!(err.to_string().contains("theme.accent"));
        assert!(Theme::from_theme_config(&config(Some("vaporwave"), None), &[]).is_err());
    }
}
