use std::time::Duration;

use ratatui::widgets::{ListState, TableState};

use crate::app::action::Pane;
use crate::app::queue::Queue;
use crate::command::LineEditor;
use crate::provider::{Playlist, Track};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// `:` prompt
    Command,
    /// `/` prompt
    Search,
}

#[derive(Debug)]
pub struct Playback {
    pub paused: bool,
    /// Waiting for the player to open the current track (yt-dlp can take a
    /// few seconds to resolve a search).
    pub loading: bool,
    pub position: Duration,
    /// Length reported by the player; more accurate than track metadata.
    pub duration: Option<Duration>,
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
                loading: false,
                position: Duration::ZERO,
                duration: None,
                volume: 70,
            },
            command_line: LineEditor::default(),
            search_line: LineEditor::default(),
            status: None,
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            level: StatusLevel::Info,
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            level: StatusLevel::Error,
        });
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
    pub fn show_tracks(&mut self, title: String, tracks: Vec<Track>) {
        self.tracks_title = title;
        self.tracks = tracks;
        self.tracks_state.select(Some(0));
        self.focus = Pane::Tracks;
    }
}
