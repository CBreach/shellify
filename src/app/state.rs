use std::time::{Duration, Instant};

use ratatui::widgets::{ListState, TableState};

use crate::app::action::{Pane, View};
use crate::app::queue::Queue;
use crate::app::settings::{Appearance, SettingRow};
use crate::command::LineEditor;
use crate::keymap::HelpEntry;
use crate::provider::{Playlist, Track};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// `:` prompt
    Command,
    /// `/` prompt
    Search,
    /// Typing a color value for a Settings row.
    EditSetting(SettingRow),
}

#[derive(Debug)]
pub struct Playback {
    pub paused: bool,
    pub position: Duration,
    pub volume: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Error,
}

#[derive(Debug)]
pub struct Status {
    pub text: String,
    pub level: StatusLevel,
    pub since: Instant,
}

impl Status {
    /// Messages fade so the footer hints come back; errors linger longer.
    pub fn expired(&self) -> bool {
        let ttl = match self.level {
            StatusLevel::Info => Duration::from_secs(4),
            StatusLevel::Error => Duration::from_secs(8),
        };
        self.since.elapsed() >= ttl
    }
}

/// A footer hint: `key label`, e.g. `a add`.
#[derive(Debug, Clone)]
pub struct Hint {
    pub key: String,
    pub label: &'static str,
}

pub struct AppState {
    pub mode: Mode,
    pub focus: Pane,
    pub should_quit: bool,

    pub library: Vec<Playlist>,
    pub library_state: ListState,

    /// Title of the middle pane: the open playlist or search query.
    pub tracks_title: String,
    pub tracks: Vec<Track>,
    pub tracks_state: TableState,

    pub queue: Queue,
    pub queue_state: ListState,

    pub playback: Playback,

    pub command_line: LineEditor,
    pub search_line: LineEditor,
    pub status: Option<Status>,

    pub help_open: bool,
    pub help_scroll: u16,
    pub help_entries: Vec<HelpEntry>,
    /// Footer hints per pane, indexed like `Pane::ALL`.
    pub hints: [Vec<Hint>; 3],
    pub settings_hints: Vec<Hint>,

    pub view: View,
    pub settings_cursor: usize,
    /// What the Settings tab shows and saves; the theme is derived from it.
    pub appearance: Appearance,
    pub setting_line: LineEditor,
    /// Where settings are saved, for display (`~/.config/...`).
    pub config_path_label: String,
    /// Header tab labels with their keys, e.g. `1 Music`.
    pub tab_labels: [String; 2],
}

impl AppState {
    pub fn new(library: Vec<Playlist>) -> Self {
        let (tracks_title, tracks) = library
            .first()
            .map(|p| (p.name.clone(), p.tracks.clone()))
            .unwrap_or_default();
        Self {
            mode: Mode::Normal,
            focus: Pane::Library,
            should_quit: false,
            library_state: ListState::default().with_selected(Some(0)),
            library,
            tracks_title,
            tracks_state: TableState::default().with_selected(Some(0)),
            tracks,
            queue: Queue::default(),
            queue_state: ListState::default().with_selected(Some(0)),
            playback: Playback {
                paused: false,
                position: Duration::ZERO,
                volume: 70,
            },
            command_line: LineEditor::default(),
            search_line: LineEditor::default(),
            status: None,
            help_open: false,
            help_scroll: 0,
            help_entries: Vec::new(),
            hints: Default::default(),
            settings_hints: Vec::new(),
            view: View::Music,
            settings_cursor: 0,
            appearance: Appearance::default(),
            setting_line: LineEditor::default(),
            config_path_label: String::new(),
            tab_labels: ["Music".into(), "Settings".into()],
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            level: StatusLevel::Info,
            since: Instant::now(),
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            level: StatusLevel::Error,
            since: Instant::now(),
        });
    }

    pub fn hints_for_focus(&self) -> &[Hint] {
        if self.view == View::Settings {
            return &self.settings_hints;
        }
        let i = Pane::ALL.iter().position(|p| *p == self.focus).unwrap_or(0);
        &self.hints[i]
    }

    /// Selected index and length of the focused pane's list.
    pub fn selection(&self, pane: Pane) -> (Option<usize>, usize) {
        match pane {
            Pane::Library => (self.library_state.selected(), self.library.len()),
            Pane::Tracks => (self.tracks_state.selected(), self.tracks.len()),
            Pane::Queue => (self.queue_state.selected(), self.queue.tracks().len()),
        }
    }

    pub fn set_selection(&mut self, pane: Pane, index: Option<usize>) {
        match pane {
            Pane::Library => self.library_state.select(index),
            Pane::Tracks => self.tracks_state.select(index),
            Pane::Queue => self.queue_state.select(index),
        }
    }

    /// Shows `tracks` in the middle pane and focuses it.
    pub fn selected_setting(&self) -> SettingRow {
        SettingRow::ALL[self.settings_cursor.min(SettingRow::ALL.len() - 1)]
    }

    pub fn show_tracks(&mut self, title: String, tracks: Vec<Track>) {
        self.tracks_title = title;
        self.tracks = tracks;
        self.tracks_state.select(Some(0));
        self.focus = Pane::Tracks;
        self.view = View::Music;
    }
}
