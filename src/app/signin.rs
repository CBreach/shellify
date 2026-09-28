//! Signing in to YouTube Music with Google (see `auth::google`).
//!
//! `:login` asks for the user's OAuth client ID (saved in the config) and
//! client secret (saved in the keychain) the first time, then shows a code
//! to enter at google.com/device. The refresh token goes in the keychain,
//! and the next start checks it quietly. All network and keychain work runs
//! in a spawned task that reports back as `AppEvent::SignIn`; each task has
//! a run number, so messages from one that was cancelled or replaced are
//! ignored.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::mpsc;

use super::state::{Credential, Mode, SignIn};
use super::{App, AppEvent};
use crate::auth::google::{AuthError, DeviceCode, Google, OAuthClient, Session};
use crate::auth::{Secret, SecretStore, keys};
use crate::config;
use crate::provider::ProviderKind;

#[derive(Debug)]
pub struct SignInEvent {
    run: u64,
    update: Update,
}

#[derive(Debug)]
enum Update {
    /// No client secret saved yet: ask for it.
    NeedSecret,
    /// Show the user this code.
    Code(DeviceCode),
    SignedIn {
        session: Arc<Session>,
        account: Option<String>,
        /// Say so (after `:login`), or stay quiet (restoring at startup).
        announce: bool,
        /// Signed in, but something is off (e.g. offline at startup).
        warning: Option<String>,
    },
    /// Not signed in, with the reason when it should be shown.
    SignedOut(Option<String>),
    /// Google rejected the client ID or secret.
    BadClient,
    Failed(String),
}

/// A sign-in task's way to report back.
struct Reporter {
    run: u64,
    events: mpsc::UnboundedSender<AppEvent>,
}

impl Reporter {
    fn send(&self, update: Update) {
        let event = SignInEvent {
            run: self.run,
            update,
        };
        // Fails only when the app is shutting down.
        let _ = self.events.send(AppEvent::SignIn(event));
    }
}

const YOUTUBE: ProviderKind = ProviderKind::YouTubeMusic;

impl App {
    /// `:login`: switches YouTube Music on if needed and signs in, asking
    /// for the client ID first if the config doesn't have one.
    pub(super) fn login(&mut self) {
        if self.state.active_provider != Some(YOUTUBE) {
            self.use_provider(Some(YOUTUBE));
        }
        if let SignIn::SignedIn { account } = &self.state.sign_in {
            let who = account.as_deref().unwrap_or("your Google account");
            return self
                .state
                .info(format!("Already signed in as {who} (:logout to sign out)"));
        }
        self.state.provider_setup = Some(YOUTUBE);
        match self.google_client_id.clone() {
            Some(id) => self.start_sign_in(id, None),
            None => self.ask(
                Credential::ClientId,
                "Paste your Google OAuth client ID (see the README) · esc cancels",
            ),
        }
    }

    fn ask(&mut self, field: Credential, hint: &str) {
        self.state.credential_line.clear();
        self.state.mode = Mode::Credential(field);
        self.state.info(hint);
    }

    /// Enter in the `:login` prompt.
    pub(super) fn submit_credential(&mut self, field: Credential, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return self.state.info("Sign-in cancelled");
        }
        match field {
            Credential::ClientId => {
                if let Err(e) = config::save_google_client_id(&self.config_path, Some(text)) {
                    self.state
                        .error(format!("couldn't save the client ID: {e:#}"));
                }
                self.google_client_id = Some(text.to_string());
                self.start_sign_in(text.to_string(), None);
            }
            Credential::ClientSecret => {
                if let Some(id) = self.google_client_id.clone() {
                    self.start_sign_in(id, Some(Secret::new(text)));
                }
            }
        }
    }

    fn start_sign_in(&mut self, client_id: String, typed_secret: Option<Secret>) {
        self.state.sign_in = SignIn::Working("Contacting Google");
        let store = self.secrets.clone();
        let endpoints = self.google_endpoints.clone();
        self.spawn_sign_in(move |report| async move {
            let secret = match typed_secret.clone() {
                Some(secret) => secret,
                None => match load(&store, keys::YOUTUBE_CLIENT_SECRET).await {
                    Ok(Some(secret)) => secret,
                    Ok(None) => return report.send(Update::NeedSecret),
                    Err(e) => return report.send(Update::Failed(format!("{e:#}"))),
                },
            };
            let client = OAuthClient {
                id: client_id,
                secret: secret.clone(),
            };
            let google = Google::new(client, endpoints);
            let signed_in = async {
                let code = google.device_code().await?;
                report.send(Update::Code(code.clone()));
                let tokens = google.wait_for_approval(&code).await?;
                save(&store, keys::YOUTUBE_REFRESH_TOKEN, tokens.refresh.clone()).await?;
                if typed_secret.is_some() {
                    save(&store, keys::YOUTUBE_CLIENT_SECRET, secret).await?;
                }
                let account = google.channel_name(&tokens.access).await.ok().flatten();
                Ok::<_, anyhow::Error>((Session::from_tokens(google, tokens), account))
            };
            report.send(match signed_in.await {
                Ok((session, account)) => Update::SignedIn {
                    session: Arc::new(session),
                    account,
                    announce: true,
                    warning: None,
                },
                Err(e) => failure(e),
            });
        });
    }

    /// Picks up a saved sign-in (when YouTube Music is switched on or at
    /// startup), checking it's still valid.
    pub(super) fn restore_sign_in(&mut self) {
        let Some(client_id) = self.google_client_id.clone() else {
            return;
        };
        if matches!(self.state.sign_in, SignIn::SignedIn { .. }) {
            return;
        }
        self.state.sign_in = SignIn::Working("Checking your sign-in");
        let store = self.secrets.clone();
        let endpoints = self.google_endpoints.clone();
        self.spawn_sign_in(move |report| async move {
            let saved = async {
                let secret = load(&store, keys::YOUTUBE_CLIENT_SECRET).await?;
                let refresh = load(&store, keys::YOUTUBE_REFRESH_TOKEN).await?;
                Ok::<_, anyhow::Error>(secret.zip(refresh))
            };
            let (secret, refresh) = match saved.await {
                Ok(Some(saved)) => saved,
                Ok(None) => return report.send(Update::SignedOut(None)),
                Err(e) => return report.send(Update::Failed(format!("{e:#}"))),
            };
            let google = Google::new(OAuthClient { id: client_id, secret }, endpoints);
            let session = Arc::new(Session::new(google, refresh));
            let update = match session.access_token().await {
                Ok(access) => Update::SignedIn {
                    account: session.google().channel_name(&access).await.ok().flatten(),
                    session,
                    announce: false,
                    warning: None,
                },
                Err(e) => match e.downcast_ref::<AuthError>() {
                    Some(AuthError::Revoked) => {
                        let _ = delete(&store, keys::YOUTUBE_REFRESH_TOKEN).await;
                        Update::SignedOut(Some(
                            "Your YouTube Music sign-in expired or was revoked: run :login to sign in again"
                                .into(),
                        ))
                    }
                    Some(AuthError::BadClient) => Update::BadClient,
                    // Probably offline: keep the sign-in and try again later.
                    _ => Update::SignedIn {
                        session,
                        account: None,
                        announce: false,
                        warning: Some(format!("Couldn't check your YouTube Music sign-in: {e:#}")),
                    },
                },
            };
            report.send(update);
        });
    }

    /// `:logout`: forgets the sign-in here and asks Google to revoke it.
    pub(super) fn logout(&mut self) {
        self.cancel_sign_in();
        if matches!(self.state.mode, Mode::Credential(_)) {
            self.state.mode = Mode::Normal;
        }
        let was_signed_in = matches!(self.state.sign_in, SignIn::SignedIn { .. });
        self.state.sign_in = SignIn::SignedOut;
        let session = self.youtube_session.take();
        let store = self.secrets.clone();
        tokio::spawn(async move {
            if let Err(e) = delete(&store, keys::YOUTUBE_REFRESH_TOKEN).await {
                tracing::warn!("forgetting the refresh token: {e:#}");
            }
            if let Some(session) = session
                && let Err(e) = session.google().revoke(session.refresh_token()).await
            {
                tracing::warn!("revoking the sign-in: {e:#}");
            }
        });
        if was_signed_in {
            self.state.info("Signed out of YouTube Music");
        } else {
            self.state.info("Not signed in to YouTube Music");
        }
    }

    /// Stops any sign-in in progress and forgets the session (YouTube Music
    /// was switched off). The saved sign-in stays for next time.
    pub(super) fn drop_sign_in(&mut self) {
        self.cancel_sign_in();
        self.youtube_session = None;
        self.state.sign_in = SignIn::SignedOut;
    }

    fn cancel_sign_in(&mut self) {
        if let Some(task) = self.sign_in_task.take() {
            task.abort();
        }
        self.sign_in_run += 1;
    }

    fn spawn_sign_in<F>(&mut self, task: impl FnOnce(Reporter) -> F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.cancel_sign_in();
        let report = Reporter {
            run: self.sign_in_run,
            events: self.events.clone(),
        };
        self.sign_in_task = Some(tokio::spawn(task(report)).abort_handle());
    }

    pub(super) fn on_sign_in_event(&mut self, event: SignInEvent) {
        if event.run != self.sign_in_run {
            tracing::debug!(run = event.run, "stale sign-in message");
            return;
        }
        match event.update {
            Update::NeedSecret => {
                self.state.sign_in = SignIn::SignedOut;
                self.ask(
                    Credential::ClientSecret,
                    "Paste the client secret (kept in your keychain) · esc cancels",
                );
            }
            Update::Code(code) => {
                self.state.info(format!(
                    "Enter {} at {}",
                    code.user_code, code.verification_url
                ));
                self.state.sign_in = SignIn::Code {
                    url: code.verification_url,
                    code: code.user_code,
                    expires: Instant::now() + code.expires_in,
                };
                self.state.provider_setup = Some(YOUTUBE);
            }
            Update::SignedIn {
                session,
                account,
                announce,
                warning,
            } => {
                self.sign_in_task = None;
                self.youtube_session = Some(session);
                if let Some(warning) = warning {
                    self.state.error(warning);
                } else if announce {
                    let who = account.as_deref().unwrap_or("your Google account");
                    self.state
                        .info(format!("Signed in to YouTube Music as {who}"));
                }
                self.state.sign_in = SignIn::SignedIn { account };
            }
            Update::SignedOut(reason) => {
                self.sign_in_task = None;
                self.state.sign_in = SignIn::SignedOut;
                if let Some(reason) = reason {
                    self.state.error(reason);
                }
            }
            Update::BadClient => {
                self.sign_in_task = None;
                self.state.sign_in = SignIn::SignedOut;
                self.google_client_id = None;
                let store = self.secrets.clone();
                tokio::spawn(async move {
                    let _ = delete(&store, keys::YOUTUBE_CLIENT_SECRET).await;
                });
                if let Err(e) = config::save_google_client_id(&self.config_path, None) {
                    tracing::warn!("forgetting the client ID: {e:#}");
                }
                self.state.error(
                    "Google rejected the client ID or secret: run :login to enter them again",
                );
            }
            Update::Failed(message) => {
                self.sign_in_task = None;
                self.state.sign_in = SignIn::SignedOut;
                self.state.error(format!("Sign-in failed: {message}"));
            }
        }
    }
}

fn failure(error: anyhow::Error) -> Update {
    match error.downcast_ref::<AuthError>() {
        Some(AuthError::BadClient) => Update::BadClient,
        _ => Update::Failed(format!("{error:#}")),
    }
}

/// The keychain can block (or show a dialog), so it's used off the runtime's
/// worker threads.
async fn load(store: &Arc<dyn SecretStore>, key: &'static str) -> Result<Option<Secret>> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || store.get(key)).await?
}

async fn save(store: &Arc<dyn SecretStore>, key: &'static str, value: Secret) -> Result<()> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || store.set(key, &value)).await?
}

async fn delete(store: &Arc<dyn SecretStore>, key: &'static str) -> Result<()> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || store.delete(key)).await?
}

/// `mm:ss` left before a sign-in code expires.
pub fn time_left(expires: Instant) -> String {
    let left = expires.saturating_duration_since(Instant::now());
    let secs = left.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::auth::MemoryStore;
    use crate::auth::google::Endpoints;
    use crate::config::Config;

    const CLIENT_ID: &str = "fake-client-id.apps.googleusercontent.com";

    fn json(status: u16, body: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(body)
    }

    /// A Google that hands out a code, approves it on the first poll,
    /// refreshes tokens and knows the channel name.
    async fn google() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(path("/device/code"))
            .respond_with(json(
                200,
                serde_json::json!({
                    "device_code": "fake-device-code",
                    "user_code": "ABCD-EFGH",
                    "verification_url": "https://www.google.com/device",
                    "expires_in": 1800,
                    "interval": 0
                }),
            ))
            .mount(&server)
            .await;
        Mock::given(path("/token"))
            .and(body_string_contains("grant_type=urn"))
            .respond_with(json(
                200,
                serde_json::json!({
                    "access_token": "fake-access",
                    "expires_in": 3599,
                    "refresh_token": "fake-refresh"
                }),
            ))
            .mount(&server)
            .await;
        Mock::given(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(json(
                200,
                serde_json::json!({ "access_token": "fake-access-2", "expires_in": 3599 }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/youtube/v3/channels"))
            .respond_with(json(
                200,
                serde_json::json!({ "items": [{ "snippet": { "title": "Test Listener" } }] }),
            ))
            .mount(&server)
            .await;
        server
    }

    struct Harness {
        app: App,
        store: Arc<MemoryStore>,
        config_path: PathBuf,
        _dir: tempfile::TempDir,
    }

    impl Harness {
        fn new(server: &MockServer) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let config_path = dir.path().join("config.toml");
            let store = Arc::new(MemoryStore::default());
            let app = Self::app(&config_path, &store, server);
            Self {
                app,
                store,
                config_path,
                _dir: dir,
            }
        }

        /// An app as if started with the saved config and keychain.
        fn app(config_path: &Path, store: &Arc<MemoryStore>, server: &MockServer) -> App {
            let config = Config::load(config_path).unwrap();
            let mut app = App::new(&config, config_path.to_path_buf()).unwrap();
            app.secrets = store.clone();
            app.google_endpoints = Endpoints::at(&server.uri());
            app
        }

        fn restart(&mut self, server: &MockServer) {
            self.app = Self::app(&self.config_path, &self.store, server);
            self.app.resume_provider();
        }

        fn type_line(&mut self, text: &str) {
            for c in text.chars() {
                self.key(KeyCode::Char(c));
            }
            self.key(KeyCode::Enter);
        }

        fn key(&mut self, code: KeyCode) {
            self.app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
        }

        /// Handles events until `done` holds.
        async fn until(&mut self, done: impl Fn(&App) -> bool) {
            let mut inbox = self.app.inbox.take().unwrap();
            let wait = async {
                while !done(&self.app) {
                    let event = inbox.recv().await.unwrap();
                    self.app.handle(event);
                }
            };
            tokio::time::timeout(Duration::from_secs(5), wait)
                .await
                .expect("timed out waiting for the app");
            self.app.inbox = Some(inbox);
        }

        fn secret(&self, key: &str) -> Option<String> {
            self.store.get(key).unwrap().map(|s| s.expose().to_string())
        }
    }

    fn signed_in(app: &App) -> bool {
        matches!(app.state.sign_in, SignIn::SignedIn { .. })
    }

    #[tokio::test]
    async fn first_login_asks_for_the_client_shows_a_code_and_signs_in() {
        let server = google().await;
        let mut h = Harness::new(&server);
        h.app.dispatch(crate::app::action::Action::Login);
        assert_eq!(h.app.state.active_provider, Some(YOUTUBE), "turned on");
        assert_eq!(h.app.state.mode, Mode::Credential(Credential::ClientId));

        h.type_line(CLIENT_ID);
        h.until(|app| app.state.mode == Mode::Credential(Credential::ClientSecret))
            .await;
        h.type_line("fake-client-secret");
        h.until(|app| {
            matches!(
                app.state.sign_in,
                SignIn::Code { .. } | SignIn::SignedIn { .. }
            )
        })
        .await;
        h.until(signed_in).await;
        assert_eq!(
            h.app.state.sign_in,
            SignIn::SignedIn {
                account: Some("Test Listener".into())
            }
        );
        let status = h.app.state.status.as_ref().unwrap();
        assert!(
            status
                .text
                .contains("Signed in to YouTube Music as Test Listener")
        );

        // The ID is in the config; the secret and token only in the keychain.
        let config = std::fs::read_to_string(&h.config_path).unwrap();
        assert!(config.contains(CLIENT_ID), "{config}");
        assert!(!config.contains("fake-client-secret") && !config.contains("fake-refresh"));
        assert_eq!(
            h.secret(keys::YOUTUBE_CLIENT_SECRET).as_deref(),
            Some("fake-client-secret")
        );
        assert_eq!(
            h.secret(keys::YOUTUBE_REFRESH_TOKEN).as_deref(),
            Some("fake-refresh")
        );

        // Neither prompt kept its input in history.
        h.app.state.credential_line.history_prev();
        assert!(h.app.state.credential_line.text().is_empty());

        // The next start signs in quietly from the keychain.
        h.restart(&server);
        assert_eq!(
            h.app.state.sign_in,
            SignIn::Working("Checking your sign-in")
        );
        h.until(signed_in).await;
        assert!(h.app.youtube_session.is_some());
    }

    #[tokio::test]
    async fn logout_forgets_the_token_and_revokes_it() {
        let server = google().await;
        Mock::given(path("/revoke"))
            .and(body_string_contains("token=fake-refresh"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let mut h = Harness::new(&server);
        h.store
            .set(
                keys::YOUTUBE_CLIENT_SECRET,
                &Secret::new("fake-client-secret"),
            )
            .unwrap();
        h.app.google_client_id = Some(CLIENT_ID.into());
        h.app.dispatch(crate::app::action::Action::Login);
        h.until(signed_in).await;

        h.app.dispatch(crate::app::action::Action::Logout);
        assert_eq!(h.app.state.sign_in, SignIn::SignedOut);
        assert!(h.app.youtube_session.is_none());
        tokio::time::timeout(Duration::from_secs(5), async {
            while h.secret(keys::YOUTUBE_REFRESH_TOKEN).is_some()
                || server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .all(|r| r.url.path() != "/revoke")
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("token forgotten and revoked");
        // The client stays, so signing in again only needs approving.
        assert!(h.secret(keys::YOUTUBE_CLIENT_SECRET).is_some());
    }

    #[tokio::test]
    async fn a_rejected_client_is_forgotten() {
        let server = MockServer::start().await;
        Mock::given(path("/device/code"))
            .respond_with(json(401, serde_json::json!({ "error": "invalid_client" })))
            .mount(&server)
            .await;
        let mut h = Harness::new(&server);
        h.app.dispatch(crate::app::action::Action::Login);
        h.type_line(CLIENT_ID);
        h.until(|app| app.state.mode == Mode::Credential(Credential::ClientSecret))
            .await;
        h.type_line("wrong-secret");
        h.until(|app| app.google_client_id.is_none()).await;
        assert_eq!(h.app.state.sign_in, SignIn::SignedOut);
        assert!(h.app.status_is_error());
        let config = std::fs::read_to_string(&h.config_path).unwrap();
        assert!(!config.contains(CLIENT_ID), "{config}");
        assert_eq!(h.secret(keys::YOUTUBE_CLIENT_SECRET), None, "never saved");
    }

    #[tokio::test]
    async fn an_expired_sign_in_at_startup_signs_out_and_says_so() {
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(json(400, serde_json::json!({ "error": "invalid_grant" })))
            .mount(&server)
            .await;
        let mut h = Harness::new(&server);
        crate::config::save_provider(&h.config_path, Some("youtube-music")).unwrap();
        crate::config::save_google_client_id(&h.config_path, Some(CLIENT_ID)).unwrap();
        h.store
            .set(
                keys::YOUTUBE_CLIENT_SECRET,
                &Secret::new("fake-client-secret"),
            )
            .unwrap();
        h.store
            .set(keys::YOUTUBE_REFRESH_TOKEN, &Secret::new("fake-refresh"))
            .unwrap();
        h.restart(&server);
        h.until(|app| app.state.sign_in == SignIn::SignedOut).await;
        let status = h.app.state.status.as_ref().unwrap();
        assert!(
            status.text.contains("expired or was revoked"),
            "{}",
            status.text
        );
        assert_eq!(h.secret(keys::YOUTUBE_REFRESH_TOKEN), None);
    }

    #[tokio::test]
    async fn switching_youtube_music_off_abandons_a_sign_in() {
        let server = google().await;
        let mut h = Harness::new(&server);
        h.store
            .set(
                keys::YOUTUBE_CLIENT_SECRET,
                &Secret::new("fake-client-secret"),
            )
            .unwrap();
        h.app.google_client_id = Some(CLIENT_ID.into());
        h.app.dispatch(crate::app::action::Action::Login);
        h.app.dispatch(crate::app::action::Action::Provider(None));
        // Whatever the abandoned task managed to send is ignored.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut inbox = h.app.inbox.take().unwrap();
        while let Ok(event) = inbox.try_recv() {
            h.app.handle(event);
        }
        assert_eq!(h.app.state.sign_in, SignIn::SignedOut);
        assert!(h.app.youtube_session.is_none());
    }

    #[test]
    fn time_left_counts_down_in_minutes_and_seconds() {
        let expires = Instant::now() + Duration::from_secs(125) + Duration::from_millis(500);
        assert_eq!(time_left(expires), "2:05");
        assert_eq!(time_left(Instant::now()), "0:00");
    }
}
