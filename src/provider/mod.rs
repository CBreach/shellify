//! Provider-agnostic music types and the `Provider` trait that each
//! streaming service implements.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

mod ytmusic;

pub use ytmusic::YouTubeMusic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// Length from metadata; zero when unknown (the player reports the real one).
    pub duration: Duration,
    pub source: Source,
}

/// Where a track's audio comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A demo track with no real source: it plays the top YouTube search
    /// result for "artist title" (see `app::demo::playback_source`).
    Demo,
    /// A track from a streaming service; `Track::id` is that service's id,
    /// and the service's provider resolves it.
    Service(ProviderKind),
    /// A URL or local file the player opens directly (`:open`).
    Direct(String),
}

#[derive(Debug, Clone)]
pub struct Playlist {
    /// The provider's id for it, passed back to `Provider::playlist_tracks`.
    pub id: String,
    pub name: String,
    /// `None` until fetched: providers list playlists without their tracks,
    /// which load when the playlist is opened.
    pub tracks: Option<Vec<Track>>,
}

/// What the player should open for a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackSource {
    /// A URL or file for mpv (with yt-dlp for YouTube pages).
    Url(String),
}

/// A streaming service: where the library, search results and playable
/// tracks come from. The app calls the async methods from spawned tasks and
/// gets the results back as events, so they may take as long as they need.
#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    /// The user's playlists, liked songs first. Tracks may be left out and
    /// fetched with `playlist_tracks` when a playlist is opened.
    async fn library(&self) -> Result<Vec<Playlist>>;

    async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>>;

    async fn search(&self, query: &str) -> Result<Vec<Track>>;

    /// What to play for one of this provider's tracks. Called on the event
    /// loop, so it must not block: slow work such as extracting a stream URL
    /// belongs to the player (mpv hands YouTube pages to yt-dlp).
    fn resolve_playback(&self, track: &Track) -> Result<PlaybackSource>;
}

/// The streaming services Shellify plans to support. The Providers tab lists
/// these; sign-in and playback land one provider at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    YouTubeMusic,
    Spotify,
    AppleMusic,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 3] = [Self::YouTubeMusic, Self::Spotify, Self::AppleMusic];

    pub fn name(self) -> &'static str {
        match self {
            Self::YouTubeMusic => "YouTube Music",
            Self::Spotify => "Spotify",
            Self::AppleMusic => "Apple Music",
        }
    }

    /// The built-in theme in the provider's colors (see `ui::theme`).
    pub fn theme(self) -> &'static str {
        match self {
            Self::YouTubeMusic => "youtube-music",
            Self::Spotify => "spotify",
            Self::AppleMusic => "apple-music",
        }
    }

    /// Its name in the config file and `:provider`: the same as its theme's.
    pub fn id(self) -> &'static str {
        self.theme()
    }

    /// Parses an id, or a short alias such as `ytmusic`.
    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "youtube-music" | "ytmusic" | "youtube" => Some(Self::YouTubeMusic),
            "spotify" => Some(Self::Spotify),
            "apple-music" | "apple" => Some(Self::AppleMusic),
            _ => None,
        }
    }

    /// A provider for it, or `None` while it isn't built yet.
    pub fn connect(self) -> Option<Arc<dyn Provider>> {
        match self {
            Self::YouTubeMusic => Some(Arc::new(YouTubeMusic::default())),
            Self::Spotify | Self::AppleMusic => None,
        }
    }

    /// Whether Shellify can use it yet.
    pub fn available(self) -> bool {
        self.connect().is_some()
    }

    /// Where it is on the roadmap.
    pub fn status(self) -> &'static str {
        match self {
            Self::YouTubeMusic => "available",
            Self::Spotify => "planned",
            Self::AppleMusic => "later",
        }
    }

    /// What signing in will need.
    pub fn requirement(self) -> &'static str {
        match self {
            Self::YouTubeMusic => "Sign in with :login to see your own playlists.",
            Self::Spotify => "Playback will need a Spotify Premium account.",
            Self::AppleMusic => "Playback will need an Apple Music subscription.",
        }
    }

    /// The provider whose theme this is, if any.
    pub fn for_theme(theme: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.theme() == theme)
    }
}
