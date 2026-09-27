use std::time::Duration;

use super::action::{Action, Focus, Pane, Seek, Select, View, Volume};
use super::settings::SettingRow;
use super::state::Mode;
use super::{App, demo};
use crate::config;
use crate::ui::theme::{Theme, parse_color};

impl App {
    pub(super) fn dispatch(&mut self, action: Action) {
        tracing::debug!(?action, "dispatch");
        if self.state.view == View::Settings && self.settings_action(&action) {
            return;
        }
        match action {
            Action::Quit => self.state.should_quit = true,
            Action::Help => {
                self.state.help_open = true;
                self.state.help_scroll = 0;
            }
            Action::View(view) => self.state.view = view,

            Action::TogglePause => {
                if self.state.queue.current().is_some() {
                    self.state.playback.paused = !self.state.playback.paused;
                }
            }
            Action::Next => {
                self.state.queue.advance(false);
                self.start_current();
            }
            Action::Prev => {
                // Like most players: restart the track unless we're near its start.
                if self.state.playback.position > Duration::from_secs(3) {
                    self.state.playback.position = Duration::ZERO;
                } else {
                    self.state.queue.back();
                    self.start_current();
                }
            }
            Action::Seek(seek) => self.seek(seek),
            Action::Volume(v) => {
                let current = i16::from(self.state.playback.volume);
                let new = match v {
                    Volume::Set(n) => i16::from(n),
                    Volume::Change(delta) => current + delta,
                };
                self.state.playback.volume = new.clamp(0, 100) as u8;
            }
            Action::Repeat(mode) => {
                let queue = &mut self.state.queue;
                queue.repeat = mode.unwrap_or_else(|| queue.repeat.cycle());
                let label = queue.repeat.label();
                self.state.info(format!("repeat: {label}"));
            }

            Action::Select(sel) => self.select(sel),
            Action::Focus(focus) => {
                let panes = Pane::ALL;
                let i = panes
                    .iter()
                    .position(|p| *p == self.state.focus)
                    .unwrap_or(0);
                self.state.focus = match focus {
                    Focus::Next => panes[(i + 1) % panes.len()],
                    Focus::Prev => panes[(i + panes.len() - 1) % panes.len()],
                    Focus::Pane(p) => p,
                };
            }

            Action::PlaySelected => self.play_selected(),
            Action::PlayQuery(query) => {
                let results = demo::search(&self.state.library, &query);
                if results.is_empty() {
                    self.state.error(format!("no results for {query:?}"));
                    return;
                }
                self.state.queue.play_list(results.clone(), 0);
                self.state.show_tracks(format!("Search: {query}"), results);
                self.start_current();
            }
            Action::AddSelected => self.add_selected(),
            Action::ClearQueue => {
                self.state.queue.clear();
                self.start_current();
                self.state.info("queue cleared");
            }
            Action::Shuffle => {
                self.state.queue.shuffle();
                self.state.info("queue shuffled");
            }
            Action::Search(query) => {
                let results = demo::search(&self.state.library, &query);
                self.state
                    .info(format!("{} results for {query:?}", results.len()));
                self.state.show_tracks(format!("Search: {query}"), results);
            }
            Action::OpenSearch => {
                self.state.status = None;
                self.state.search_line.clear();
                self.state.mode = super::state::Mode::Search;
            }
        }
    }

    /// Navigation and editing keys mean something else on the Settings tab:
    /// up/down move between rows, left/right (h/l, arrows) change the value,
    /// Enter activates. Going through actions keeps user rebindings working.
    /// Returns whether the action was handled here.
    fn settings_action(&mut self, action: &Action) -> bool {
        let rows = SettingRow::ALL.len();
        let cursor = &mut self.state.settings_cursor;
        match action {
            Action::Select(Select::By(delta)) => {
                *cursor = (*cursor as i64 + i64::from(*delta)).clamp(0, rows as i64 - 1) as usize;
            }
            Action::Select(Select::First) => *cursor = 0,
            Action::Select(Select::Last) => *cursor = rows - 1,
            Action::Focus(Focus::Next) | Action::Seek(Seek::Forward(_)) => self.step_setting(1),
            Action::Focus(Focus::Prev) | Action::Seek(Seek::Back(_)) => self.step_setting(-1),
            Action::Focus(Focus::Pane(_)) => {
                // Jumping to a pane implies the Music tab; let it run there.
                self.state.view = View::Music;
                return false;
            }
            Action::PlaySelected => self.activate_setting(),
            _ => return false,
        }
        true
    }

    fn step_setting(&mut self, delta: i32) {
        let row = self.state.selected_setting();
        if row != SettingRow::Reset {
            self.state.appearance.step(row, delta);
            self.apply_appearance();
        }
    }

    /// Enter: type a value for color rows, reset on the Reset row, else step.
    fn activate_setting(&mut self) {
        let row = self.state.selected_setting();
        match row {
            SettingRow::Reset => {
                self.state.appearance = Default::default();
                self.apply_appearance();
            }
            _ if row.color_key().is_some() => {
                let current = self
                    .state
                    .appearance
                    .color_slot(row)
                    .and_then(|slot| slot.clone())
                    .unwrap_or_default();
                self.state.status = None;
                self.state.setting_line.set(current);
                self.state.mode = Mode::EditSetting(row);
            }
            _ => self.step_setting(1),
        }
    }

    /// Enter in the color prompt: empty means "use the preset's color".
    pub(super) fn submit_setting(&mut self, row: SettingRow, text: &str) {
        let text = text.trim();
        let value = if text.is_empty() {
            None
        } else if let Err(e) = parse_color(text) {
            self.state
                .error(format!("{}: {e}", row.color_key().unwrap_or("color")));
            return;
        } else {
            Some(text.to_string())
        };
        if let Some(slot) = self.state.appearance.color_slot(row) {
            *slot = value;
        }
        self.apply_appearance();
    }

    /// Rebuilds the live theme from the Settings values and saves them.
    fn apply_appearance(&mut self) {
        let a = &self.state.appearance;
        match Theme::from_config(&a.theme, a.color, a.icons) {
            Ok(theme) => self.theme = theme,
            Err(e) => {
                self.state.error(e.to_string());
                return;
            }
        }
        match config::save_appearance(&self.config_path, a) {
            Ok(()) => {
                let label = self.state.config_path_label.clone();
                self.state.info(format!("Saved to {label}"));
            }
            Err(e) => self.state.error(format!("couldn't save settings: {e:#}")),
        }
    }

    fn select(&mut self, sel: Select) {
        let pane = self.state.focus;
        let (current, len) = self.state.selection(pane);
        if len == 0 {
            self.state.set_selection(pane, None);
            return;
        }
        let last = len - 1;
        let index = match sel {
            Select::First => 0,
            Select::Last => last,
            Select::By(delta) => {
                let cur = current.unwrap_or(0) as i64;
                (cur + i64::from(delta)).clamp(0, last as i64) as usize
            }
        };
        self.state.set_selection(pane, Some(index));
    }

    fn play_selected(&mut self) {
        let (selected, _) = self.state.selection(self.state.focus);
        let Some(index) = selected else { return };
        match self.state.focus {
            Pane::Library => {
                let Some(playlist) = self.state.library.get(index) else {
                    return;
                };
                let (name, tracks) = (playlist.name.clone(), playlist.tracks.clone());
                self.state.show_tracks(name, tracks);
            }
            Pane::Tracks => {
                if index < self.state.tracks.len() {
                    self.state.queue.play_list(self.state.tracks.clone(), index);
                    self.start_current();
                }
            }
            Pane::Queue => {
                self.state.queue.jump(index);
                self.start_current();
            }
        }
    }

    fn add_selected(&mut self) {
        let (selected, _) = self.state.selection(self.state.focus);
        let Some(index) = selected else { return };
        let added: Vec<_> = match self.state.focus {
            Pane::Library => self
                .state
                .library
                .get(index)
                .map(|p| p.tracks.clone())
                .unwrap_or_default(),
            Pane::Tracks => self.state.tracks.get(index).cloned().into_iter().collect(),
            Pane::Queue => return,
        };
        let n = added.len();
        for track in added {
            self.state.queue.push(track);
        }
        self.state.info(format!("added {n} track(s) to the queue"));
    }

    fn seek(&mut self, seek: Seek) {
        let Some(track) = self.state.queue.current() else {
            return;
        };
        let duration = track.duration;
        let pos = self.state.playback.position;
        let target = match seek {
            Seek::To(t) => t,
            Seek::Forward(d) => pos + d,
            Seek::Back(d) => pos.saturating_sub(d),
        };
        self.state.playback.position = target.min(duration);
    }

    /// Starts the queue's current track from the beginning (or stops if none).
    /// Until the mpv player lands (roadmap step 2) playback is simulated.
    fn start_current(&mut self) {
        self.state.playback.position = Duration::ZERO;
        self.state.playback.paused = false;
        self.state
            .queue_state
            .select(self.state.queue.current_index());
    }

    pub(super) fn on_tick(&mut self, elapsed: Duration) {
        let Some(track) = self.state.queue.current() else {
            return;
        };
        if self.state.playback.paused {
            return;
        }
        let duration = track.duration;
        self.state.playback.position += elapsed;
        if self.state.playback.position >= duration {
            self.state.queue.advance(true);
            self.start_current();
        }
    }
}
