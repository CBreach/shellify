//! The Settings tab's model: which rows exist and how each value cycles.
//! Rendering lives in `ui/settings.rs`; saving in `config::save_appearance`.

use crate::ui::icons::IconPack;
use crate::ui::theme::{ColorMode, PRESETS, ThemeConfig};

/// Everything the Settings tab edits. Mirrors `[theme]` and `[ui]` in config.toml.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Appearance {
    pub theme: ThemeConfig,
    pub color: ColorMode,
    pub icons: IconPack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    Preset,
    Accent,
    Text,
    Muted,
    Error,
    SelectionFg,
    Icons,
    Color,
    Reset,
}

/// Colors offered when cycling with h/l. Anything else can be typed via Enter.
pub const NAMED_COLORS: &[&str] = &[
    "cyan",
    "light-cyan",
    "blue",
    "light-blue",
    "magenta",
    "light-magenta",
    "green",
    "light-green",
    "yellow",
    "light-yellow",
    "red",
    "light-red",
    "white",
    "gray",
    "dark-gray",
    "black",
];

impl SettingRow {
    pub const ALL: [SettingRow; 9] = [
        SettingRow::Preset,
        SettingRow::Accent,
        SettingRow::Text,
        SettingRow::Muted,
        SettingRow::Error,
        SettingRow::SelectionFg,
        SettingRow::Icons,
        SettingRow::Color,
        SettingRow::Reset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Preset => "Theme",
            Self::Accent => "Accent",
            Self::Text => "Text",
            Self::Muted => "Muted",
            Self::Error => "Error",
            Self::SelectionFg => "Selection text",
            Self::Icons => "Icons",
            Self::Color => "Color",
            Self::Reset => "Reset appearance to defaults",
        }
    }

    /// The `[theme]` key for color rows, used as the edit prompt label.
    pub fn color_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Accent => "accent",
            Self::Text => "text",
            Self::Muted => "muted",
            Self::Error => "error",
            Self::SelectionFg => "selection_fg",
            _ => return None,
        })
    }
}

impl Appearance {
    pub fn color_slot(&mut self, row: SettingRow) -> Option<&mut Option<String>> {
        let t = &mut self.theme;
        Some(match row {
            SettingRow::Accent => &mut t.accent,
            SettingRow::Text => &mut t.text,
            SettingRow::Muted => &mut t.muted,
            SettingRow::Error => &mut t.error,
            SettingRow::SelectionFg => &mut t.selection_fg,
            _ => return None,
        })
    }

    /// What the row currently shows, e.g. `catppuccin`, `#ff8800`, `preset`.
    pub fn value_label(&self, row: SettingRow) -> String {
        let t = &self.theme;
        let color = |c: &Option<String>| c.clone().unwrap_or_else(|| "preset".into());
        match row {
            SettingRow::Preset => t.preset.clone().unwrap_or_else(|| "default".into()),
            SettingRow::Accent => color(&t.accent),
            SettingRow::Text => color(&t.text),
            SettingRow::Muted => color(&t.muted),
            SettingRow::Error => color(&t.error),
            SettingRow::SelectionFg => color(&t.selection_fg),
            SettingRow::Icons => self.icons.label().into(),
            SettingRow::Color => self.color.label().into(),
            SettingRow::Reset => String::new(),
        }
    }

    /// Moves a row's value forward (`delta > 0`) or back through its options.
    pub fn step(&mut self, row: SettingRow, delta: i32) {
        match row {
            SettingRow::Preset => {
                let current = self.theme.preset.as_deref().unwrap_or("default");
                let next = cycle(PRESETS, &current, delta);
                // "default" is the implicit preset; keep the config minimal.
                self.theme.preset = (next != "default").then(|| next.to_string());
            }
            SettingRow::Icons => self.icons = cycle(&IconPack::ALL, &self.icons, delta),
            SettingRow::Color => self.color = cycle(&ColorMode::ALL, &self.color, delta),
            SettingRow::Reset => {}
            color_row => {
                let slot = self.color_slot(color_row).expect("color row");
                // Options: preset (None), then the named colors. A custom
                // value (e.g. "#ff8800") steps as if it were "preset".
                let mut options: Vec<Option<&str>> = vec![None];
                options.extend(NAMED_COLORS.iter().map(|c| Some(*c)));
                let current = slot.as_deref().filter(|c| NAMED_COLORS.contains(c));
                *slot = cycle(&options, &current, delta).map(str::to_string);
            }
        }
    }
}

/// The item `delta` steps away from `current`, wrapping around. An unknown
/// `current` counts as the first item.
fn cycle<T: PartialEq + Copy>(items: &[T], current: &T, delta: i32) -> T {
    let len = items.len() as i32;
    let i = items.iter().position(|x| x == current).unwrap_or(0) as i32;
    items[(i + delta).rem_euclid(len) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_cycles_and_wraps() {
        let mut a = Appearance::default();
        a.step(SettingRow::Preset, 1);
        assert_eq!(a.theme.preset.as_deref(), Some("nord"));
        a.step(SettingRow::Preset, -2);
        assert_eq!(a.theme.preset.as_deref(), Some("catppuccin"));
        a.step(SettingRow::Preset, 1);
        assert_eq!(a.theme.preset, None, "default is stored as no preset");
    }

    #[test]
    fn colors_cycle_through_preset_and_named() {
        let mut a = Appearance::default();
        a.step(SettingRow::Accent, 1);
        assert_eq!(a.theme.accent.as_deref(), Some("cyan"));
        a.step(SettingRow::Accent, -1);
        assert_eq!(a.theme.accent, None);
        a.step(SettingRow::Accent, -1);
        assert_eq!(a.theme.accent.as_deref(), Some("black"));
    }

    #[test]
    fn custom_color_steps_from_preset() {
        let mut a = Appearance::default();
        a.theme.muted = Some("#123456".into());
        assert_eq!(a.value_label(SettingRow::Muted), "#123456");
        a.step(SettingRow::Muted, 1);
        assert_eq!(a.theme.muted.as_deref(), Some("cyan"));
    }

    #[test]
    fn icons_and_color_mode_cycle() {
        let mut a = Appearance::default();
        a.step(SettingRow::Icons, 1);
        assert_eq!(a.icons, IconPack::Ascii);
        a.step(SettingRow::Color, -1);
        assert_eq!(a.color, ColorMode::Never);
        assert_eq!(a.value_label(SettingRow::Color), "never");
    }
}
