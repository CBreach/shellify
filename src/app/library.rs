//! Getting the library, playlist tracks and search results: from the
//! provider in spawned tasks when one has been added, or from the demo
//! library when not.
//!
//! Every provider call gets a `RequestId`, and its reply comes back through
//! the main event channel as `AppEvent::Provider`. A newer request for the
//! same place (the Library pane, or the Tracks pane) replaces the older one,
//! so a slow reply can't overwrite what the user asked for since.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use super::{App, AppEvent, demo};
use crate::provider::{PlaybackSource, Playlist, Provider, Source, Track};

pub type RequestId = u64;

#[derive(Debug)]
pub struct ProviderEvent {
    request: RequestId,
    reply: Reply,
}

#[derive(Debug)]
enum Reply {
    Library(Result<Vec<Playlist>>),
    Tracks(Result<Vec<Track>>),
}

/// What an in-flight request is for.
#[derive(Debug)]
enum Request {
    Library,
    /// Show a playlist in the Tracks pane.
    Open {
        id: String,
    },
    /// Add a playlist's tracks to the queue.
    Add {
        id: String,
        name: String,
    },
    /// Show results in the Tracks pane, and with `play`, play them.
    Search {
        query: String,
        play: bool,
    },
}

impl Request {
    /// Requests for the same place on screen; only the newest one counts.
    fn slot(&self) -> Option<u8> {
        match self {
            Request::Library => Some(0),
            Request::Open { .. } | Request::Search { .. } => Some(1),
            Request::Add { .. } => None,
        }
    }
}

/// Provider requests still waiting for a reply.
#[derive(Debug, Default)]
pub(super) struct Requests {
    next: RequestId,
    pending: HashMap<RequestId, Request>,
}

impl App {
    /// Switches to `provider`'s library, or back to the demo library.
    pub(super) fn set_provider(&mut self, provider: Option<Arc<dyn Provider>>) {
        self.requests.pending.clear();
        self.provider = provider;
        let state = &mut self.state;
        state.active_provider = self.provider.as_ref().map(|p| p.kind());
        state.library_loading = false;
        state.tracks_loading = false;
        state.tracks_title.clear();
        state.tracks.clear();
        if self.provider.is_some() {
            state.library.clear();
            state.library_loading = true;
            self.request(Request::Library, |p| async move {
                Reply::Library(p.library().await)
            });
        } else {
            state.library = demo::library();
        }
        self.state.library_state.select(Some(0));
    }

    /// Enter on a playlist: show its tracks, fetching them first if needed.
    pub(super) fn open_playlist(&mut self, index: usize) {
        let Some(playlist) = self.state.library.get(index) else {
            return;
        };
        let (id, name) = (playlist.id.clone(), playlist.name.clone());
        let cached = playlist.tracks.clone();
        self.state
            .show_tracks(name, cached.clone().unwrap_or_default());
        if cached.is_none() {
            self.state.tracks_loading = true;
            self.request(Request::Open { id: id.clone() }, |p| async move {
                Reply::Tracks(p.playlist_tracks(&id).await)
            });
        }
    }

    /// `a` on a playlist: queue all its tracks, fetching them first if needed.
    pub(super) fn add_playlist(&mut self, index: usize) {
        let Some(playlist) = self.state.library.get(index) else {
            return;
        };
        let (id, name) = (playlist.id.clone(), playlist.name.clone());
        match playlist.tracks.clone() {
            Some(tracks) => self.queue_tracks(tracks),
            None => {
                self.state.info(format!("Adding {name} to the queue"));
                self.request(
                    Request::Add {
                        id: id.clone(),
                        name,
                    },
                    |p| async move { Reply::Tracks(p.playlist_tracks(&id).await) },
                );
            }
        }
    }

    /// `/` search, or `:play <query>` with `play` (plays the results).
    pub(super) fn search(&mut self, query: String, play: bool) {
        if self.provider.is_none() {
            let results = demo::search(&self.state.library, &query);
            self.show_results(&query, results, play);
            return;
        }
        self.state
            .show_tracks(format!("Search: {query}"), Vec::new());
        self.state.tracks_loading = true;
        let q = query.clone();
        self.request(Request::Search { query, play }, |p| async move {
            Reply::Tracks(p.search(&q).await)
        });
    }

    fn show_results(&mut self, query: &str, results: Vec<Track>, play: bool) {
        if play {
            if results.is_empty() {
                self.state.error(format!("no results for {query:?}"));
                return;
            }
            self.state.queue.play_list(results.clone(), 0);
            self.state.show_tracks(format!("Search: {query}"), results);
            self.start_current();
        } else {
            self.state
                .info(format!("{} results for {query:?}", results.len()));
            self.state.show_tracks(format!("Search: {query}"), results);
        }
    }

    fn queue_tracks(&mut self, tracks: Vec<Track>) {
        let n = tracks.len();
        for track in tracks {
            self.state.queue.push(track);
        }
        self.state.info(format!("added {n} track(s) to the queue"));
    }

    /// Sends `request` to the provider in a spawned task, replacing any older
    /// request for the same place.
    fn request<F>(&mut self, request: Request, call: impl FnOnce(Arc<dyn Provider>) -> F)
    where
        F: Future<Output = Reply> + Send + 'static,
    {
        let Some(provider) = self.provider.clone() else {
            return;
        };
        let requests = &mut self.requests;
        if let Some(slot) = request.slot() {
            requests.pending.retain(|_, r| r.slot() != Some(slot));
        }
        requests.next += 1;
        let id = requests.next;
        requests.pending.insert(id, request);
        let events = self.events.clone();
        let task = call(provider);
        tokio::spawn(async move {
            let reply = task.await;
            // Fails only when the app is shutting down.
            let _ = events.send(AppEvent::Provider(ProviderEvent { request: id, reply }));
        });
    }

    pub(super) fn on_provider_event(&mut self, event: ProviderEvent) {
        let Some(request) = self.requests.pending.remove(&event.request) else {
            tracing::debug!(request = event.request, "stale provider reply");
            return;
        };
        match (request, event.reply) {
            (Request::Library, Reply::Library(result)) => {
                self.state.library_loading = false;
                match result {
                    Ok(playlists) => {
                        let selected = (!playlists.is_empty()).then_some(0);
                        self.state.library = playlists;
                        self.state.library_state.select(selected);
                    }
                    Err(e) => self
                        .state
                        .error(format!("couldn't load your library: {e:#}")),
                }
            }
            (Request::Open { id }, Reply::Tracks(result)) => {
                self.state.tracks_loading = false;
                match result {
                    Ok(tracks) => {
                        self.cache_tracks(&id, &tracks);
                        self.state.tracks = tracks;
                        self.state.tracks_state.select(Some(0));
                    }
                    Err(e) => self
                        .state
                        .error(format!("couldn't load the playlist: {e:#}")),
                }
            }
            (Request::Add { id, name }, Reply::Tracks(result)) => match result {
                Ok(tracks) => {
                    self.cache_tracks(&id, &tracks);
                    self.queue_tracks(tracks);
                }
                Err(e) => self.state.error(format!("couldn't load {name}: {e:#}")),
            },
            (Request::Search { query, play }, Reply::Tracks(result)) => {
                self.state.tracks_loading = false;
                match result {
                    Ok(results) => self.show_results(&query, results, play),
                    Err(e) => self.state.error(format!("search failed: {e:#}")),
                }
            }
            (request, reply) => {
                tracing::error!(?request, ?reply, "provider reply doesn't match its request");
            }
        }
    }

    /// Keeps a playlist's tracks so opening it again is instant.
    fn cache_tracks(&mut self, playlist_id: &str, tracks: &[Track]) {
        if let Some(playlist) = self.state.library.iter_mut().find(|p| p.id == playlist_id) {
            playlist.tracks = Some(tracks.to_vec());
        }
    }

    /// What the player should open for `track`.
    pub(super) fn playback_source(&self, track: &Track) -> Result<String, String> {
        match &track.source {
            Source::Direct(source) => Ok(source.clone()),
            Source::Demo => Ok(demo::playback_source(track)),
            Source::Service(kind) => match &self.provider {
                Some(provider) if provider.kind() == *kind => provider
                    .resolve_playback(track)
                    .map(|PlaybackSource::Url(url)| url)
                    .map_err(|e| format!("{e:#}")),
                _ => Err(format!("{} isn't set up", kind.name())),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use anyhow::anyhow;
    use async_trait::async_trait;

    use super::*;
    use crate::app::action::{Action, Pane};
    use crate::app::testing::FakePlayer;
    use crate::config::Config;
    use crate::provider::ProviderKind;

    fn track(id: &str, title: &str) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: "Artist".into(),
            duration: Duration::from_secs(180),
            source: Source::Service(ProviderKind::YouTubeMusic),
        }
    }

    /// Answers straight away with fixed data, and records each call.
    #[derive(Default)]
    struct FakeProvider {
        calls: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Provider for FakeProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::YouTubeMusic
        }
        async fn library(&self) -> Result<Vec<Playlist>> {
            self.calls.lock().unwrap().push("library".into());
            let playlist = |id: &str, name: &str| Playlist {
                id: id.into(),
                name: name.into(),
                tracks: None,
            };
            Ok(vec![
                playlist("liked", "Liked"),
                playlist("broken", "Broken"),
            ])
        }
        async fn playlist_tracks(&self, id: &str) -> Result<Vec<Track>> {
            self.calls.lock().unwrap().push(format!("tracks {id}"));
            match id {
                "broken" => Err(anyhow!("HTTP 500")),
                _ => Ok(vec![track("v1", "One"), track("v2", "Two")]),
            }
        }
        async fn search(&self, query: &str) -> Result<Vec<Track>> {
            self.calls.lock().unwrap().push(format!("search {query}"));
            match query {
                "nothing" => Ok(Vec::new()),
                _ => Ok(vec![track(&format!("s-{query}"), query)]),
            }
        }
        fn resolve_playback(&self, track: &Track) -> Result<PlaybackSource> {
            Ok(PlaybackSource::Url(format!(
                "https://music.example.invalid/watch?v={}",
                track.id
            )))
        }
    }

    struct Harness {
        app: App,
        provider: Arc<FakeProvider>,
        player: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Harness {
        /// An app using a `FakeProvider`, with its library loaded.
        async fn new() -> Self {
            let config_path = std::env::temp_dir().join("shellify-test-config.toml");
            let mut app = App::new(&Config::default(), config_path).unwrap();
            let player = FakePlayer::default();
            let calls = player.calls.clone();
            app.player = Some(Box::new(player));
            let provider = Arc::new(FakeProvider::default());
            app.set_provider(Some(provider.clone()));
            let mut h = Self {
                app,
                provider,
                player: calls,
            };
            h.settle().await;
            h
        }

        /// Handles provider replies until none are pending, then any stale
        /// ones still in flight.
        async fn settle(&mut self) {
            let mut inbox = self.app.inbox.take().unwrap();
            while !self.app.requests.pending.is_empty() {
                let event = inbox.recv().await.unwrap();
                self.app.handle(event);
            }
            while let Ok(Some(event)) =
                tokio::time::timeout(Duration::from_millis(20), inbox.recv()).await
            {
                self.app.handle(event);
            }
            self.app.inbox = Some(inbox);
        }

        fn provider_calls(&self) -> Vec<String> {
            std::mem::take(&mut *self.provider.calls.lock().unwrap())
        }

        fn titles(&self) -> Vec<&str> {
            self.app
                .state
                .tracks
                .iter()
                .map(|t| t.title.as_str())
                .collect()
        }
    }

    #[tokio::test]
    async fn starts_on_the_demo_library_and_loads_the_providers_instead() {
        let config_path = std::env::temp_dir().join("shellify-test-config.toml");
        let mut app = App::new(&Config::default(), config_path).unwrap();
        assert!(app.state.is_demo());
        assert_eq!(app.state.library[0].name, "Liked Songs");

        let provider = Arc::new(FakeProvider::default());
        app.set_provider(Some(provider));
        assert!(!app.state.is_demo());
        assert!(app.state.library_loading);
        assert!(app.state.library.is_empty());

        let event = app.inbox.as_mut().unwrap().recv().await.unwrap();
        app.handle(event);
        assert!(!app.state.library_loading);
        let names: Vec<_> = app.state.library.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Liked", "Broken"]);

        app.set_provider(None);
        assert!(app.state.is_demo());
        assert_eq!(app.state.library[0].name, "Liked Songs");
    }

    #[tokio::test]
    async fn opening_a_playlist_fetches_its_tracks_once() {
        let mut h = Harness::new().await;
        assert_eq!(h.provider_calls(), ["library"]);

        h.app.dispatch(Action::PlaySelected); // open "Liked"
        assert!(h.app.state.tracks_loading);
        assert_eq!(h.app.state.tracks_title, "Liked");
        assert_eq!(h.app.state.focus, Pane::Tracks);
        h.settle().await;
        assert!(!h.app.state.tracks_loading);
        assert_eq!(h.titles(), ["One", "Two"]);
        assert_eq!(h.provider_calls(), ["tracks liked"]);

        // Cached: reopening doesn't ask again.
        h.app.state.focus = Pane::Library;
        h.app.dispatch(Action::PlaySelected);
        assert!(!h.app.state.tracks_loading);
        assert_eq!(h.titles(), ["One", "Two"]);
        assert!(h.provider_calls().is_empty());
    }

    #[tokio::test]
    async fn a_failed_load_is_reported() {
        let mut h = Harness::new().await;
        h.app
            .dispatch(Action::Select(crate::app::action::Select::Last));
        h.app.dispatch(Action::PlaySelected); // open "Broken"
        h.settle().await;
        assert!(!h.app.state.tracks_loading);
        assert!(h.app.state.tracks.is_empty());
        let status = h.app.state.status.as_ref().unwrap();
        assert!(status.text.contains("HTTP 500"), "{}", status.text);
    }

    #[tokio::test]
    async fn only_the_latest_search_is_shown() {
        let mut h = Harness::new().await;
        h.app.dispatch(Action::Search("first".into()));
        h.app.dispatch(Action::Search("second".into()));
        assert!(h.app.state.tracks_loading);
        h.settle().await;
        assert_eq!(h.app.state.tracks_title, "Search: second");
        assert_eq!(h.titles(), ["second"]);
    }

    #[tokio::test]
    async fn a_search_replaces_a_playlist_still_loading() {
        let mut h = Harness::new().await;
        h.app.dispatch(Action::PlaySelected); // open "Liked"
        h.app.dispatch(Action::Search("song".into()));
        h.settle().await;
        assert_eq!(h.app.state.tracks_title, "Search: song");
        assert_eq!(h.titles(), ["song"]);
    }

    #[tokio::test]
    async fn play_query_plays_the_resolved_url() {
        let mut h = Harness::new().await;
        h.app.dispatch(Action::PlayQuery("hit".into()));
        assert!(h.player.lock().unwrap().is_empty());
        h.settle().await;
        assert_eq!(
            *h.player.lock().unwrap(),
            ["load https://music.example.invalid/watch?v=s-hit"]
        );

        h.app.dispatch(Action::PlayQuery("nothing".into()));
        h.settle().await;
        let status = h.app.state.status.as_ref().unwrap();
        assert!(status.text.contains("no results"), "{}", status.text);
    }

    #[tokio::test]
    async fn adding_an_unopened_playlist_queues_it_when_loaded() {
        let mut h = Harness::new().await;
        h.app.dispatch(Action::AddSelected); // on "Liked", in the Library pane
        assert!(h.app.state.queue.tracks().is_empty());
        h.settle().await;
        let queued: Vec<_> = h.app.state.queue.tracks().iter().map(|t| &t.id).collect();
        assert_eq!(queued, ["v1", "v2"]);
        // The Tracks pane was left alone.
        assert!(h.app.state.tracks.is_empty());
    }

    #[tokio::test]
    async fn tracks_from_a_provider_that_is_gone_fail_to_play() {
        let mut h = Harness::new().await;
        h.app.dispatch(Action::PlaySelected);
        h.settle().await;
        let tracks = h.app.state.tracks.clone();
        h.app.set_provider(None);
        h.app.state.queue.play_list(tracks, 0);
        h.app.start_current();
        assert!(
            h.player
                .lock()
                .unwrap()
                .iter()
                .all(|c| !c.starts_with("load"))
        );
        let status = h.app.state.status.as_ref().unwrap();
        assert!(status.text.contains("isn't set up"), "{}", status.text);
    }
}
