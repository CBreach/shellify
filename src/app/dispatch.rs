use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::BaseDirs;

use super::action::{Action, Focus, Pane, Seek, Select, View, Volume};
use super::settings::SettingRow;
use super::state::Mode;
use super::{App, demo, set_mouse_capture};
use crate::config;
use crate::player::{EndReason, PlayerEvent};
use crate::provider::Track;
use crate::ui::theme::{Theme, parse_color};

/// Consecutive playback failures after which we stop instead of skipping on.
const MAX_FAILURES: usize = 3;

/// Turns an `:open` argument into something mpv can open: URLs pass through,
/// paths get `~` expanded, are made absolute and must exist.
fn resolve_open_target(target: &str) -> Result<String, String> {
    if target.contains("://") {
        return Ok(target.to_string());
    }
    let mut path = match target.strip_prefix("~/") {
        Some(rest) => BaseDirs::new()
            .map(|d| d.home_dir().join(rest))
            .ok_or("can't find your home directory")?,
        None => PathBuf::from(target),
    };
    if path.is_relative() {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        path = cwd.join(path);
    }
    if !path.exists() {
        return Err(format!("open: no such file: {}", path.display()));
    }
    Ok(path.to_string_lossy().into_owned())
}

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
                    let paused = !self.state.playback.paused;
                    self.state.playback.paused = paused;
                    if let Some(player) = &mut self.player {
                        player.set_pause(paused);
                    }
                }
            }
            Action::Next => {
                self.state.queue.advance(false);
                self.start_current();
            }
            Action::Prev => {
                // Like most players: restart the track unless we're near its start.
                if self.state.playback.position > Duration::from_secs(3) {
                    self.seek(Seek::To(Duration::ZERO));
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
                let volume = new.clamp(0, 100) as u8;
                self.state.playback.volume = volume;
                if let Some(player) = &mut self.player {
                    player.set_volume(volume);
                }
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
            Action::Open(target) => self.open(&target),
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
        if a.mouse != self.mouse_captured {
            self.mouse_captured = a.mouse;
            set_mouse_capture(a.mouse);
        }
        let a = &self.state.appearance;
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

    /// Queues a URL or local file and plays it right away.
    fn open(&mut self, target: &str) {
        let source = match resolve_open_target(target) {
            Ok(source) => source,
            Err(e) => return self.state.error(e),
        };
        let title = if source.contains("://") {
            source.clone()
        } else {
            Path::new(&source)
                .file_name()
                .map_or_else(|| source.clone(), |n| n.to_string_lossy().into_owned())
        };
        self.state.queue.push(Track {
            id: format!("open:{source}"),
            title,
            artist: String::new(),
            duration: Duration::ZERO,
            source: Some(source),
        });
        self.state.queue.jump(self.state.queue.tracks().len() - 1);
        self.start_current();
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
        let playback = &mut self.state.playback;
        if playback.loading {
            return;
        }
        let duration = playback.duration.unwrap_or(track.duration);
        let pos = playback.position;
        let mut target = match seek {
            Seek::To(t) => t,
            Seek::Forward(d) => pos + d,
            Seek::Back(d) => pos.saturating_sub(d),
        };
        if !duration.is_zero() {
            target = target.min(duration);
        }
        playback.position = target;
        if let Some(player) = &mut self.player {
            player.seek(target);
        }
    }

    /// Starts the queue's current track from the beginning, or stops playback
    /// if there is none.
    fn start_current(&mut self) {
        self.state
            .queue_state
            .select(self.state.queue.current_index());
        let playback = &mut self.state.playback;
        playback.position = Duration::ZERO;
        playback.duration = None;
        playback.paused = false;
        playback.loading = false;
        self.current_load = None;

        let Some(track) = self.state.queue.current() else {
            if let Some(player) = &mut self.player {
                player.stop();
            }
            return;
        };
        let source = demo::playback_source(track);
        match &mut self.player {
            Some(player) => {
                tracing::info!(%source, "loading");
                self.current_load = Some(player.load(&source));
                self.state.playback.loading = true;
            }
            None => self.state.error("can't play: mpv is not running"),
        }
    }

    pub(super) fn on_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::Loaded(load) if Some(load) == self.current_load => {
                self.state.playback.loading = false;
                self.failures = 0;
            }
            PlayerEvent::Ended { load, reason } if Some(load) == self.current_load => {
                self.current_load = None;
                match reason {
                    EndReason::Eof => {
                        self.state.queue.advance(true);
                        self.start_current();
                    }
                    EndReason::Error(e) => self.on_track_failed(&e),
                }
            }
            // Events for a load we've since replaced.
            PlayerEvent::Loaded(_) | PlayerEvent::Ended { .. } => {}
            // Until the new track is open, properties may still describe the old one.
            PlayerEvent::Position(_) | PlayerEvent::Duration(_) if self.state.playback.loading => {}
            PlayerEvent::Position(pos) => self.state.playback.position = pos,
            PlayerEvent::Duration(d) => self.state.playback.duration = Some(d),
            PlayerEvent::Paused(p) => self.state.playback.paused = p,
            PlayerEvent::Exited(msg) => {
                tracing::error!("player exited: {msg}");
                self.player = None;
                self.current_load = None;
                self.state.playback.loading = false;
                self.state.error(format!("{msg}; playback stopped"));
            }
        }
    }

    fn on_track_failed(&mut self, error: &str) {
        self.failures += 1;
        let title = self
            .state
            .queue
            .current()
            .map(|t| t.title.clone())
            .unwrap_or_default();
        tracing::warn!(%title, %error, "track failed");
        let limit = MAX_FAILURES.min(self.state.queue.tracks().len());
        if self.failures >= limit {
            self.failures = 0;
            self.state.playback.loading = false;
            if let Some(player) = &mut self.player {
                player.stop();
            }
            self.state
                .error(format!("couldn't play {title:?}: {error} (stopped)"));
            return;
        }
        self.state
            .error(format!("couldn't play {title:?}: {error}"));
        self.state.queue.advance(false);
        self.start_current();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::config::Config;
    use crate::player::{LoadId, Player};

    /// Records what the app asked the player to do.
    #[derive(Default)]
    struct FakePlayer {
        calls: Arc<Mutex<Vec<String>>>,
        loads: LoadId,
    }

    #[async_trait]
    impl Player for FakePlayer {
        fn load(&mut self, source: &str) -> LoadId {
            self.loads += 1;
            self.calls.lock().unwrap().push(format!("load {source}"));
            self.loads
        }
        fn set_pause(&mut self, paused: bool) {
            self.calls.lock().unwrap().push(format!("pause {paused}"));
        }
        fn seek(&mut self, position: Duration) {
            let secs = position.as_secs();
            self.calls.lock().unwrap().push(format!("seek {secs}"));
        }
        fn set_volume(&mut self, volume: u8) {
            self.calls.lock().unwrap().push(format!("volume {volume}"));
        }
        fn stop(&mut self) {
            self.calls.lock().unwrap().push("stop".into());
        }
        async fn shutdown(&mut self) {}
    }

    fn app() -> (App, Arc<Mutex<Vec<String>>>) {
        let config_path = std::env::temp_dir().join("shellify-test-config.toml");
        let mut app = App::new(&Config::default(), config_path).unwrap();
        let player = FakePlayer::default();
        let calls = player.calls.clone();
        app.player = Some(Box::new(player));
        (app, calls)
    }

    /// Plays the "Liked Songs" demo playlist from its first track.
    fn play_liked(app: &mut App) {
        app.dispatch(Action::PlaySelected); // open the playlist
        app.dispatch(Action::PlaySelected); // play track 0
    }

    fn take(calls: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        std::mem::take(&mut *calls.lock().unwrap())
    }

    #[test]
    fn playing_a_demo_track_loads_a_youtube_search() {
        let (mut app, calls) = app();
        play_liked(&mut app);
        assert_eq!(
            take(&calls),
            ["load ytdl://ytsearch1:Daft Punk Harder, Better, Faster, Stronger"]
        );
        assert!(app.state.playback.loading);
        app.on_player_event(PlayerEvent::Loaded(1));
        assert!(!app.state.playback.loading);
    }

    #[test]
    fn eof_advances_but_stale_events_are_ignored() {
        let (mut app, calls) = app();
        play_liked(&mut app);
        app.dispatch(Action::Next); // load 2 replaces load 1
        take(&calls);

        // Load 1's end arrives late: must not skip again.
        app.on_player_event(PlayerEvent::Ended {
            load: 1,
            reason: EndReason::Eof,
        });
        assert!(take(&calls).is_empty());
        assert_eq!(app.state.queue.current_index(), Some(1));

        app.on_player_event(PlayerEvent::Loaded(2));
        app.on_player_event(PlayerEvent::Ended {
            load: 2,
            reason: EndReason::Eof,
        });
        assert_eq!(app.state.queue.current_index(), Some(2));
        assert_eq!(take(&calls), ["load ytdl://ytsearch1:Kavinsky Nightcall"]);
    }

    #[test]
    fn position_updates_wait_until_the_new_track_is_loaded() {
        let (mut app, _) = app();
        play_liked(&mut app);
        app.on_player_event(PlayerEvent::Position(Duration::from_secs(99)));
        assert_eq!(app.state.playback.position, Duration::ZERO);
        app.on_player_event(PlayerEvent::Loaded(1));
        app.on_player_event(PlayerEvent::Position(Duration::from_secs(5)));
        app.on_player_event(PlayerEvent::Duration(Duration::from_secs(230)));
        assert_eq!(app.state.playback.position, Duration::from_secs(5));
        assert_eq!(app.state.playback.duration, Some(Duration::from_secs(230)));
    }

    #[test]
    fn failures_skip_ahead_then_stop() {
        let (mut app, calls) = app();
        play_liked(&mut app);
        for load in 1..=MAX_FAILURES as u64 {
            app.on_player_event(PlayerEvent::Ended {
                load,
                reason: EndReason::Error("loading failed".into()),
            });
        }
        let calls = take(&calls);
        let loads = calls.iter().filter(|c| c.starts_with("load")).count();
        assert_eq!(loads, MAX_FAILURES);
        assert_eq!(calls.last().map(String::as_str), Some("stop"));
        assert!(!app.state.playback.loading);
        assert!(app.state.status.as_ref().unwrap().text.contains("stopped"));
    }

    #[test]
    fn controls_reach_the_player() {
        let (mut app, calls) = app();
        play_liked(&mut app);
        app.on_player_event(PlayerEvent::Loaded(1));
        take(&calls);
        app.dispatch(Action::TogglePause);
        app.dispatch(Action::Seek(Seek::To(Duration::from_secs(90))));
        app.dispatch(Action::Seek(Seek::To(Duration::from_secs(9999))));
        app.dispatch(Action::Volume(Volume::Change(-20)));
        assert_eq!(
            take(&calls),
            ["pause true", "seek 90", "seek 224", "volume 50"]
        );
    }

    #[test]
    fn open_plays_urls_and_existing_files() {
        let (mut app, calls) = app();
        app.dispatch(Action::Open("https://example.com/stream.mp3".into()));
        assert_eq!(take(&calls), ["load https://example.com/stream.mp3"]);

        // Any existing file works for resolving; cargo runs tests from the crate root.
        app.dispatch(Action::Open("Cargo.toml".into()));
        let expected = std::env::current_dir().unwrap().join("Cargo.toml");
        assert_eq!(take(&calls), [format!("load {}", expected.display())]);
        assert_eq!(app.state.queue.current().unwrap().title, "Cargo.toml");
        assert_eq!(app.state.queue.tracks().len(), 2);
    }

    #[test]
    fn open_rejects_missing_files() {
        let (mut app, calls) = app();
        app.dispatch(Action::Open("no/such/file.mp3".into()));
        assert!(take(&calls).is_empty());
        let status = app.state.status.as_ref().unwrap();
        assert!(status.text.contains("no such file"));
    }

    #[test]
    fn player_exit_is_reported() {
        let (mut app, _) = app();
        app.on_player_event(PlayerEvent::Exited("mpv exited unexpectedly".into()));
        assert!(app.player.is_none());
        play_liked(&mut app);
        let status = app.state.status.as_ref().unwrap();
        assert!(status.text.contains("mpv is not running"));
    }
}
