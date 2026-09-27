pub mod action;
mod demo;
mod dispatch;
pub mod queue;
pub mod state;

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
use action::Action;
use state::{AppState, Mode};

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
    last_tick: Instant,
}

impl App {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            state: AppState::new(demo::library()),
            keymap: Keymap::new(&config.keys)?,
            last_tick: Instant::now(),
        })
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        spawn_input(tx.clone());
        spawn_ticker(tx);

        self.state
            .info("Welcome to Shellify. Press : for commands, q to quit.");
        while !self.state.should_quit {
            terminal.draw(|frame| ui::draw(frame, &mut self.state))?;
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
        match self.state.mode {
            Mode::Normal => self.on_normal_key(key),
            Mode::Command | Mode::Search => self.on_prompt_key(key),
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
        let searching = self.state.mode == Mode::Search;
        let line = if searching {
            &mut self.state.search_line
        } else {
            &mut self.state.command_line
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
            KeyCode::Tab if !searching => match command::complete(line.text()) {
                Completion::Replace(text) => line.set(text),
                Completion::Candidates(names) => self.state.info(names.join("  ")),
                Completion::None => {}
            },
            KeyCode::Char(c) if !ctrl => line.insert(c),
            KeyCode::Enter => {
                let text = line.submit();
                self.state.mode = Mode::Normal;
                if searching {
                    if !text.trim().is_empty() {
                        self.dispatch(Action::Search(text.trim().to_string()));
                    }
                } else if !text.trim().is_empty() {
                    match command::parse(&text) {
                        Ok(action) => self.dispatch(action),
                        Err(e) => self.state.error(e),
                    }
                }
            }
            _ => {}
        }
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
