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

    /// Where it is on the roadmap.
    pub fn status(self) -> &'static str {
        match self {
            Self::YouTubeMusic => "coming next",
            Self::Spotify => "planned",
            Self::AppleMusic => "later",
        }
    }

    /// What signing in will need.
    pub fn requirement(self) -> &'static str {
        match self {
            Self::YouTubeMusic => "You'll sign in with your Google account.",
            Self::Spotify => "Playback will need a Spotify Premium account.",
            Self::AppleMusic => "Playback will need an Apple Music subscription.",
        }
    }

    /// The provider whose theme this is, if any.
    pub fn for_theme(theme: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.theme() == theme)
    }
}
