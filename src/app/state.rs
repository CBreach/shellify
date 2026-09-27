use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::widgets::{ListState, TableState};

use crate::app::action::{Pane, View};
use crate::app::queue::Queue;
use crate::app::settings::{Appearance, SettingRow};
use crate::command::LineEditor;
use crate::keymap::HelpEntry;
use crate::provider::{Playlist, Track};
use crate::ui::layout::PaneSizes;

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

/// Where clickable things were drawn in the last frame, so mouse events can
/// be mapped back to them. Rebuilt on every draw.
#[derive(Debug, Default)]
pub struct HitMap {
    pub tabs: Vec<(Rect, View)>,
    pub lists: Vec<ListHit>,
    /// The progress bar itself (between the time labels), for click-to-seek.
    pub progress: Option<Rect>,
    /// Settings rows on screen: (row area, index into `SettingRow::ALL`).
    pub settings_rows: Vec<(Rect, usize)>,
    pub help: Option<Rect>,
    /// Draggable pane borders: (grab area, the side pane it resizes).
    pub dividers: Vec<(Rect, Pane)>,
    /// The area the three panes share, for converting a drag to a percentage.
    pub panes_area: Option<Rect>,
}

#[derive(Debug, Clone, Copy)]
pub struct ListHit {
    pub pane: Pane,
    /// The whole pane, borders included (a click here focuses it).
    pub area: Rect,
    /// The rows holding items (below borders and any table header).
    pub rows: Rect,
    /// Screen lines per item (the queue shows two per track).
    pub item_height: u16,
}

impl ListHit {
    /// The item index under screen row `y`, given the list's scroll offset.
    pub fn item_at(&self, y: u16, offset: usize) -> Option<usize> {
        if y < self.rows.y || y >= self.rows.bottom() {
            return None;
        }
        Some(offset + usize::from((y - self.rows.y) / self.item_height.max(1)))
    }
}

/// An in-progress drag of a pane border.
#[derive(Debug, Clone, Copy)]
pub struct BorderDrag {
    /// The side pane being resized.
    pub pane: Pane,
    /// Sizes when the drag began. Every motion is clamped against these, so
    /// squeezing the other pane is undone if the user drags back.
    pub start: PaneSizes,
    /// Pointer column minus the pane's own border column at press time; the
    /// grab zone is two cells wide, so this keeps the border from jumping.
    pub grab_offset: i32,
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

    pub hits: HitMap,
    /// The pane border under the mouse pointer, highlighted as draggable.
    pub divider_hover: Option<Pane>,
    /// The pane border being dragged right now.
    pub divider_drag: Option<BorderDrag>,
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
            hits: HitMap::default(),
            divider_hover: None,
            divider_drag: None,
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

    /// Scroll offset of a pane's list (first visible item).
    pub fn list_offset(&self, pane: Pane) -> usize {
        match pane {
            Pane::Library => self.library_state.offset(),
            Pane::Tracks => self.tracks_state.offset(),
            Pane::Queue => self.queue_state.offset(),
        }
    }

    pub fn selected_setting(&self) -> SettingRow {
        SettingRow::ALL[self.settings_cursor.min(SettingRow::ALL.len() - 1)]
    }

    /// Shows `tracks` in the middle pane and focuses it.
    pub fn show_tracks(&mut self, title: String, tracks: Vec<Track>) {
        self.tracks_title = title;
        self.tracks = tracks;
        self.tracks_state.select(Some(0));
        self.focus = Pane::Tracks;
        self.view = View::Music;
    }
}
