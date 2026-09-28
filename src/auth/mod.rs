//! Signing in: secrets and where they're kept.
//!
//! Secrets (OAuth client secrets, refresh tokens) live in the OS keychain,
//! never in the config file or the repo, and never in logs: `Secret` hides
//! its value from `Debug`.

pub mod google;

use std::fmt;

use anyhow::{Context, Result};

/// A value that must not be logged or shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Keychain entry names.
pub mod keys {
    /// The OAuth client secret the user created for YouTube Music.
    pub const YOUTUBE_CLIENT_SECRET: &str = "youtube-music/client-secret";
    /// The refresh token from signing in to YouTube Music.
    pub const YOUTUBE_REFRESH_TOKEN: &str = "youtube-music/refresh-token";
}

/// Where secrets are kept. Calls may block (the keychain can be slow), so
/// the app makes them from `spawn_blocking`.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Secret>>;
    fn set(&self, key: &str, value: &Secret) -> Result<()>;
    /// Removing a secret that isn't there is not an error.
    fn delete(&self, key: &str) -> Result<()>;
}

/// The OS keychain (macOS Keychain, Secret Service on Linux). Tests use
/// `MemoryStore` instead, so they never touch it.
#[cfg_attr(test, allow(dead_code))]
pub struct Keychain;

#[cfg_attr(test, allow(dead_code))]
const SERVICE: &str = "shellify";

impl SecretStore for Keychain {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        let entry = keyring::Entry::new(SERVICE, key).context("opening the keychain")?;
        match entry.get_password() {
            Ok(value) => Ok(Some(Secret(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e).context("reading from the keychain"),
        }
    }

    fn set(&self, key: &str, value: &Secret) -> Result<()> {
        keyring::Entry::new(SERVICE, key)
            .context("opening the keychain")?
            .set_password(value.expose())
            .context("saving to the keychain")
    }

    fn delete(&self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, key).context("opening the keychain")?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e).context("removing from the keychain"),
        }
    }
}

/// A keychain stand-in for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<std::collections::HashMap<String, Secret>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &Secret) -> Result<()> {
        self.0.lock().unwrap().insert(key.into(), value.clone());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_show_in_debug_output() {
        let secret = Secret::new("fake-refresh-token-123");
        let shown = format!("{secret:?} {:?}", Some(&secret));
        assert!(!shown.contains("fake-refresh-token-123"), "{shown}");
        assert_eq!(secret.expose(), "fake-refresh-token-123");
    }

    #[test]
    fn memory_store_round_trips_and_deletes_quietly() {
        let store = MemoryStore::default();
        assert_eq!(store.get("k").unwrap(), None);
        store.set("k", &Secret::new("v")).unwrap();
        assert_eq!(store.get("k").unwrap(), Some(Secret::new("v")));
        store.delete("k").unwrap();
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
    }
}
