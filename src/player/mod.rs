//! Audio playback backends. The app talks to a [`Player`] and receives
//! [`PlayerEvent`]s; `MpvPlayer` is the only backend for now (librespot
//! joins it with Spotify).

// Not wired into the app yet; removed once the app drives the player.
#![allow(dead_code, unused_imports)]

mod ipc;
pub mod mpv;

pub use mpv::{MpvOptions, MpvPlayer};

use std::time::Duration;

use async_trait::async_trait;

/// Identifies one `load` call, so late events from a previous track (e.g. its
/// end-of-file arriving just after the user skipped) can be told apart.
pub type LoadId = u64;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// The source for this load was opened and playback started.
    Loaded(LoadId),
    /// The track for this load stopped by itself.
    Ended {
        load: LoadId,
        reason: EndReason,
    },
    Position(Duration),
    Duration(Duration),
    Paused(bool),
    /// The backend died; no further events will arrive.
    Exited(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum EndReason {
    /// Played to the end.
    Eof,
    /// Could not be played (bad URL, yt-dlp failure, network error...).
    Error(String),
}

#[async_trait]
pub trait Player: Send {
    /// Replaces whatever is playing with `source` (URL or file path) and
    /// unpauses. Returns the id that later events for this load carry.
    fn load(&mut self, source: &str) -> LoadId;
    fn set_pause(&mut self, paused: bool);
    fn seek(&mut self, position: Duration);
    /// Volume in percent, 0-100.
    fn set_volume(&mut self, volume: u8);
    fn stop(&mut self);
    /// Stops the backend and cleans up after it.
    async fn shutdown(&mut self);
}
