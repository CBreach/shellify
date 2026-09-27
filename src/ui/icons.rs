use ratatui::symbols::border;
use serde::Deserialize;

/// `[ui] icons`: which glyph set to draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconPack {
    /// Plain Unicode symbols; works in any modern terminal font.
    #[default]
    Unicode,
    /// 7-bit ASCII only, for legacy terminals, serial consoles and `TERM=linux`.
    Ascii,
    /// Nerd Font glyphs. Opt-in: a terminal can't report whether a Nerd Font
    /// is installed, so this is never auto-detected.
    Nerd,
}

impl IconPack {
    pub const ALL: [IconPack; 3] = [IconPack::Unicode, IconPack::Ascii, IconPack::Nerd];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unicode => "unicode",
            Self::Ascii => "ascii",
            Self::Nerd => "nerd",
        }
    }

    pub fn icons(self) -> &'static Icons {
        match self {
            Self::Unicode => &UNICODE,
            Self::Ascii => &ASCII,
            Self::Nerd => &NERD,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Icons {
    pub playing: &'static str,
    pub paused: &'static str,
    /// Marks the current entry in the queue.
    pub current: &'static str,
    pub playlist: &'static str,
    pub repeat: &'static str,
    pub error: &'static str,
    /// Progress/volume bar pieces.
    pub bar_filled: &'static str,
    pub bar_empty: &'static str,
    pub knob: &'static str,
    /// Appended to truncated text.
    pub ellipsis: &'static str,
    /// Frames of the loading spinner.
    pub spinner: &'static [&'static str],
    /// Separator in titles and hint bars.
    pub sep: &'static str,
    /// Color sample in the Settings tab.
    pub swatch: &'static str,
    /// Drawn across a draggable pane border (two cells: left, right).
    pub grip: [&'static str; 2],
    pub border: border::Set<'static>,
    /// Border for the focused pane when there is no color to mark it.
    pub border_focus: border::Set<'static>,
}

const UNICODE: Icons = Icons {
    playing: "▶",
    paused: "⏸",
    current: "♪",
    playlist: "♫",
    repeat: "⟳",
    error: "✗",
    bar_filled: "━",
    bar_empty: "─",
    knob: "●",
    ellipsis: "…",
    spinner: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    sep: "·",
    swatch: "███",
    grip: ["◂", "▸"],
    border: border::ROUNDED,
    border_focus: border::THICK,
};

const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

const ASCII: Icons = Icons {
    playing: ">",
    paused: "=",
    current: "*",
    playlist: "+",
    repeat: "R",
    error: "x",
    bar_filled: "=",
    bar_empty: "-",
    knob: "o",
    ellipsis: "...",
    spinner: &["|", "/", "-", "\\"],
    sep: "|",
    swatch: "###",
    grip: ["<", ">"],
    border: ASCII_BORDER,
    border_focus: border::Set {
        top_left: "#",
        top_right: "#",
        bottom_left: "#",
        bottom_right: "#",
        vertical_left: "#",
        vertical_right: "#",
        horizontal_top: "=",
        horizontal_bottom: "=",
    },
};

/// Font Awesome codepoints, which are stable across Nerd Fonts v2 and v3
/// (v3 moved only the Material Design range).
const NERD: Icons = Icons {
    playing: "\u{f04b}",
    paused: "\u{f04c}",
    current: "\u{f028}",
    playlist: "\u{f001}",
    repeat: "\u{f01e}",
    error: "\u{f00d}",
    ..UNICODE
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_pack_is_pure_ascii() {
        let i = IconPack::Ascii.icons();
        let border = &i.border;
        let focus = &i.border_focus;
        let all = [
            i.playing,
            i.paused,
            i.current,
            i.playlist,
            i.repeat,
            i.error,
            i.bar_filled,
            i.bar_empty,
            i.knob,
            i.ellipsis,
            i.sep,
            i.swatch,
            i.grip[0],
            i.grip[1],
            border.top_left,
            border.horizontal_top,
            border.vertical_left,
            focus.top_left,
            focus.horizontal_top,
            focus.vertical_left,
        ];
        for glyph in all {
            assert!(glyph.is_ascii(), "{glyph:?}");
        }
    }

    #[test]
    fn single_cell_glyphs() {
        use unicode_width::UnicodeWidthStr;
        for pack in [IconPack::Unicode, IconPack::Ascii, IconPack::Nerd] {
            let i = pack.icons();
            for glyph in [i.bar_filled, i.bar_empty, i.knob] {
                assert_eq!(glyph.width(), 1, "{pack:?} {glyph:?}");
            }
        }
    }
}
