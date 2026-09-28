pub mod action;
mod demo;
mod dispatch;
mod library;
mod mouse;
pub(crate) mod pointer;
pub mod queue;
pub mod settings;
pub(crate) mod signin;
pub mod state;
#[cfg(test)]
mod testing;
pub mod visualizer;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::{mpsc, watch};

use crate::auth::SecretStore;
use crate::auth::google::{Endpoints, Session};
use crate::command::{self, Completion};
use crate::config::Config;
use crate::keymap::{KeyPress, KeyResult, Keymap};
use crate::player::{self, LoadId, MpvOptions, MpvPlayer, Player, PlayerEvent};
use crate::provider::{Provider, ProviderKind};
use crate::themes;
use crate::ui;
use crate::ui::layout::PaneSizes;
use crate::ui::theme::Theme;
use crate::ui::theme::theme_ids;
use action::Pane;
use action::{Action, View};
use library::{ProviderEvent, Requests};
use settings::Appearance;
use state::{AppState, Hint, Mode};

const FRAME: Duration = Duration::from_millis(33);
const TICK: Duration = Duration::from_millis(250);

/// Everything the main loop reacts to, so the loop is a single `recv`.
#[derive(Debug)]
pub enum AppEvent {
    Input(Event),
    Tick,
    /// Animation frame (~30fps), only sent while the visualizer is moving.
    Frame,
    Player(PlayerEvent),
    /// A reply from a provider request (see `library`).
    Provider(ProviderEvent),
    /// Progress signing in (see `signin`).
    SignIn(signin::SignInEvent),
}

pub struct App {
    state: AppState,
    keymap: Keymap,
    theme: Theme,
    /// Where the Settings tab saves to.
    config_path: PathBuf,
    /// `None` when mpv couldn't be started or has died.
    player: Option<Box<dyn Player>>,
    /// The streaming service in use; `None` shows the demo library.
    provider: Option<Arc<dyn Provider>>,
    /// The provider saved in the config, switched to once `run` starts.
    startup_provider: Option<ProviderKind>,
    /// Provider requests waiting for a reply.
    requests: Requests,
    /// Where secrets are kept: the OS keychain (memory in tests).
    secrets: Arc<dyn SecretStore>,
    /// Google's endpoints (a mock server in tests).
    google_endpoints: Endpoints,
    /// The user's own Google OAuth client ID, from the config.
    google_client_id: Option<String>,
    /// The signed-in YouTube Music session.
    youtube_session: Option<Arc<Session>>,
    /// The sign-in task in flight, and its run number: messages from older
    /// runs are ignored.
    sign_in_task: Option<tokio::task::AbortHandle>,
    sign_in_run: u64,
    /// The main loop's event channel, for tasks to report back on.
    events: mpsc::UnboundedSender<AppEvent>,
    /// Its receiving end, until `run` takes it.
    inbox: Option<mpsc::UnboundedReceiver<AppEvent>>,
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
    /// Turns the animation clock on and off (`None` in tests: no clock).
    animate: Option<watch::Sender<bool>>,
    last_frame: Option<Instant>,
}

impl App {
    pub fn new(config: &Config, config_path: PathBuf) -> Result<Self> {
        let keymap = Keymap::new(&config.keys)?;
        let mut state = AppState::new(demo::library());
        state.help_entries = keymap.help_entries();
        state.hints = pane_hints(&keymap);
        state.settings_hints = settings_hints(&keymap);
        state.provider_hints = provider_hints(&keymap);
        state.appearance = Appearance {
            theme: config.theme.clone(),
            color: config.ui.color,
            icons: config.ui.icons,
            mouse: config.ui.mouse,
            resize_cursor: config.ui.resize_cursor,
            visualizer: config.ui.visualizer,
            visualizer_style: config.ui.visualizer_style,
            visualizer_fade: config.ui.visualizer_fade,
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
        state.providers_key = keymap.key_for("view providers");
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
        // Start on the provider whose theme is in use, if any.
        if let Some(i) = state
            .appearance
            .theme
            .preset
            .as_deref()
            .and_then(ProviderKind::for_theme)
            .and_then(|kind| ProviderKind::ALL.iter().position(|k| *k == kind))
        {
            state.provider_cursor = i;
        }
        state.tab_labels = [
            ("view music", "Music"),
            ("view settings", "Settings"),
            ("view providers", "Providers"),
        ]
        .map(|(cmd, name)| match keymap.key_for(cmd) {
            Some(key) => format!("{key} {name}"),
            None => name.to_string(),
        });
        let theme = Theme::from_config(
            &state.appearance.theme,
            config.ui.color,
            config.ui.icons,
            &state.user_themes,
        )?;
        let startup_provider = config.providers.active.as_deref().and_then(|id| {
            let kind = ProviderKind::from_id(id).filter(|k| k.available());
            if kind.is_none() {
                state.error(format!("Can't use provider {id:?} from the config"));
            }
            kind
        });
        let (events, inbox) = mpsc::unbounded_channel();
        Ok(Self {
            state,
            keymap,
            theme,
            config_path,
            player: None,
            provider: None,
            startup_provider,
            requests: Requests::default(),
            secrets: default_secret_store(),
            google_endpoints: Endpoints::default(),
            google_client_id: config.providers.youtube_music.client_id.clone(),
            youtube_session: None,
            sign_in_task: None,
            sign_in_run: 0,
            events,
            inbox: Some(inbox),
            current_load: None,
            failures: 0,
            mouse_captured: false,
            pointer_resize: false,
            last_click: None,
            animate: None,
            last_frame: None,
        })
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let tx = self.events.clone();
        let mut rx = self.inbox.take().expect("App::run is called once");
        spawn_input(tx.clone());
        spawn_ticker(tx.clone());
        let (animate, animate_rx) = watch::channel(false);
        spawn_animator(tx.clone(), animate_rx);
        self.animate = Some(animate);

        if self.state.appearance.mouse {
            self.mouse_captured = true;
            set_mouse_capture(true);
        }
        self.resume_provider();
        // Keep startup warnings (bad theme files...) rather than hiding them.
        if self.state.status.is_none() {
            let welcome = match self.state.active_provider {
                None => format!(
                    "Welcome! {} to add a provider, ? for help",
                    capitalize(&providers_hint(&self.state))
                ),
                Some(kind) => format!(
                    "Welcome! {} to search {}, ? for help",
                    capitalize(&self.search_hint()),
                    kind.name()
                ),
            };
            self.state.info(welcome);
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
            self.sync_animation();
        }
        Ok(())
    }

    /// Whether audio is actually coming out right now.
    fn is_playing(&self) -> bool {
        let p = &self.state.playback;
        self.state.queue.current().is_some() && !p.paused && !p.loading
    }

    /// Whether the Providers tab is showing its bouncing logo.
    fn bouncing(&self) -> bool {
        self.state.view == View::Providers && !self.state.help_open
    }

    /// Runs the animation clock only while something moves (the visualizer
    /// playing or settling, or a bouncing provider logo), so an idle
    /// Shellify doesn't redraw 30 times a second.
    fn sync_animation(&mut self) {
        let visualizer = self.state.appearance.visualizer
            && (self.is_playing() || !self.state.visualizer.at_rest());
        let want = visualizer || self.bouncing();
        if !want {
            self.last_frame = None;
        }
        if let Some(animate) = &self.animate {
            animate.send_if_modified(|on| std::mem::replace(on, want) != want);
        }
    }

    fn on_frame(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map_or(FRAME, |last| now.duration_since(last));
        self.last_frame = Some(now);
        let playing = self.is_playing();
        self.state.visualizer.tick(dt.as_secs_f32(), playing);
        if self.bouncing() {
            self.state.bounce += dt.as_secs_f32();
        }
    }

    async fn start_player(&mut self, tx: mpsc::UnboundedSender<AppEvent>) {
        let (player_tx, mut player_rx) = mpsc::unbounded_channel();
        let options = MpvOptions {
            volume: self.state.playback.volume,
            ..Default::default()
        };
        match MpvPlayer::spawn(options, player_tx).await {
            Ok(mut player) => {
                if self.state.appearance.visualizer {
                    player.set_metering(true);
                }
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
            AppEvent::Frame => self.on_frame(),
            AppEvent::Player(event) => self.on_player_event(event),
            AppEvent::Provider(event) => self.on_provider_event(event),
            AppEvent::SignIn(event) => self.on_sign_in_event(event),
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
        // An open prompt gets the keys, even over the setup screen (`:login`
        // asks for the client ID while showing it).
        if self.state.mode != Mode::Normal {
            self.on_prompt_key(key);
            return;
        }
        if self.state.provider_setup.is_some() {
            // The setup screen closes, or takes a command such as `:login`.
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    self.state.provider_setup = None;
                }
                KeyCode::Char(':') => self.open_command_line(),
                _ => {}
            }
            return;
        }
        self.on_normal_key(key);
    }

    fn open_command_line(&mut self) {
        self.keymap.reset();
        self.state.status = None;
        self.state.command_line.clear();
        self.state.mode = Mode::Command;
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
            KeyCode::Char(':') => self.open_command_line(),
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
            Mode::Credential(_) => &mut self.state.credential_line,
            Mode::Normal | Mode::Command => &mut self.state.command_line,
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                line.clear();
                self.state.mode = Mode::Normal;
                if let Mode::Credential(_) = mode {
                    self.state.info("Sign-in cancelled");
                }
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
                // Sign-in details stay out of the prompt history.
                let text = match mode {
                    Mode::Credential(_) => line.take(),
                    _ => line.submit(),
                };
                self.state.mode = Mode::Normal;
                let query = text.trim();
                match mode {
                    Mode::Credential(field) => self.submit_credential(field, &text),
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

fn provider_hints(keymap: &Keymap) -> Vec<Hint> {
    [
        ("focus next", "next"),
        ("play", "choose"),
        ("view music", "music"),
    ]
    .iter()
    .filter_map(|&(cmd, label)| keymap.key_for(cmd).map(|key| Hint { key, label }))
    .collect()
}

/// The OS keychain; tests get an in-memory store so they never touch it.
fn default_secret_store() -> Arc<dyn SecretStore> {
    #[cfg(test)]
    return Arc::new(crate::auth::MemoryStore::default());
    #[cfg(not(test))]
    Arc::new(crate::auth::Keychain)
}

/// The demo library, for UI tests.
#[cfg(test)]
pub(crate) fn demo_library() -> Vec<crate::provider::Playlist> {
    demo::library()
}

impl App {
    /// Switches to the provider chosen last time (from the config), picking
    /// up its saved sign-in.
    fn resume_provider(&mut self) {
        if let Some(kind) = self.startup_provider.take()
            && let Some(provider) = kind.connect()
        {
            self.set_provider(Some(provider));
            self.restore_sign_in();
        }
    }

    /// How to search: `press /`, or the command if unbound.
    pub(super) fn search_hint(&self) -> String {
        match self.keymap.key_for("search") {
            Some(key) => format!("press {key}"),
            None => "run :search".to_string(),
        }
    }
}

/// How to get to the Providers tab: `press 3`, or the command if unbound.
pub(crate) fn providers_hint(state: &AppState) -> String {
    match &state.providers_key {
        Some(key) => format!("press {key}"),
        None => "run :providers".to_string(),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
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

/// Sends `AppEvent::Frame` at ~30fps while `on` is true, and sleeps (no
/// wakeups at all) while it's false.
fn spawn_animator(tx: mpsc::UnboundedSender<AppEvent>, mut on: watch::Receiver<bool>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(FRAME);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if !*on.borrow_and_update() {
                if on.changed().await.is_err() {
                    break;
                }
                continue;
            }
            tokio::select! {
                _ = interval.tick() => {
                    if tx.send(AppEvent::Frame).is_err() {
                        break;
                    }
                }
                changed = on.changed() => {
                    if changed.is_err() {
                        break;
                    }
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
