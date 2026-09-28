//! YouTube Music, signed out: search through YouTube Music's own (unofficial)
//! API via `ytmapi-rs`, and playback through yt-dlp in mpv. No account and no
//! cookies are involved. Your library needs sign-in, which comes later.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::sync::OnceCell;
use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::common::YoutubeID;
use ytmapi_rs::parse::SearchResultSong;

use super::{PlaybackSource, Playlist, Provider, ProviderKind, Source, Track};

#[derive(Default)]
pub struct YouTubeMusic {
    /// Created on first use: it fetches a visitor token from YouTube Music.
    client: OnceCell<YtMusic<NoAuthToken>>,
}

impl YouTubeMusic {
    async fn client(&self) -> Result<&YtMusic<NoAuthToken>> {
        self.client
            .get_or_try_init(YtMusic::new_unauthenticated)
            .await
            .context("couldn't reach YouTube Music")
    }
}

#[async_trait]
impl Provider for YouTubeMusic {
    fn kind(&self) -> ProviderKind {
        ProviderKind::YouTubeMusic
    }

    /// Signed out, there's no library to show.
    async fn library(&self) -> Result<Vec<Playlist>> {
        Ok(Vec::new())
    }

    async fn playlist_tracks(&self, _playlist_id: &str) -> Result<Vec<Track>> {
        bail!("sign in to YouTube Music to see your playlists")
    }

    async fn search(&self, query: &str) -> Result<Vec<Track>> {
        let songs = self
            .client()
            .await?
            .search_songs(query)
            .await
            .context("YouTube Music search failed")?;
        Ok(songs.iter().map(song_track).collect())
    }

    fn resolve_playback(&self, track: &Track) -> Result<PlaybackSource> {
        Ok(PlaybackSource::Url(watch_url(&track.id)))
    }
}

/// The page yt-dlp (through mpv) plays for a video id.
fn watch_url(video_id: &str) -> String {
    format!("https://music.youtube.com/watch?v={video_id}")
}

fn song_track(song: &SearchResultSong) -> Track {
    Track {
        id: song.video_id.get_raw().to_string(),
        title: song.title.clone(),
        artist: song.artist.clone(),
        duration: parse_duration(&song.duration).unwrap_or_default(),
        source: Source::Service(ProviderKind::YouTubeMusic),
    }
}

/// `3:45` or `1:02:03`.
fn parse_duration(text: &str) -> Option<Duration> {
    let mut secs = 0u64;
    for part in text.trim().split(':') {
        secs = secs.checked_mul(60)? + part.parse::<u64>().ok()?;
    }
    Some(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("3:45"), Some(Duration::from_secs(225)));
        assert_eq!(parse_duration("1:02:03"), Some(Duration::from_secs(3723)));
        assert_eq!(parse_duration("0:07"), Some(Duration::from_secs(7)));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("LIVE"), None);
    }

    #[test]
    fn plays_the_youtube_music_page() {
        let track = Track {
            id: "abcDEF12345".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            duration: Duration::ZERO,
            source: Source::Service(ProviderKind::YouTubeMusic),
        };
        assert_eq!(
            YouTubeMusic::default().resolve_playback(&track).unwrap(),
            PlaybackSource::Url("https://music.youtube.com/watch?v=abcDEF12345".into())
        );
    }

    /// Talks to the real YouTube Music: `cargo test -- --ignored ytmusic`.
    #[tokio::test]
    #[ignore = "needs the network"]
    async fn ytmusic_live_search_finds_songs() {
        let results = YouTubeMusic::default()
            .search("daft punk harder better faster stronger")
            .await
            .unwrap();
        assert!(!results.is_empty());
        let first = &results[0];
        eprintln!("{first:?}");
        assert_eq!(first.id.len(), 11, "a YouTube video id");
        assert!(!first.title.is_empty() && !first.artist.is_empty());
        assert!(first.duration > Duration::from_secs(60));
    }
}
