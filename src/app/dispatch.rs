use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::BaseDirs;

use super::action::{
    Action, Focus, Pane, Resize, Seek, Select, ThemeCommand, View, VizCommand, Volume,
};
use super::settings::SettingRow;
use super::state::{Mode, StatusLevel};
use super::{App, set_mouse_capture};
use crate::config;
use crate::player::{EndReason, PlayerEvent};
use crate::provider::{ProviderKind, Source, Track};
use crate::themes;
use crate::ui::layout::PaneSizes;
use crate::ui::theme::{Theme, parse_color, theme_ids};

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
        if self.state.view == View::Providers && self.providers_action(&action) {
            return;
        }
        match action {
            Action::Quit => self.state.should_quit = true,
            Action::Help => {
                self.state.help_open = true;
                self.state.help_scroll = 0;
            }
            Action::View(view) => self.state.view = view,
            Action::Resize(resize) => self.resize(resize),
            Action::Theme(command) => self.theme_command(command),
            Action::Visualizer(command) => self.visualizer_command(command),
            Action::Provider(kind) => self.use_provider(kind),

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
            Action::PlayQuery(query) => self.search(query, true),
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
            Action::Search(query) => self.search(query, false),
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

    /// On the Providers tab, every direction moves between providers and
    /// Enter opens the highlighted one's setup screen.
    fn providers_action(&mut self, action: &Action) -> bool {
        let last = ProviderKind::ALL.len() as i64 - 1;
        let cursor = self.state.provider_cursor as i64;
        let target = match action {
            Action::Select(Select::By(delta)) => cursor + i64::from(*delta),
            Action::Select(Select::First) => 0,
            Action::Select(Select::Last) => last,
            Action::Focus(Focus::Next) | Action::Seek(Seek::Forward(_)) => cursor + 1,
            Action::Focus(Focus::Prev) | Action::Seek(Seek::Back(_)) => cursor - 1,
            Action::Focus(Focus::Pane(_)) => {
                self.state.view = View::Music;
                return false;
            }
            Action::PlaySelected => {
                self.choose_provider(self.state.provider_cursor);
                return true;
            }
            _ => return false,
        };
        let target = target.clamp(0, last) as usize;
        if target != self.state.provider_cursor {
            self.state.provider_cursor = target;
            // Each newly highlighted logo starts its hop from the ground.
            self.state.bounce = 0.0;
        }
        true
    }

    /// Enter (or a click) on a provider. One that's available is switched
    /// on, or back off if it's already in use; the others open their setup
    /// screen, which says what they'll need. Turning one on also shows the
    /// setup screen, and switches to the provider's theme.
    pub(super) fn choose_provider(&mut self, index: usize) {
        let Some(&kind) = ProviderKind::ALL.get(index) else {
            return;
        };
        self.state.provider_cursor = index;
        self.state.bounce = 0.0;
        if kind.available() && self.state.active_provider == Some(kind) {
            self.use_provider(None);
            return;
        }
        let theme = &mut self.state.appearance.theme.preset;
        if theme.as_deref() != Some(kind.theme()) {
            *theme = Some(kind.theme().to_string());
            self.apply_appearance();
            if !self.status_is_error() {
                self.state.info(format!("Theme: {}", kind.name()));
            }
        }
        if kind.available() {
            self.use_provider(Some(kind));
        }
        self.state.provider_setup = Some(kind);
    }

    /// Switches to `kind`'s library (`None`: the demo one) and saves it.
    fn use_provider(&mut self, kind: Option<ProviderKind>) {
        if kind == self.state.active_provider {
            let name = kind.map_or("the demo library", ProviderKind::name);
            return self.state.info(format!("Already using {name}"));
        }
        match kind {
            None => {
                self.set_provider(None);
                self.state.info("Back to the demo tracks");
            }
            Some(kind) => {
                let Some(provider) = kind.connect() else {
                    let status = kind.status();
                    return self
                        .state
                        .error(format!("{} isn't available yet ({status})", kind.name()));
                };
                self.set_provider(Some(provider));
                let search = self.search_hint();
                self.state
                    .info(format!("{} is on: {search} to search", kind.name()));
            }
        }
        if let Err(e) = config::save_provider(&self.config_path, kind.map(ProviderKind::id)) {
            self.state
                .error(format!("couldn't save the provider: {e:#}"));
        }
    }

    fn step_setting(&mut self, delta: i32) {
        let row = self.state.selected_setting();
        if row != SettingRow::Reset {
            let themes = theme_ids(&self.state.user_themes);
            self.state.appearance.step_in(row, delta, &themes);
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

    /// `:visualizer [on|off|next|<style>]`, `v`, `V`.
    fn visualizer_command(&mut self, command: VizCommand) {
        let a = &mut self.state.appearance;
        match command {
            VizCommand::Toggle => a.visualizer = !a.visualizer,
            VizCommand::On => a.visualizer = true,
            VizCommand::Off => a.visualizer = false,
            VizCommand::NextStyle => {
                a.visualizer_style = a.visualizer_style.next();
                a.visualizer = true;
            }
            VizCommand::Style(style) => {
                a.visualizer_style = style;
                a.visualizer = true;
            }
            VizCommand::Fade(on) => {
                a.visualizer_fade = on.unwrap_or(!a.visualizer_fade);
                a.visualizer = true;
            }
        }
        self.apply_appearance();
        if self.status_is_error() {
            return;
        }
        let a = &self.state.appearance;
        let msg = if a.visualizer {
            let fade = if a.visualizer_fade { ", fading" } else { "" };
            format!("Visualizer on: {}{fade}", a.visualizer_style.label())
        } else {
            "Visualizer off".to_string()
        };
        self.state.info(msg);
    }

    /// `:theme <name>`, `:theme import <file>`, `:theme reload`.
    fn theme_command(&mut self, command: ThemeCommand) {
        match command {
            ThemeCommand::Use(id) => {
                let ids = theme_ids(&self.state.user_themes);
                if !ids.contains(&id) {
                    self.state.error(format!(
                        "unknown theme {id:?} (available: {})",
                        ids.join(", ")
                    ));
                    return;
                }
                // "default" is the implicit preset; keep the config minimal.
                self.state.appearance.theme.preset = (id != "default").then(|| id.clone());
                self.apply_appearance();
                if !self.status_is_error() {
                    self.state.info(format!("Theme: {}", self.theme_label(&id)));
                }
            }
            ThemeCommand::Import(path) => {
                let dir = themes::themes_dir(&self.config_path);
                match themes::import(&expand_home(&path), &dir) {
                    Ok(id) => {
                        self.reload_themes();
                        self.theme_command(ThemeCommand::Use(id.clone()));
                        if !self.status_is_error() {
                            let label = self.theme_label(&id);
                            self.state
                                .info(format!("Imported {label} and switched to it"));
                        }
                    }
                    Err(e) => self.state.error(format!("theme import: {e:#}")),
                }
            }
            ThemeCommand::Reload => {
                let problems = self.reload_themes();
                if problems.is_empty() {
                    let n = self.state.user_themes.len();
                    self.state.info(format!("Reloaded themes: {n} custom"));
                } else {
                    self.state
                        .error(format!("Skipped theme file: {}", problems.join("; ")));
                }
            }
        }
    }

    /// Re-reads the themes folder and re-applies the current theme (its file
    /// may have been edited). Falls back to the default if it disappeared.
    /// Returns the files that couldn't be loaded.
    fn reload_themes(&mut self) -> Vec<String> {
        let (themes, problems) = themes::load_dir(&themes::themes_dir(&self.config_path));
        self.state.user_themes = themes;
        let current = &mut self.state.appearance.theme.preset;
        if current
            .as_ref()
            .is_some_and(|id| !theme_ids(&self.state.user_themes).contains(id))
        {
            *current = None;
        }
        let a = &self.state.appearance;
        if let Ok(theme) = Theme::from_config(&a.theme, a.color, a.icons, &self.state.user_themes) {
            self.theme = theme;
        }
        problems
    }

    /// "Tokyo Night (custom)" for custom themes, the id for built-ins.
    fn theme_label(&self, id: &str) -> String {
        match self.state.user_themes.iter().find(|t| t.id == id) {
            Some(t) => format!("{} (custom)", t.name),
            None => id.to_string(),
        }
    }

    fn status_is_error(&self) -> bool {
        self.state
            .status
            .as_ref()
            .is_some_and(|s| s.level == StatusLevel::Error)
    }

    /// `:resize`: change a side pane's width and save it.
    fn resize(&mut self, resize: Resize) {
        let panes = self.state.appearance.panes;
        self.state.appearance.panes = match resize {
            Resize::Reset => PaneSizes::default(),
            Resize::Set(pane, pct) => panes.with(pane, pct),
            Resize::Change(pane, delta) => {
                let current = panes.get(pane).unwrap_or(0);
                let pct = (i32::from(current) + i32::from(delta)).clamp(0, 100) as u16;
                panes.with(pane, pct)
            }
        };
        self.save_pane_sizes();
    }

    /// Saves the current pane sizes (with the rest of the settings) and
    /// reports them, replacing the generic "Saved" message.
    pub(super) fn save_pane_sizes(&mut self) {
        self.apply_appearance();
        if self
            .state
            .status
            .as_ref()
            .is_some_and(|s| s.level == StatusLevel::Error)
        {
            return;
        }
        let p = self.state.appearance.panes;
        self.state.info(format!(
            "Library {}%, Queue {}% (saved)",
            p.library, p.queue
        ));
    }

    /// Rebuilds the live theme from the Settings values and saves them.
    fn apply_appearance(&mut self) {
        let a = &self.state.appearance;
        match Theme::from_config(&a.theme, a.color, a.icons, &self.state.user_themes) {
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
        // Only meter loudness while something wants it.
        let visualizer = a.visualizer;
        if let Some(player) = self.player.as_mut() {
            player.set_metering(visualizer);
        }
        // The pointer setting (or mouse) may just have been switched off.
        self.sync_pointer();
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
            Pane::Library => self.open_playlist(index),
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
            source: Source::Direct(source),
        });
        self.state.queue.jump(self.state.queue.tracks().len() - 1);
        self.start_current();
    }

    fn add_selected(&mut self) {
        let (selected, _) = self.state.selection(self.state.focus);
        let Some(index) = selected else { return };
        match self.state.focus {
            Pane::Library => self.add_playlist(index),
            Pane::Tracks => {
                if let Some(track) = self.state.tracks.get(index).cloned() {
                    self.state.queue.push(track);
                    self.state.info("added 1 track(s) to the queue");
                }
            }
            Pane::Queue => {}
        }
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
    pub(super) fn start_current(&mut self) {
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
        let source = match self.playback_source(track) {
            Ok(source) => source,
            Err(e) => return self.on_track_failed(&e),
        };
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
            PlayerEvent::Levels { rms_db, peak_db } => self.state.visualizer.meter(rms_db, peak_db),
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
        let current = self.state.queue.current();
        let title = current.map(|t| t.title.clone()).unwrap_or_default();
        tracing::warn!(%title, %error, "track failed");
        let error = current.map_or(error, |t| explain_failure(t, error));
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

/// mpv's reason for a failed track, made useful where we can tell more.
fn explain_failure<'a>(track: &Track, error: &'a str) -> &'a str {
    // When yt-dlp gets no audio, mpv tries to play the web page itself. For
    // YouTube Music signed out, that almost always means the song is only
    // available to signed-in users.
    if track.source == Source::Service(ProviderKind::YouTubeMusic)
        && error == "unrecognized file format"
    {
        "YouTube didn't send any audio (some songs need you to sign in)"
    } else {
        error
    }
}

/// Expands a leading `~/` (typed paths in `:theme import`).
fn expand_home(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), BaseDirs::new()) {
        (Some(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::app::testing::FakePlayer;
    use crate::config::Config;

    fn app() -> (App, Arc<Mutex<Vec<String>>>) {
        let config_path = std::env::temp_dir().join("shellify-test-config.toml");
        let mut app = App::new(&Config::default(), config_path).unwrap();
        let player = FakePlayer::default();
        let calls = player.calls.clone();
        app.player = Some(Box::new(player));
        (app, calls)
    }

    #[test]
    fn visualizer_toggles_mpv_metering_and_levels_drive_it() {
        let (mut app, calls) = app();
        app.dispatch(Action::Visualizer(VizCommand::On));
        app.dispatch(Action::Visualizer(VizCommand::Off));
        let calls = calls.lock().unwrap().clone();
        assert!(calls.contains(&"metering true".to_string()), "{calls:?}");
        assert_eq!(calls.last().map(String::as_str), Some("metering false"));

        app.on_player_event(PlayerEvent::Levels {
            rms_db: -12.0,
            peak_db: -6.0,
        });
        for _ in 0..10 {
            app.state.visualizer.tick(1.0 / 30.0, true);
        }
        assert!(app.state.visualizer.level > 0.5);
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

#[cfg(test)]
mod theme_tests {
    use std::path::{Path, PathBuf};

    use ratatui::style::Color;

    use super::*;
    use crate::config::Config;

    const OCEAN_YAML: &str = "scheme: \"Ocean Breeze\"\nbase00: \"101820\"\nbase03: \"4a5a6a\"\nbase05: \"d0e0f0\"\nbase08: \"ff6060\"\nbase0D: \"33aaff\"\n";

    fn app_at(config_path: &Path) -> App {
        let config = Config::load(config_path).unwrap();
        App::new(&config, config_path.to_path_buf()).unwrap()
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        (dir, config)
    }

    #[test]
    fn importing_a_base16_scheme_switches_to_it_and_saves() {
        let (dir, config) = setup();
        let yaml = dir.path().join("ocean.yaml");
        std::fs::write(&yaml, OCEAN_YAML).unwrap();
        let mut app = app_at(&config);

        app.dispatch(Action::Theme(ThemeCommand::Import(
            yaml.display().to_string(),
        )));

        assert_eq!(app.theme.accent, Color::Rgb(0x33, 0xaa, 0xff));
        assert_eq!(
            app.state.appearance.theme.preset.as_deref(),
            Some("ocean-breeze")
        );
        let status = app.state.status.as_ref().unwrap();
        assert!(
            status.text.contains("Imported Ocean Breeze (custom)"),
            "{}",
            status.text
        );
        assert!(dir.path().join("themes/ocean-breeze.toml").exists());
        let saved = std::fs::read_to_string(&config).unwrap();
        assert!(saved.contains("preset = \"ocean-breeze\""), "{saved}");

        // A fresh start picks the custom theme back up from config + themes/.
        let restarted = app_at(&config);
        assert_eq!(restarted.theme.accent, Color::Rgb(0x33, 0xaa, 0xff));
    }

    #[test]
    fn unknown_theme_is_rejected_and_nothing_changes() {
        let (_dir, config) = setup();
        let mut app = app_at(&config);
        app.dispatch(Action::Theme(ThemeCommand::Use("vaporwave".into())));
        assert_eq!(app.state.status.as_ref().unwrap().level, StatusLevel::Error);
        assert_eq!(app.state.appearance.theme.preset, None);
        assert!(!config.exists());
    }

    #[test]
    fn a_deleted_theme_falls_back_to_default_on_reload_and_restart() {
        let (dir, config) = setup();
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("mine.toml"), "accent = \"#010203\"\n").unwrap();
        std::fs::write(&config, "[theme]\npreset = \"mine\"\n").unwrap();

        let mut app = app_at(&config);
        assert_eq!(app.theme.accent, Color::Rgb(1, 2, 3));

        std::fs::remove_file(themes.join("mine.toml")).unwrap();
        app.dispatch(Action::Theme(ThemeCommand::Reload));
        assert_eq!(app.state.appearance.theme.preset, None);
        assert_eq!(app.theme.accent, Theme::default().accent);

        // Starting with a config that names a missing theme still works.
        let restarted = app_at(&config);
        assert_eq!(restarted.theme.accent, Theme::default().accent);
        assert!(
            restarted
                .state
                .status
                .as_ref()
                .unwrap()
                .text
                .contains("not found")
        );
    }

    #[test]
    fn broken_theme_files_warn_but_dont_block_startup() {
        let (dir, config) = setup();
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("oops.toml"), "accent = \"blurple\"\n").unwrap();
        std::fs::write(themes.join("fine.toml"), "text = \"white\"\n").unwrap();

        let app = app_at(&config);
        let ids: Vec<_> = app
            .state
            .user_themes
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(ids, ["fine"]);
        let status = app.state.status.as_ref().unwrap();
        assert_eq!(status.level, StatusLevel::Error);
        assert!(status.text.contains("oops.toml"), "{}", status.text);
    }

    #[test]
    fn settings_cycles_through_custom_themes() {
        let (dir, config) = setup();
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("zebra.toml"), "accent = \"white\"\n").unwrap();
        let mut app = app_at(&config);
        app.state.view = View::Settings;
        app.state.settings_cursor = 0; // Theme row
        // The built-ins (default, nord, gruvbox, catppuccin and the three
        // provider themes), then zebra.
        for _ in 0..crate::ui::theme::PRESETS.len() {
            app.dispatch(Action::Focus(Focus::Next));
        }
        assert_eq!(app.state.appearance.theme.preset.as_deref(), Some("zebra"));
        assert_eq!(app.theme.accent, Color::White);
    }
}

#[cfg(test)]
mod provider_tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;

    use super::*;
    use crate::config::Config;

    fn app() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let mut app = App::new(&Config::default(), config).unwrap();
        app.dispatch(Action::View(View::Providers));
        (dir, app)
    }

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn arrows_move_between_providers_and_stop_at_the_ends() {
        let (_dir, mut app) = app();
        assert_eq!(app.state.provider_cursor, 0);
        key(&mut app, KeyCode::Char('l'));
        key(&mut app, KeyCode::Right);
        assert_eq!(app.state.provider_cursor, 2);
        key(&mut app, KeyCode::Char('l'));
        assert_eq!(app.state.provider_cursor, 2, "clamped");
        key(&mut app, KeyCode::Char('k'));
        assert_eq!(app.state.provider_cursor, 1);
        assert_eq!(app.state.view, View::Providers, "stays on the tab");
    }

    #[test]
    fn moving_restarts_the_bounce() {
        let (_dir, mut app) = app();
        assert!(app.bouncing());
        app.state.bounce = 0.5;
        key(&mut app, KeyCode::Char('l'));
        assert_eq!(app.state.bounce, 0.0);
        app.dispatch(Action::View(View::Music));
        assert!(!app.bouncing(), "no animation off the tab");
    }

    #[test]
    fn enter_opens_setup_and_switches_to_the_provider_theme() {
        let (dir, mut app) = app();
        key(&mut app, KeyCode::Char('l')); // Spotify
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.state.provider_setup, Some(ProviderKind::Spotify));
        assert_eq!(
            app.state.appearance.theme.preset.as_deref(),
            Some("spotify")
        );
        assert_eq!(app.theme.accent, Color::Rgb(0x1e, 0xd7, 0x60), "green");
        let saved = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(saved.contains("preset = \"spotify\""), "{saved}");

        // The setup screen captures keys until it's closed.
        key(&mut app, KeyCode::Char('h'));
        assert_eq!(app.state.provider_cursor, 1);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.state.provider_setup, None);
        assert_eq!(app.state.view, View::Providers);

        // Restarting with that theme highlights its provider.
        let config = Config::load(&dir.path().join("config.toml")).unwrap();
        let app = App::new(&config, dir.path().join("config.toml")).unwrap();
        assert_eq!(app.state.provider_cursor, 1);
    }

    /// Enter on YouTube Music switches it on (signed out, so nothing touches
    /// the network until a search), and again switches back to the demo.
    #[tokio::test]
    async fn enter_toggles_youtube_music_and_remembers_it() {
        let (dir, mut app) = app();
        let config_path = dir.path().join("config.toml");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.state.active_provider, Some(ProviderKind::YouTubeMusic));
        assert!(!app.state.is_demo());
        assert_eq!(app.state.provider_setup, Some(ProviderKind::YouTubeMusic));
        let status = app.state.status.as_ref().unwrap();
        assert!(
            status.text.contains("YouTube Music is on"),
            "{}",
            status.text
        );
        let saved = std::fs::read_to_string(&config_path).unwrap();
        assert!(saved.contains("active = \"youtube-music\""), "{saved}");
        assert!(saved.contains("preset = \"youtube-music\""), "{saved}");

        // Restarting picks it up again.
        let config = Config::load(&config_path).unwrap();
        let restarted = App::new(&config, config_path.clone()).unwrap();
        assert_eq!(restarted.startup_provider, Some(ProviderKind::YouTubeMusic));

        key(&mut app, KeyCode::Esc); // close the setup screen
        key(&mut app, KeyCode::Enter);
        assert!(app.state.is_demo());
        assert_eq!(
            app.state.provider_setup, None,
            "turning off shows no screen"
        );
        assert_eq!(app.state.library[0].name, "Liked Songs");
        let saved = std::fs::read_to_string(&config_path).unwrap();
        assert!(!saved.contains("[providers]"), "{saved}");
    }

    #[tokio::test]
    async fn provider_command_switches_and_rejects_unbuilt_ones() {
        let (_dir, mut app) = app();
        app.dispatch(Action::Provider(Some(ProviderKind::Spotify)));
        assert!(app.state.is_demo());
        assert!(app.status_is_error());

        app.dispatch(Action::Provider(Some(ProviderKind::YouTubeMusic)));
        assert_eq!(app.state.active_provider, Some(ProviderKind::YouTubeMusic));
        assert_eq!(
            app.state.provider_setup, None,
            "only Enter opens the screen"
        );
        app.dispatch(Action::Provider(None));
        assert!(app.state.is_demo());
    }

    #[test]
    fn an_unknown_provider_in_the_config_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let config: Config = toml::from_str("[providers]\nactive = \"napster\"").unwrap();
        let app = App::new(&config, dir.path().join("config.toml")).unwrap();
        assert_eq!(app.startup_provider, None);
        assert!(app.status_is_error());
    }

    #[test]
    fn a_youtube_song_that_needs_sign_in_says_so() {
        let track = |source| Track {
            id: "x".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            duration: Duration::ZERO,
            source,
        };
        let yt = track(Source::Service(ProviderKind::YouTubeMusic));
        assert!(explain_failure(&yt, "unrecognized file format").contains("sign in"));
        assert_eq!(explain_failure(&yt, "network error"), "network error");
        let file = track(Source::Direct("/tmp/a.txt".into()));
        assert_eq!(
            explain_failure(&file, "unrecognized file format"),
            "unrecognized file format"
        );
    }
}
