//! Placeholder library so the TUI has something to show. Replaced by the
//! YouTube Music provider in roadmap step 3.

use std::time::Duration;

use crate::provider::{Playlist, Track};

fn track(id: &str, title: &str, artist: &str, secs: u64) -> Track {
    Track {
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
        duration: Duration::from_secs(secs),
        source: None,
    }
}

/// What the player should open for `track`. Demo tracks have no real source,
/// so they play the first YouTube search result for "artist title" via
/// yt-dlp. Step 3 replaces this with the provider's `resolve_playback`.
pub fn playback_source(track: &Track) -> String {
    track
        .source
        .clone()
        .unwrap_or_else(|| format!("ytdl://ytsearch1:{} {}", track.artist, track.title))
}

pub fn library() -> Vec<Playlist> {
    let liked = vec![
        track("d1", "Harder, Better, Faster, Stronger", "Daft Punk", 224),
        track("d2", "Midnight City", "M83", 243),
        track("d3", "Nightcall", "Kavinsky", 258),
        track("d4", "Resonance", "HOME", 212),
        track("d5", "Tadow", "Masego & FKJ", 301),
    ];
    let focus = vec![
        track("f1", "Weightless", "Marconi Union", 480),
        track("f2", "Intro", "The xx", 128),
        track("f3", "Svefn-g-englar", "Sigur Rós", 604),
        track("f4", "Teardrop", "Massive Attack", 330),
    ];
    let chill = vec![
        track("c1", "Sunset Lover", "Petit Biscuit", 237),
        track("c2", "Coffee", "beabadoobee", 196),
        track("d4", "Resonance", "HOME", 212),
        track("c4", "Electric Feel", "MGMT", 229),
    ];
    vec![
        Playlist {
            name: "Liked Songs".into(),
            tracks: liked,
        },
        Playlist {
            name: "Deep Focus".into(),
            tracks: focus,
        },
        Playlist {
            name: "Chill Mix".into(),
            tracks: chill,
        },
    ]
}

/// Case-insensitive search over titles and artists, without duplicates.
pub fn search(library: &[Playlist], query: &str) -> Vec<Track> {
    let q = query.to_lowercase();
    let mut results: Vec<Track> = Vec::new();
    for t in library.iter().flat_map(|p| &p.tracks) {
        let hit = t.title.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q);
        if hit && !results.iter().any(|r| r.id == t.id) {
            results.push(t.clone());
        }
    }
    results
}
