use std::time::Duration;

use crate::app::visualizer::VizStyle;

/// Everything the user can do. Key bindings and `:commands` both resolve to an
/// `Action`, so every feature is reachable from command mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Open the help overlay.
    Help,
    /// Switch between the Music and Settings tabs.
    View(View),
    /// Change a side pane's width (percent of the window).
    Resize(Resize),
    /// Switch, import or reload color themes.
    Theme(ThemeCommand),
    /// Show, hide or restyle the audio visualizer.
    Visualizer(VizCommand),

    // Playback
    TogglePause,
    Next,
    Prev,
    Seek(Seek),
    Volume(Volume),
    Repeat(Option<RepeatMode>),

    // Navigation
    Select(Select),
    Focus(Focus),

    // Library / queue
    PlaySelected,
    PlayQuery(String),
    AddSelected,
    ClearQueue,
    Shuffle,
    Search(String),
    /// Open the `/` search prompt.
    OpenSearch,
    /// Play a URL or local file directly (anything mpv/yt-dlp can open).
    Open(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    To(Duration),
    Forward(Duration),
    Back(Duration),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volume {
    Set(u8),
    Change(i16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Select {
    By(i32),
    First,
    Last,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Next,
    Prev,
    Pane(Pane),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VizCommand {
    Toggle,
    On,
    Off,
    /// Next style (turns it on if it was off).
    NextStyle,
    Style(VizStyle),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeCommand {
    /// Switch to a built-in or custom theme by id.
    Use(String),
    /// Validate a theme file (Shellify `.toml` or base16 `.yaml`), copy it
    /// into the themes folder and switch to it.
    Import(String),
    /// Re-read the themes folder (after editing or adding files by hand).
    Reload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resize {
    Reset,
    Set(Pane, u16),
    Change(Pane, i16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Music,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Library,
    Tracks,
    Queue,
}

impl Pane {
    pub const ALL: [Pane; 3] = [Pane::Library, Pane::Tracks, Pane::Queue];
}
