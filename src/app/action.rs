use std::time::Duration;

/// Everything the user can do. Key bindings and `:commands` both resolve to an
/// `Action`, so every feature is reachable from command mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,

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
pub enum Pane {
    Library,
    Tracks,
    Queue,
}

impl Pane {
    pub const ALL: [Pane; 3] = [Pane::Library, Pane::Tracks, Pane::Queue];
}
