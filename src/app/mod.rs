pub mod action;
mod demo;
mod dispatch;
pub mod queue;
pub mod settings;
pub mod state;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;

use crate::command::{self, Completion};
use crate::config::Config;
use crate::keymap::{KeyPress, KeyResult, Keymap};
use crate::ui;
use crate::ui::theme::Theme;
use action::{Action, View};
use settings::Appearance;
use state::{AppState, Hint, Mode};

const TICK: Duration = Duration::from_millis(250);

/// Everything the main loop reacts to. Player and provider events join this
/// enum in later steps so the loop stays a single `recv`.
#[derive(Debug)]
pub enum AppEvent {
    Input(Event),
    Tick,
}

pub struct App {
    state: AppState,
    keymap: Keymap,
    theme: Theme,
    /// Where the Settings tab saves to.
    config_path: PathBuf,
    last_tick: Instant,
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
        };
        state.config_path_label = display_path(&config_path);
        state.tab_labels = [("view music", "Music"), ("view settings", "Settings")].map(
            |(cmd, name)| match keymap.key_for(cmd) {
                Some(key) => format!("{key} {name}"),
                None => name.to_string(),
            },
        );
        Ok(Self {
            state,
            keymap,
            theme: Theme::from_config(&config.theme, config.ui.color, config.ui.icons)?,
            config_path,
            last_tick: Instant::now(),
        })
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        spawn_input(tx.clone());
        spawn_ticker(tx);

        self.state.info("Welcome to Shellify! Press ? for help");
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

    fn handle(&mut self, event: AppEvent) {
        match event {
            AppEvent::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => self.on_key(key),
            AppEvent::Input(_) => {} // resize etc.: the next draw picks it up
            AppEvent::Tick => {
                if self.state.status.as_ref().is_some_and(|s| s.expired()) {
                    self.state.status = None;
                }
                let now = Instant::now();
                self.on_tick(now - self.last_tick);
                self.last_tick = now;
            }
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
