//! Google sign-in with the OAuth 2.0 device flow (the one TVs use): Shellify
//! shows a short code, the user approves it at google.com/device on any
//! device, and Google hands back a refresh token. It asks only for
//! read-only YouTube access, and the user can revoke it any time at
//! myaccount.google.com/permissions.
//!
//! Each user brings their own OAuth client ("TVs and Limited Input devices"
//! type) from their own Google Cloud project; see the README.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::Secret;

/// Read-only access to the user's YouTube account (playlists, likes).
pub const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";

/// How much longer to wait between polls when Google says to slow down.
const SLOW_DOWN: Duration = Duration::from_secs(5);
/// Refresh access tokens this long before they expire.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

/// Where the Google APIs are; tests point these at a local mock server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub device_code: String,
    pub token: String,
    pub revoke: String,
    /// YouTube Data API v3 base URL.
    pub youtube: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            device_code: "https://oauth2.googleapis.com/device/code".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            youtube: "https://www.googleapis.com/youtube/v3".into(),
        }
    }
}

impl Endpoints {
    /// Every endpoint under one base URL, for tests.
    #[cfg(test)]
    pub fn at(base: &str) -> Self {
        Self {
            device_code: format!("{base}/device/code"),
            token: format!("{base}/token"),
            revoke: format!("{base}/revoke"),
            youtube: format!("{base}/youtube/v3"),
        }
    }
}

/// The user's OAuth client.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub id: String,
    pub secret: Secret,
}

/// Why signing in or staying signed in failed, when the app needs to act on
/// it (anything else is a plain error).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("Google rejected the client ID or secret")]
    BadClient,
    #[error("sign-in was declined")]
    Denied,
    #[error("the sign-in code expired before it was approved")]
    Expired,
    #[error("the sign-in is no longer valid (it expired or was revoked)")]
    Revoked,
}

/// What to show the user while waiting for them to approve.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    device_code: Secret,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: Duration,
    pub interval: Duration,
}

/// Tokens from a successful sign-in.
#[derive(Debug, Clone)]
pub struct Tokens {
    pub access: Secret,
    pub expires_in: Duration,
    pub refresh: Secret,
}

#[derive(Debug)]
enum Poll {
    Pending,
    SlowDown,
    Granted(Tokens),
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct DeviceCodeBody {
    device_code: String,
    user_code: String,
    #[serde(alias = "verification_uri")]
    verification_url: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenBody {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Talks to Google's OAuth endpoints and the YouTube Data API.
#[derive(Clone)]
pub struct Google {
    http: reqwest::Client,
    endpoints: Arc<Endpoints>,
    client: OAuthClient,
}

impl Google {
    pub fn new(client: OAuthClient, endpoints: Endpoints) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("building the HTTP client");
        Self {
            http,
            endpoints: Arc::new(endpoints),
            client,
        }
    }

    /// Starts signing in: the code for the user to enter, and where.
    pub async fn device_code(&self) -> Result<DeviceCode> {
        let response = self
            .http
            .post(&self.endpoints.device_code)
            .form(&[("client_id", self.client.id.as_str()), ("scope", SCOPE)])
            .send()
            .await
            .context("couldn't reach Google")?;
        if !response.status().is_success() {
            return Err(oauth_error(response).await);
        }
        let body: DeviceCodeBody = response
            .json()
            .await
            .context("unexpected reply from Google")?;
        Ok(DeviceCode {
            device_code: Secret::new(body.device_code),
            user_code: body.user_code,
            verification_url: body.verification_url,
            expires_in: Duration::from_secs(body.expires_in),
            interval: Duration::from_secs(body.interval),
        })
    }

    /// Waits for the user to approve `code`, polling Google as often as it
    /// allows.
    pub async fn wait_for_approval(&self, code: &DeviceCode) -> Result<Tokens> {
        let deadline = Instant::now() + code.expires_in;
        let mut interval = code.interval;
        loop {
            tokio::time::sleep(interval).await;
            match self.poll(code).await? {
                Poll::Pending => {}
                Poll::SlowDown => interval += SLOW_DOWN,
                Poll::Granted(tokens) => return Ok(tokens),
            }
            if Instant::now() >= deadline {
                return Err(AuthError::Expired.into());
            }
        }
    }

    async fn poll(&self, code: &DeviceCode) -> Result<Poll> {
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("client_id", self.client.id.as_str()),
                ("client_secret", self.client.secret.expose()),
                ("device_code", code.device_code.expose()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .context("couldn't reach Google")?;
        if response.status().is_success() {
            let body: TokenBody = response
                .json()
                .await
                .context("unexpected reply from Google")?;
            let refresh = body
                .refresh_token
                .context("Google didn't return a refresh token")?;
            return Ok(Poll::Granted(Tokens {
                access: Secret::new(body.access_token),
                expires_in: Duration::from_secs(body.expires_in),
                refresh: Secret::new(refresh),
            }));
        }
        let error = oauth_error(response).await;
        match error.downcast_ref::<Pending>() {
            Some(Pending::Wait) => Ok(Poll::Pending),
            Some(Pending::SlowDown) => Ok(Poll::SlowDown),
            None => Err(error),
        }
    }

    /// A fresh access token from a refresh token.
    pub async fn refresh(&self, refresh: &Secret) -> Result<(Secret, Duration)> {
        let response = self
            .http
            .post(&self.endpoints.token)
            .form(&[
                ("client_id", self.client.id.as_str()),
                ("client_secret", self.client.secret.expose()),
                ("refresh_token", refresh.expose()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .context("couldn't reach Google")?;
        if !response.status().is_success() {
            return Err(oauth_error(response).await);
        }
        let body: TokenBody = response
            .json()
            .await
            .context("unexpected reply from Google")?;
        Ok((
            Secret::new(body.access_token),
            Duration::from_secs(body.expires_in),
        ))
    }

    /// Tells Google to forget the grant (signing out everywhere).
    pub async fn revoke(&self, token: &Secret) -> Result<()> {
        let response = self
            .http
            .post(&self.endpoints.revoke)
            .form(&[("token", token.expose())])
            .send()
            .await
            .context("couldn't reach Google")?;
        if !response.status().is_success() {
            return Err(oauth_error(response).await);
        }
        Ok(())
    }

    /// The name of the signed-in user's YouTube channel, if they have one.
    pub async fn channel_name(&self, access: &Secret) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Channels {
            #[serde(default)]
            items: Vec<Channel>,
        }
        #[derive(Deserialize)]
        struct Channel {
            snippet: Snippet,
        }
        #[derive(Deserialize)]
        struct Snippet {
            title: String,
        }
        let response = self
            .http
            .get(format!("{}/channels", self.endpoints.youtube))
            .query(&[("part", "snippet"), ("mine", "true")])
            .bearer_auth(access.expose())
            .send()
            .await
            .context("couldn't reach YouTube")?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AuthError::Revoked.into());
        }
        if !status.is_success() {
            anyhow::bail!("YouTube returned {status}");
        }
        let channels: Channels = response
            .json()
            .await
            .context("unexpected reply from YouTube")?;
        Ok(channels.items.into_iter().next().map(|c| c.snippet.title))
    }
}

/// Polling answers that mean "not yet" rather than failure.
#[derive(Debug, thiserror::Error)]
enum Pending {
    #[error("waiting for approval")]
    Wait,
    #[error("polling too fast")]
    SlowDown,
}

/// Turns an OAuth error reply into an error the app can act on.
async fn oauth_error(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let Ok(body) = response.json::<ErrorBody>().await else {
        return anyhow::anyhow!("Google returned {status}");
    };
    match body.error.as_str() {
        "authorization_pending" => Pending::Wait.into(),
        "slow_down" => Pending::SlowDown.into(),
        "access_denied" => AuthError::Denied.into(),
        "expired_token" => AuthError::Expired.into(),
        "invalid_grant" => AuthError::Revoked.into(),
        "invalid_client" | "unauthorized_client" => AuthError::BadClient.into(),
        other => match body.error_description {
            Some(description) => anyhow::anyhow!("Google said {other}: {description}"),
            None => anyhow::anyhow!("Google said {other}"),
        },
    }
}

/// A signed-in user: hands out access tokens, refreshing them as needed.
/// Its `Debug` shows nothing secret.
pub struct Session {
    google: Google,
    refresh: Secret,
    access: Mutex<Option<(Secret, Instant)>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session")
    }
}

impl Session {
    pub fn new(google: Google, refresh: Secret) -> Self {
        Self {
            google,
            refresh,
            access: Mutex::new(None),
        }
    }

    /// A session straight after signing in, with its first access token.
    pub fn from_tokens(google: Google, tokens: Tokens) -> Self {
        let expires = Instant::now() + tokens.expires_in;
        Self {
            google,
            refresh: tokens.refresh,
            access: Mutex::new(Some((tokens.access, expires))),
        }
    }

    pub fn google(&self) -> &Google {
        &self.google
    }

    pub fn refresh_token(&self) -> &Secret {
        &self.refresh
    }

    /// A valid access token, refreshed if the cached one is (nearly) expired.
    pub async fn access_token(&self) -> Result<Secret> {
        let mut cached = self.access.lock().await;
        if let Some((token, expires)) = cached.as_ref()
            && Instant::now() + EXPIRY_MARGIN < *expires
        {
            return Ok(token.clone());
        }
        let (token, lifetime) = self.google.refresh(&self.refresh).await?;
        *cached = Some((token.clone(), Instant::now() + lifetime));
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const CLIENT_ID: &str = "fake-client-id.apps.googleusercontent.com";

    fn google(server: &MockServer) -> Google {
        let client = OAuthClient {
            id: CLIENT_ID.into(),
            secret: Secret::new("fake-client-secret"),
        };
        Google::new(client, Endpoints::at(&server.uri()))
    }

    fn json(status: u16, body: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(body)
    }

    fn oauth_error_reply(error: &str) -> ResponseTemplate {
        json(428, serde_json::json!({ "error": error }))
    }

    async fn device_code(server: &MockServer) -> DeviceCode {
        Mock::given(method("POST"))
            .and(path("/device/code"))
            .and(body_string_contains(
                "scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fyoutube.readonly",
            ))
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
            .mount(server)
            .await;
        google(server).device_code().await.unwrap()
    }

    #[tokio::test]
    async fn device_code_asks_for_read_only_youtube() {
        let server = MockServer::start().await;
        let code = device_code(&server).await;
        assert_eq!(code.user_code, "ABCD-EFGH");
        assert_eq!(code.verification_url, "https://www.google.com/device");
        assert_eq!(code.expires_in, Duration::from_secs(1800));
    }

    #[tokio::test]
    async fn waits_while_pending_then_returns_the_tokens() {
        let server = MockServer::start().await;
        let code = device_code(&server).await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("device_code=fake-device-code"))
            .respond_with(oauth_error_reply("authorization_pending"))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(json(
                200,
                serde_json::json!({
                    "access_token": "fake-access",
                    "expires_in": 3599,
                    "refresh_token": "fake-refresh",
                    "scope": SCOPE,
                    "token_type": "Bearer"
                }),
            ))
            .mount(&server)
            .await;
        let tokens = google(&server).wait_for_approval(&code).await.unwrap();
        assert_eq!(tokens.refresh.expose(), "fake-refresh");
        assert_eq!(tokens.access.expose(), "fake-access");
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn declined_expired_and_bad_clients_are_told_apart() {
        for (reply, expected) in [
            ("access_denied", AuthError::Denied),
            ("expired_token", AuthError::Expired),
            ("invalid_client", AuthError::BadClient),
        ] {
            let server = MockServer::start().await;
            let code = device_code(&server).await;
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(oauth_error_reply(reply))
                .mount(&server)
                .await;
            let error = google(&server).wait_for_approval(&code).await.unwrap_err();
            assert_eq!(
                error.downcast_ref::<AuthError>(),
                Some(&expected),
                "{reply}"
            );
        }
    }

    #[tokio::test]
    async fn a_bad_client_is_reported_when_asking_for_a_code() {
        let server = MockServer::start().await;
        Mock::given(path("/device/code"))
            .respond_with(json(401, serde_json::json!({ "error": "invalid_client" })))
            .mount(&server)
            .await;
        let error = google(&server).device_code().await.unwrap_err();
        assert_eq!(
            error.downcast_ref::<AuthError>(),
            Some(&AuthError::BadClient)
        );
    }

    #[tokio::test]
    async fn sessions_cache_access_tokens_and_refresh_when_needed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=fake-refresh"))
            .respond_with(json(
                200,
                serde_json::json!({ "access_token": "fake-access-2", "expires_in": 3599 }),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let session = Session::new(google(&server), Secret::new("fake-refresh"));
        assert_eq!(
            session.access_token().await.unwrap().expose(),
            "fake-access-2"
        );
        // Cached now: no second refresh (checked by `expect(1)` on drop).
        assert_eq!(
            session.access_token().await.unwrap().expose(),
            "fake-access-2"
        );
    }

    #[tokio::test]
    async fn a_revoked_refresh_token_is_reported_as_such() {
        let server = MockServer::start().await;
        Mock::given(path("/token"))
            .respond_with(json(400, serde_json::json!({ "error": "invalid_grant" })))
            .mount(&server)
            .await;
        let session = Session::new(google(&server), Secret::new("fake-refresh"));
        let error = session.access_token().await.unwrap_err();
        assert_eq!(error.downcast_ref::<AuthError>(), Some(&AuthError::Revoked));
    }

    #[tokio::test]
    async fn reads_the_channel_name_with_the_access_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/youtube/v3/channels"))
            .and(query_param("mine", "true"))
            .and(header("authorization", "Bearer fake-access"))
            .respond_with(json(
                200,
                serde_json::json!({ "items": [{ "snippet": { "title": "Test Listener" } }] }),
            ))
            .mount(&server)
            .await;
        let name = google(&server)
            .channel_name(&Secret::new("fake-access"))
            .await
            .unwrap();
        assert_eq!(name.as_deref(), Some("Test Listener"));
    }

    #[tokio::test]
    async fn revoking_posts_the_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/revoke"))
            .and(body_string_contains("token=fake-refresh"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        google(&server)
            .revoke(&Secret::new("fake-refresh"))
            .await
            .unwrap();
    }
}
