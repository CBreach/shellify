pub mod action;
mod demo;
mod dispatch;
mod mouse;
pub(crate) mod pointer;
pub mod queue;
pub mod settings;
pub mod state;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::command::{self, Completion};
use crate::config::Config;
use crate::keymap::{KeyPress, KeyResult, Keymap};
use crate::player::{self, LoadId, MpvOptions, MpvPlayer, Player, PlayerEvent};
use crate::themes;
use crate::ui;
use crate::ui::layout::PaneSizes;
use crate::ui::theme::Theme;
use crate::ui::theme::theme_ids;
use action::Pane;
use action::{Action, View};
use settings::Appearance;
use state::{AppState, Hint, Mode};

const TICK: Duration = Duration::from_millis(250);

/// Everything the main loop reacts to. Provider events join this enum in a
/// later step so the loop stays a single `recv`.
#[derive(Debug)]
pub enum AppEvent {
    Input(Event),
    Tick,
    Player(PlayerEvent),
}

pub struct App {
    state: AppState,
    keymap: Keymap,
    theme: Theme,
    /// Where the Settings tab saves to.
    config_path: PathBuf,
    /// `None` when mpv couldn't be started or has died.
    player: Option<Box<dyn Player>>,
    /// The load whose events we act on; events from older loads are stale.
    current_load: Option<LoadId>,
    /// Tracks that failed to play in a row, to stop skipping through a queue
    /// that can't play at all (e.g. no network).
    failures: usize,
    /// Whether terminal mouse capture is currently on (follows the setting).
    mouse_captured: bool,
    /// Whether we asked the terminal for the resize pointer (see `pointer`).
    pointer_resize: bool,
    /// Last left click, for double-click detection.
    last_click: Option<(ratatui::layout::Position, Instant)>,
}

impl App {
    pub fn new(config: &Config, config_path: PathBuf) -> Result<Self> {
        let keymap = Keymap::new(&config.keys)?;
        let mut state = AppState::new(demo::library());
        state.help_entries = keymap.help_entries();
        state.hints = pane_hints(&keymap);
        state.settings_hints = settings_hints(&keymap);
        state.appearance = Appearance {
            theme: config.theme.clone(),
            color: config.ui.color,
            icons: config.ui.icons,
            mouse: config.ui.mouse,
            resize_cursor: config.ui.resize_cursor,
            panes: PaneSizes {
                library: config
                    .ui
                    .library_width
                    .unwrap_or(PaneSizes::default().library),
                queue: config.ui.queue_width.unwrap_or(PaneSizes::default().queue),
            }
            .clamped(Pane::Library),
        };
        state.config_path_label = display_path(&config_path);
        let (user_themes, problems) = themes::load_dir(&themes::themes_dir(&config_path));
        state.user_themes = user_themes;
        if !problems.is_empty() {
            state.error(format!("Skipped theme file: {}", problems.join("; ")));
        }
        // A theme that was deleted or renamed shouldn't stop Shellify starting.
        if let Some(name) = state.appearance.theme.preset.clone()
            && !theme_ids(&state.user_themes).contains(&name)
        {
            state.error(format!("Theme {name:?} not found; using the default theme"));
            state.appearance.theme.preset = None;
        }
        state.tab_labels = [("view music", "Music"), ("view settings", "Settings")].map(
            |(cmd, name)| match keymap.key_for(cmd) {
                Some(key) => format!("{key} {name}"),
                None => name.to_string(),
            },
        );
        let theme = Theme::from_config(
            &state.appearance.theme,
            config.ui.color,
            config.ui.icons,
            &state.user_themes,
        )?;
        Ok(Self {
            state,
            keymap,
            theme,
            config_path,
            player: None,
            current_load: None,
            failures: 0,
            mouse_captured: false,
            pointer_resize: false,
            last_click: None,
        })
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        spawn_input(tx.clone());
        spawn_ticker(tx.clone());

        if self.state.appearance.mouse {
            self.mouse_captured = true;
            set_mouse_capture(true);
        }
        // Keep startup warnings (bad theme files...) rather than hiding them.
        if self.state.status.is_none() {
            self.state.info("Welcome to Shellify! Press ? for help");
        }
        self.start_player(tx).await;
        // Clean up however the loop ends, including a draw error.
        let result = self.event_loop(&mut terminal, &mut rx).await;
        if let Some(mut player) = self.player.take() {
            player.shutdown().await;
        }
        // Always release the mouse, or the shell keeps receiving escape codes.
        set_mouse_capture(false);
        pointer::restore();
        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut DefaultTerminal,
        rx: &mut mpsc::UnboundedReceiver<AppEvent>,
    ) -> Result<()> {
        while !self.state.should_quit {
            terminal.draw(|frame| ui::draw(frame, &mut self.state, &self.theme))?;
            let Some(event) = rx.recv().await else { break };
            self.handle(event);
            // Apply anything else already queued before redrawing.
            while let Ok(event) = rx.try_recv() {
                self.handle(event);
            }
        }
        Ok(())
    }

    async fn start_player(&mut self, tx: mpsc::UnboundedSender<AppEvent>) {
        let (player_tx, mut player_rx) = mpsc::unbounded_channel();
        let options = MpvOptions {
            volume: self.state.playback.volume,
            ..Default::default()
        };
        match MpvPlayer::spawn(options, player_tx).await {
            Ok(player) => {
                self.player = Some(Box::new(player));
                // mpv needs yt-dlp for YouTube sources, and only says so per track.
                if !player::on_path("yt-dlp") {
                    self.state.error(
                        "yt-dlp not found: YouTube tracks won't play (install it, e.g. `brew install yt-dlp`)",
                    );
                }
            }
            Err(e) => {
                tracing::error!("starting mpv: {e:#}");
                self.state.error(format!("{e:#}"));
            }
        }
        tokio::spawn(async move {
            while let Some(event) = player_rx.recv().await {
                if tx.send(AppEvent::Player(event)).is_err() {
                    break;
                }
            }
        });
    }

    fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => self.on_key(key),
            AppEvent::Input(Event::Mouse(mouse)) => self.on_mouse(mouse),
            AppEvent::Input(_) => {} // resize etc.: the next draw picks it up
            AppEvent::Tick => {
                // Periodic redraw; also clears expired status messages.
                if self.state.status.as_ref().is_some_and(|s| s.expired()) {
                    self.state.status = None;
                }
            }
            AppEvent::Player(event) => self.on_player_event(event),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.state.should_quit = true;
            return;
        }
        if self.state.help_open {
            self.on_help_key(key);
            return;
        }
        match self.state.mode {
            Mode::Normal => self.on_normal_key(key),
            Mode::Command | Mode::Search | Mode::EditSetting(_) => self.on_prompt_key(key),
        }
    }

    /// The help overlay captures input: scroll it or close it.
    fn on_help_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let scroll = &mut self.state.help_scroll;
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | '?') => {
                self.state.help_open = false;
            }
            KeyCode::Char('j') | KeyCode::Down => *scroll = scroll.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Char('d') if ctrl => *scroll = scroll.saturating_add(10),
            KeyCode::Char('u') if ctrl => *scroll = scroll.saturating_sub(10),
            KeyCode::PageDown => *scroll = scroll.saturating_add(10),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
            KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
            // Clamped to the content height when drawn.
            KeyCode::Char('G') | KeyCode::End => *scroll = u16::MAX,
            _ => {}
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(':') => {
                self.keymap.reset();
                self.state.status = None;
                self.state.command_line.clear();
                self.state.mode = Mode::Command;
            }
            KeyCode::Esc => {
                self.keymap.reset();
                self.state.status = None;
                self.state.view = View::Music;
            }
            _ => match self.keymap.press(KeyPress::from_event(key)) {
                KeyResult::Action(action) => self.dispatch(action),
                KeyResult::Pending | KeyResult::Unbound => {}
            },
        }
    }

    fn on_prompt_key(&mut self, key: KeyEvent) {
        // Hints (e.g. completion candidates) are only valid until the next key.
        self.state.status = None;
        let mode = self.state.mode;
        let line = match mode {
            Mode::Search => &mut self.state.search_line,
            Mode::EditSetting(_) => &mut self.state.setting_line,
            Mode::Normal | Mode::Command => &mut self.state.command_line,
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                line.clear();
                self.state.mode = Mode::Normal;
            }
            KeyCode::Backspace if line.is_empty() => self.state.mode = Mode::Normal,
            KeyCode::Backspace => line.backspace(),
            KeyCode::Delete => line.delete(),
            KeyCode::Left => line.left(),
            KeyCode::Right => line.right(),
            KeyCode::Home => line.home(),
            KeyCode::End => line.end(),
            KeyCode::Up => line.history_prev(),
            KeyCode::Down => line.history_next(),
            KeyCode::Char('a') if ctrl => line.home(),
            KeyCode::Char('e') if ctrl => line.end(),
            KeyCode::Char('u') if ctrl => line.clear(),
            KeyCode::Tab if mode == Mode::Command => match command::complete(line.text()) {
                Completion::Replace(text) => line.set(text),
                Completion::Candidates(names) => self.state.info(names.join("  ")),
                Completion::None => {}
            },
            KeyCode::Char(c) if !ctrl => line.insert(c),
            KeyCode::Enter => {
                let text = line.submit();
                self.state.mode = Mode::Normal;
                let query = text.trim();
                match mode {
                    Mode::EditSetting(row) => self.submit_setting(row, &text),
                    _ if query.is_empty() => {}
                    Mode::Search => self.dispatch(Action::Search(query.to_string())),
                    _ => match command::parse(query) {
                        Ok(action) => self.dispatch(action),
                        Err(e) => self.state.error(e),
                    },
                }
            }
            _ => {}
        }
    }
}

/// Footer hints for each pane (indexed like `Pane::ALL`), using whatever keys
/// the user actually has bound. Unbound commands are left out.
fn pane_hints(keymap: &Keymap) -> [Vec<Hint>; 3] {
    let build = |items: &[(&str, &'static str)]| -> Vec<Hint> {
        let mut hints: Vec<Hint> = items
            .iter()
            .filter_map(|&(cmd, label)| keymap.key_for(cmd).map(|key| Hint { key, label }))
            .collect();
        hints.push(Hint {
            key: ":".into(),
            label: "command",
        });
        hints
    };
    [
        build(&[
            ("play", "open"),
            ("add", "add all"),
            ("search", "search"),
            ("help", "help"),
        ]),
        build(&[
            ("play", "play"),
            ("add", "add"),
            ("pause", "pause"),
            ("search", "search"),
            ("help", "help"),
        ]),
        build(&[
            ("play", "play"),
            ("next", "next"),
            ("shuffle", "shuffle"),
            ("help", "help"),
        ]),
    ]
}

/// Turns terminal mouse reporting on or off. With it on, the terminal's own
/// click-drag selection needs Shift held (in most emulators).
pub(crate) fn set_mouse_capture(on: bool) {
    // Tests drive the app without a terminal; writing this to the real stdout
    // would switch on mouse reporting in the developer's shell.
    if cfg!(test) {
        return;
    }
    let mut out = std::io::stdout();
    let result = if on {
        crossterm::execute!(out, EnableMouseCapture)
    } else {
        crossterm::execute!(out, DisableMouseCapture)
    };
    if let Err(e) = result {
        tracing::warn!(
            "couldn't {} mouse capture: {e}",
            if on { "enable" } else { "disable" }
        );
    }
}

fn settings_hints(keymap: &Keymap) -> Vec<Hint> {
    [
        ("select +1", "move"),
        ("focus next", "change"),
        ("play", "edit"),
        ("view music", "music"),
    ]
    .iter()
    .filter_map(|&(cmd, label)| keymap.key_for(cmd).map(|key| Hint { key, label }))
    .collect()
}

/// `/Users/me/.config/x` -> `~/.config/x`, for status messages.
fn display_path(path: &Path) -> String {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
    match home.and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rel) => format!("~/{}", rel.display()),
        None => path.display().to_string(),
    }
}

fn spawn_input(tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut events = EventStream::new();
        while let Some(event) = events.next().await {
            match event {
                Ok(event) => {
                    if tx.send(AppEvent::Input(event)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("terminal input error: {e}");
                    break;
                }
            }
        }
    });
}

fn spawn_ticker(tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        loop {
            interval.tick().await;
            if tx.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });
}
