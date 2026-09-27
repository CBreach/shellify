//! Provider-agnostic music types. The `Provider` trait itself lands with the
//! YouTube Music integration (roadmap step 3).

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// Length from metadata; zero when unknown (the player reports the real one).
    pub duration: Duration,
    /// URL or file the player can open directly. `None` means the app resolves
    /// one from the track (see `app::demo::playback_source`).
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
}
