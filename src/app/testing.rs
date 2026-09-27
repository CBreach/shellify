//! Test doubles for driving the app without mpv or a network.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::player::{LoadId, Player};

/// Records what the app asked the player to do.
#[derive(Default)]
pub struct FakePlayer {
    pub calls: Arc<Mutex<Vec<String>>>,
    loads: LoadId,
}

#[async_trait]
impl Player for FakePlayer {
    fn load(&mut self, source: &str) -> LoadId {
        self.loads += 1;
        self.calls.lock().unwrap().push(format!("load {source}"));
        self.loads
    }
    fn set_pause(&mut self, paused: bool) {
        self.calls.lock().unwrap().push(format!("pause {paused}"));
    }
    fn seek(&mut self, position: Duration) {
        let secs = position.as_secs();
        self.calls.lock().unwrap().push(format!("seek {secs}"));
    }
    fn set_volume(&mut self, volume: u8) {
        self.calls.lock().unwrap().push(format!("volume {volume}"));
    }
    fn stop(&mut self) {
        self.calls.lock().unwrap().push("stop".into());
    }
    fn set_metering(&mut self, on: bool) {
        self.calls.lock().unwrap().push(format!("metering {on}"));
    }
    async fn shutdown(&mut self) {}
}
