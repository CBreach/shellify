//! [`Player`] backed by an `mpv` child process controlled over its JSON IPC
//! socket. mpv's built-in yt-dlp hook resolves YouTube URLs and searches.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::ipc::{
    Command, Incoming, METER_FILTER, METER_LABEL, METER_PROPERTY, OBSERVE_DURATION, OBSERVE_LEVELS,
    OBSERVE_PAUSE, OBSERVE_TIME_POS,
};
use super::{EndReason, LoadId, Player, PlayerEvent};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const QUIT_TIMEOUT: Duration = Duration::from_secs(2);
/// Minimum change in `time-pos` worth reporting; mpv sends many more updates.
const POSITION_STEP: f64 = 0.25;

pub struct MpvOptions {
    pub binary: String,
    /// Initial volume in percent.
    pub volume: u8,
    /// Extra command-line arguments (tests use `--ao=null`).
    pub extra_args: Vec<String>,
}

impl Default for MpvOptions {
    fn default() -> Self {
        Self {
            binary: "mpv".into(),
            volume: 70,
            extra_args: Vec::new(),
        }
    }
}

pub struct MpvPlayer {
    commands: mpsc::UnboundedSender<(Command, u64)>,
    next_load: LoadId,
    child: Option<Child>,
    socket_path: Option<PathBuf>,
    shutting_down: Arc<AtomicBool>,
    tasks: Vec<JoinHandle<()>>,
    metering: bool,
}

impl MpvPlayer {
    /// Starts mpv and connects to its IPC socket. Events are sent to `events`.
    pub async fn spawn(
        options: MpvOptions,
        events: mpsc::UnboundedSender<PlayerEvent>,
    ) -> Result<Self> {
        static INSTANCE: AtomicU32 = AtomicU32::new(0);
        let socket_path = std::env::temp_dir().join(format!(
            "shellify-mpv-{}-{}.sock",
            std::process::id(),
            INSTANCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&socket_path);

        let mut child = ProcessCommand::new(&options.binary)
            .args([
                "--idle=yes",
                "--no-video",
                "--no-terminal",
                "--force-window=no",
                "--keep-open=no",
                "--audio-display=no",
                "--ytdl-format=bestaudio/best",
            ])
            .arg(format!("--volume={}", options.volume))
            .arg(format!("--input-ipc-server={}", socket_path.display()))
            .args(&options.extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => anyhow::anyhow!(
                    "{} not found; install mpv to play music (e.g. `brew install mpv`)",
                    options.binary
                ),
                _ => anyhow::Error::new(e).context(format!("starting {}", options.binary)),
            })?;

        let stream = connect(&mut child, &socket_path).await?;
        let mut player = Self::attach(stream, events);
        player.child = Some(child);
        player.socket_path = Some(socket_path);
        Ok(player)
    }

    /// Wires up IO tasks for an already-connected socket.
    fn attach(stream: UnixStream, events: mpsc::UnboundedSender<PlayerEvent>) -> Self {
        let (read_half, mut write_half) = stream.into_split();
        let (commands, mut command_rx) = mpsc::unbounded_channel::<(Command, u64)>();
        let shutting_down = Arc::new(AtomicBool::new(false));

        let writer = tokio::spawn(async move {
            while let Some((command, request_id)) = command_rx.recv().await {
                if let Err(e) = write_half
                    .write_all(command.encode(request_id).as_bytes())
                    .await
                {
                    tracing::warn!("mpv ipc write failed: {e}");
                    break;
                }
            }
        });

        let reader_shutdown = shutting_down.clone();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(read_half).lines();
            let mut tracker = Tracker::default();
            // Ends when mpv closes the socket (quit or crash).
            while let Ok(Some(line)) = lines.next_line().await {
                match Incoming::parse(&line) {
                    Ok(message) => {
                        for event in tracker.on_message(message) {
                            if events.send(event).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) => tracing::warn!("unparseable mpv message {line:?}: {e}"),
                }
            }
            if !reader_shutdown.load(Ordering::SeqCst) {
                let _ = events.send(PlayerEvent::Exited("mpv exited unexpectedly".into()));
            }
        });

        for (id, name) in [
            (OBSERVE_TIME_POS, "time-pos"),
            (OBSERVE_DURATION, "duration"),
            (OBSERVE_PAUSE, "pause"),
            // Reports nothing until the meter filter is added.
            (OBSERVE_LEVELS, METER_PROPERTY),
        ] {
            let _ = commands.send((Command::Observe(id, name), 0));
        }

        Self {
            commands,
            next_load: 0,
            child: None,
            socket_path: None,
            shutting_down,
            tasks: vec![writer, reader],
            metering: false,
        }
    }

    fn send(&self, command: Command) {
        // A closed channel means mpv is gone; the reader reports that as Exited.
        let _ = self.commands.send((command, 0));
    }
}

/// Waits for mpv to create its socket, failing early if mpv exits.
async fn connect(child: &mut Child, socket_path: &PathBuf) -> Result<UnixStream> {
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("mpv exited during startup ({status})");
        }
        match UnixStream::connect(socket_path).await {
            Ok(stream) => return Ok(stream),
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("connecting to mpv at {}", socket_path.display()));
            }
        }
    }
}

#[async_trait]
impl Player for MpvPlayer {
    fn load(&mut self, source: &str) -> LoadId {
        self.next_load += 1;
        // mpv keeps the pause state across files; a new track should play.
        self.send(Command::SetPause(false));
        let _ = self
            .commands
            .send((Command::LoadFile(source.to_string()), self.next_load));
        self.next_load
    }

    fn set_pause(&mut self, paused: bool) {
        self.send(Command::SetPause(paused));
    }

    fn seek(&mut self, position: Duration) {
        self.send(Command::SeekAbsolute(position.as_secs_f64()));
    }

    fn set_volume(&mut self, volume: u8) {
        self.send(Command::SetVolume(volume.min(100)));
    }

    fn stop(&mut self) {
        self.send(Command::Stop);
    }

    fn set_metering(&mut self, on: bool) {
        if on == self.metering {
            return;
        }
        self.metering = on;
        if on {
            self.send(Command::AddAudioFilter(METER_FILTER));
        } else {
            self.send(Command::RemoveAudioFilter(METER_LABEL));
        }
    }

    async fn shutdown(&mut self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.send(Command::Quit);
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(QUIT_TIMEOUT, child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    tracing::warn!("mpv did not quit in time; killing it");
                    let _ = child.kill().await;
                }
            }
        }
        for task in self.tasks.drain(..) {
            task.abort();
        }
        if let Some(path) = self.socket_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        // The child is killed by `kill_on_drop`; mpv leaves its socket behind.
        if let Some(path) = &self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Turns raw mpv messages into [`PlayerEvent`]s, attributing file events to
/// the `load` call that caused them.
#[derive(Default)]
struct Tracker {
    /// mpv playlist entry id -> load that created it.
    entry_loads: HashMap<u64, LoadId>,
    /// Load of the entry mpv is currently playing.
    current: Option<LoadId>,
    last_position: Option<f64>,
    last_duration: Option<f64>,
}

impl Tracker {
    fn on_message(&mut self, message: Incoming) -> Vec<PlayerEvent> {
        match message {
            Incoming::Response {
                request_id: 0,
                error: Some(e),
                ..
            } => {
                tracing::debug!("mpv command failed: {e}");
                vec![]
            }
            Incoming::Response { request_id: 0, .. } => vec![],
            Incoming::Response {
                request_id: load,
                error: Some(e),
                ..
            } => vec![PlayerEvent::Ended {
                load,
                reason: EndReason::Error(e),
            }],
            Incoming::Response {
                request_id: load,
                data,
                ..
            } => {
                if let Some(entry) = data.get("playlist_entry_id").and_then(Value::as_u64) {
                    self.entry_loads.insert(entry, load);
                }
                vec![]
            }
            Incoming::StartFile { entry } => {
                self.current = self.entry_loads.get(&entry).copied();
                self.last_position = None;
                self.last_duration = None;
                vec![]
            }
            Incoming::FileLoaded => self.current.map(PlayerEvent::Loaded).into_iter().collect(),
            Incoming::EndFile {
                entry,
                reason,
                file_error,
                insert_id,
                insert_count,
            } => {
                let Some(load) = self.entry_loads.remove(&entry) else {
                    return vec![];
                };
                if self.current == Some(load) && reason != "redirect" {
                    self.current = None;
                }
                match reason.as_str() {
                    "eof" => vec![PlayerEvent::Ended {
                        load,
                        reason: EndReason::Eof,
                    }],
                    "error" => vec![PlayerEvent::Ended {
                        load,
                        reason: EndReason::Error(
                            file_error.unwrap_or_else(|| "playback failed".into()),
                        ),
                    }],
                    // The source expanded into new entries (e.g. a ytsearch
                    // result); they belong to the same load.
                    "redirect" => {
                        if let Some(first) = insert_id {
                            for entry in first..first + insert_count {
                                self.entry_loads.insert(entry, load);
                            }
                        }
                        vec![]
                    }
                    // "stop" (replaced or stopped by us) and "quit".
                    _ => vec![],
                }
            }
            Incoming::PropertyChange { id, data } => self.on_property(id, data),
            Incoming::Other => vec![],
        }
    }

    fn on_property(&mut self, id: u64, data: Value) -> Vec<PlayerEvent> {
        match id {
            OBSERVE_TIME_POS => {
                let Some(pos) = data.as_f64() else {
                    return vec![];
                };
                let changed = self
                    .last_position
                    .is_none_or(|last| pos < last || pos - last >= POSITION_STEP);
                if !changed {
                    return vec![];
                }
                self.last_position = Some(pos);
                vec![PlayerEvent::Position(Duration::from_secs_f64(pos.max(0.0)))]
            }
            OBSERVE_DURATION => {
                let Some(dur) = data.as_f64() else {
                    return vec![];
                };
                // Streams refine their duration continuously; report whole seconds.
                if self
                    .last_duration
                    .is_some_and(|last| (dur - last).abs() < 1.0)
                {
                    return vec![];
                }
                self.last_duration = Some(dur);
                vec![PlayerEvent::Duration(Duration::from_secs_f64(dur.max(0.0)))]
            }
            OBSERVE_PAUSE => data
                .as_bool()
                .map(PlayerEvent::Paused)
                .into_iter()
                .collect(),
            OBSERVE_LEVELS => parse_levels(&data).into_iter().collect(),
            _ => vec![],
        }
    }
}

/// `af-metadata` from the meter: string values like `"-23.5"` or `"-inf"`.
fn parse_levels(data: &Value) -> Option<PlayerEvent> {
    let db = |key: &str| data.get(key)?.as_str()?.trim().parse::<f32>().ok();
    Some(PlayerEvent::Levels {
        rms_db: db("lavfi.astats.Overall.RMS_level")?,
        peak_db: db("lavfi.astats.Overall.Peak_level")?,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_meter_levels() {
        let data = serde_json::json!({
            "lavfi.astats.Overall.RMS_level": "-23.5",
            "lavfi.astats.Overall.Peak_level": "-inf",
        });
        match parse_levels(&data) {
            Some(PlayerEvent::Levels { rms_db, peak_db }) => {
                assert_eq!(rms_db, -23.5);
                assert!(peak_db.is_infinite() && peak_db < 0.0);
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_levels(&serde_json::Value::Null).is_none());
        assert!(
            parse_levels(&serde_json::json!({"lavfi.astats.Overall.RMS_level": "x"})).is_none()
        );
    }

    use serde_json::json;
    use tokio::io::AsyncBufReadExt;

    use super::*;

    fn msg(line: &str) -> Incoming {
        Incoming::parse(line).unwrap()
    }

    fn load_response(load: u64, entry: u64) -> Incoming {
        Incoming::Response {
            request_id: load,
            error: None,
            data: json!({ "playlist_entry_id": entry }),
        }
    }

    #[test]
    fn tracker_reports_loaded_and_eof_for_the_right_load() {
        let mut t = Tracker::default();
        assert!(t.on_message(load_response(1, 1)).is_empty());
        t.on_message(msg(r#"{"event":"start-file","playlist_entry_id":1}"#));
        assert_eq!(
            t.on_message(Incoming::FileLoaded),
            vec![PlayerEvent::Loaded(1)]
        );
        assert_eq!(
            t.on_message(msg(
                r#"{"event":"end-file","reason":"eof","playlist_entry_id":1}"#
            )),
            vec![PlayerEvent::Ended {
                load: 1,
                reason: EndReason::Eof
            }]
        );
    }

    #[test]
    fn tracker_follows_ytsearch_redirects() {
        let mut t = Tracker::default();
        t.on_message(load_response(4, 1));
        t.on_message(msg(r#"{"event":"start-file","playlist_entry_id":1}"#));
        let redirect = r#"{"event":"end-file","reason":"redirect","playlist_entry_id":1,"playlist_insert_id":2,"playlist_insert_num_entries":1}"#;
        assert!(t.on_message(msg(redirect)).is_empty());
        t.on_message(msg(r#"{"event":"start-file","playlist_entry_id":2}"#));
        assert_eq!(
            t.on_message(Incoming::FileLoaded),
            vec![PlayerEvent::Loaded(4)]
        );
        assert_eq!(
            t.on_message(msg(
                r#"{"event":"end-file","reason":"eof","playlist_entry_id":2}"#
            )),
            vec![PlayerEvent::Ended {
                load: 4,
                reason: EndReason::Eof
            }]
        );
    }

    #[test]
    fn tracker_ignores_replaced_and_unknown_entries() {
        let mut t = Tracker::default();
        t.on_message(load_response(1, 1));
        t.on_message(msg(r#"{"event":"start-file","playlist_entry_id":1}"#));
        t.on_message(load_response(2, 2));
        // Replacing entry 1 ends it with reason "stop": not an EOF.
        assert!(
            t.on_message(msg(
                r#"{"event":"end-file","reason":"stop","playlist_entry_id":1}"#
            ))
            .is_empty()
        );
        assert!(
            t.on_message(msg(
                r#"{"event":"end-file","reason":"eof","playlist_entry_id":99}"#
            ))
            .is_empty()
        );
    }

    #[test]
    fn tracker_reports_errors() {
        let mut t = Tracker::default();
        let failed = t.on_message(Incoming::Response {
            request_id: 3,
            error: Some("invalid parameter".into()),
            data: Value::Null,
        });
        assert_eq!(
            failed,
            vec![PlayerEvent::Ended {
                load: 3,
                reason: EndReason::Error("invalid parameter".into())
            }]
        );

        t.on_message(load_response(5, 7));
        t.on_message(msg(r#"{"event":"start-file","playlist_entry_id":7}"#));
        assert_eq!(
            t.on_message(msg(
                r#"{"event":"end-file","reason":"error","playlist_entry_id":7,"file_error":"loading failed"}"#
            )),
            vec![PlayerEvent::Ended { load: 5, reason: EndReason::Error("loading failed".into()) }]
        );
    }

    #[test]
    fn tracker_throttles_position_and_duration() {
        let mut t = Tracker::default();
        let pos = |t: &mut Tracker, p: f64| t.on_property(OBSERVE_TIME_POS, json!(p));
        assert_eq!(
            pos(&mut t, 1.0),
            vec![PlayerEvent::Position(Duration::from_secs(1))]
        );
        assert!(pos(&mut t, 1.1).is_empty());
        assert_eq!(pos(&mut t, 1.3).len(), 1);
        // Seeking backwards is always reported.
        assert_eq!(pos(&mut t, 0.5).len(), 1);
        assert!(t.on_property(OBSERVE_TIME_POS, Value::Null).is_empty());

        assert_eq!(t.on_property(OBSERVE_DURATION, json!(200.4)).len(), 1);
        assert!(t.on_property(OBSERVE_DURATION, json!(200.9)).is_empty());
        assert_eq!(
            t.on_property(OBSERVE_PAUSE, json!(true)),
            vec![PlayerEvent::Paused(true)]
        );
    }

    /// Drives `MpvPlayer` against a fake mpv on the other end of a socket pair.
    #[tokio::test]
    async fn player_speaks_ipc_over_the_socket() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let mut player = MpvPlayer::attach(ours, events_tx);

        let (read, mut write) = theirs.into_split();
        let mut lines = BufReader::new(read).lines();
        let mut next_command = async || -> Value {
            let line = lines.next_line().await.unwrap().unwrap();
            serde_json::from_str(&line).unwrap()
        };

        for name in ["time-pos", "duration", "pause", METER_PROPERTY] {
            assert_eq!(next_command().await["command"][2], json!(name));
        }

        let load = player.load("ytdl://ytsearch1:artist title");
        assert_eq!(
            next_command().await["command"],
            json!(["set_property", "pause", false])
        );
        let loadfile = next_command().await;
        assert_eq!(
            loadfile["command"],
            json!(["loadfile", "ytdl://ytsearch1:artist title", "replace"])
        );
        assert_eq!(loadfile["request_id"], json!(load));

        let reply = format!(
            concat!(
                r#"{{"data":{{"playlist_entry_id":1}},"request_id":{},"error":"success"}}"#,
                "\n",
                r#"{{"event":"start-file","playlist_entry_id":1}}"#,
                "\n",
                r#"{{"event":"file-loaded"}}"#,
                "\n",
                r#"{{"event":"property-change","id":1,"name":"time-pos","data":2.0}}"#,
                "\n",
                r#"{{"event":"end-file","reason":"eof","playlist_entry_id":1}}"#,
                "\n",
            ),
            load
        );
        write.write_all(reply.as_bytes()).await.unwrap();

        assert_eq!(events.recv().await, Some(PlayerEvent::Loaded(load)));
        assert_eq!(
            events.recv().await,
            Some(PlayerEvent::Position(Duration::from_secs(2)))
        );
        assert_eq!(
            events.recv().await,
            Some(PlayerEvent::Ended {
                load,
                reason: EndReason::Eof
            })
        );

        player.seek(Duration::from_secs(90));
        assert_eq!(
            next_command().await["command"],
            json!(["seek", 90.0, "absolute"])
        );

        // mpv going away unexpectedly is reported.
        drop(write);
        drop(lines);
        assert!(matches!(events.recv().await, Some(PlayerEvent::Exited(_))));
    }

    /// Waits (up to 15s) for an event matching `pred`, skipping others.
    async fn wait_for(
        events: &mut mpsc::UnboundedReceiver<PlayerEvent>,
        pred: impl Fn(&PlayerEvent) -> bool,
    ) -> PlayerEvent {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let event = events.recv().await.expect("player event channel closed");
                if pred(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("timed out waiting for player event")
    }

    /// Plays a generated tone through a real mpv (no network, no audio device).
    #[tokio::test]
    #[ignore = "needs mpv installed; run with `cargo test -- --ignored`"]
    async fn real_mpv_plays_to_the_end_and_cleans_up() {
        let (tx, mut events) = mpsc::unbounded_channel();
        let options = MpvOptions {
            extra_args: vec!["--ao=null".into()],
            ..Default::default()
        };
        let mut player = MpvPlayer::spawn(options, tx).await.unwrap();
        let socket = player.socket_path.clone().unwrap();
        let pid = player.child.as_ref().unwrap().id().unwrap();
        assert!(socket.exists());

        let tone = player.load("av://lavfi:sine=frequency=440:duration=1");
        wait_for(&mut events, |e| *e == PlayerEvent::Loaded(tone)).await;
        wait_for(&mut events, |e| {
            *e == PlayerEvent::Ended {
                load: tone,
                reason: EndReason::Eof,
            }
        })
        .await;

        let missing = player.load("/nonexistent/shellify-test.mp3");
        wait_for(&mut events, |e| {
            matches!(e, PlayerEvent::Ended { load, reason: EndReason::Error(_) } if *load == missing)
        })
        .await;

        player.shutdown().await;
        assert!(!socket.exists(), "socket file left behind");
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "mpv process still running");
    }

    #[tokio::test]
    #[ignore = "needs mpv installed; run with `cargo test -- --ignored`"]
    async fn real_mpv_meter_reports_loudness_that_tracks_the_audio() {
        let (tx, mut events) = mpsc::unbounded_channel();
        let options = MpvOptions {
            extra_args: vec!["--ao=null".into()],
            ..Default::default()
        };
        let mut player = MpvPlayer::spawn(options, tx).await.unwrap();
        player.set_metering(true);
        // A tone that fades in over two seconds.
        player.load("av://lavfi:sine=frequency=440:duration=2,volume=volume='t/2':eval=frame");

        let mut readings = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        while readings.len() < 12 {
            let event = tokio::time::timeout_at(deadline, events.recv())
                .await
                .expect("no loudness readings from mpv")
                .unwrap();
            if let PlayerEvent::Levels { rms_db, .. } = event {
                readings.push(rms_db);
            }
        }
        let early: f32 = readings[1..4].iter().sum::<f32>() / 3.0;
        let late: f32 = readings[readings.len() - 3..].iter().sum::<f32>() / 3.0;
        assert!(late > early + 3.0, "fade-in not reflected: {readings:?}");

        // Switching the meter off stops the readings.
        player.set_metering(false);
        tokio::time::sleep(Duration::from_millis(300)).await;
        while events.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut more = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(event, PlayerEvent::Levels { .. }) {
                more += 1;
            }
        }
        assert_eq!(more, 0, "meter still reporting after being removed");
        player.shutdown().await;
    }
}
