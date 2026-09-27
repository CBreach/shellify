//! Provider-agnostic music types. The `Provider` trait itself lands with the
//! YouTube Music integration (roadmap step 3).

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
}
