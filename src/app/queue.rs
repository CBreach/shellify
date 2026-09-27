use rand::seq::SliceRandom;

use crate::app::action::RepeatMode;
use crate::provider::Track;

/// The play queue. Owned by the app (not mpv) so it stays provider-agnostic.
#[derive(Debug, Default)]
pub struct Queue {
    tracks: Vec<Track>,
    current: Option<usize>,
    pub repeat: RepeatMode,
}

impl Queue {
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<&Track> {
        self.current.map(|i| &self.tracks[i])
    }

    /// Replaces the queue with `tracks` and starts at `start`.
    pub fn play_list(&mut self, tracks: Vec<Track>, start: usize) -> Option<&Track> {
        self.current = (start < tracks.len()).then_some(start);
        self.tracks = tracks;
        self.current()
    }

    /// Jumps to an existing queue entry.
    pub fn jump(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.current = Some(index);
        }
        self.current()
    }

    pub fn push(&mut self, track: Track) {
        self.tracks.push(track);
    }

    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current = None;
    }

    /// Moves to the next track. `auto` is true when the current track ended by
    /// itself, in which case repeat-one replays it; a manual skip always moves on.
    pub fn advance(&mut self, auto: bool) -> Option<&Track> {
        let cur = self.current?;
        let next = match self.repeat {
            RepeatMode::One if auto => Some(cur),
            _ if cur + 1 < self.tracks.len() => Some(cur + 1),
            RepeatMode::Off => None,
            RepeatMode::All | RepeatMode::One => Some(0),
        };
        self.current = next;
        self.current()
    }

    pub fn back(&mut self) -> Option<&Track> {
        let cur = self.current?;
        self.current = match (cur, self.repeat) {
            (0, RepeatMode::All) => Some(self.tracks.len() - 1),
            (0, _) => Some(0),
            _ => Some(cur - 1),
        };
        self.current()
    }

    /// Shuffles the tracks after the current one (or all, if nothing is playing).
    pub fn shuffle(&mut self) {
        let start = self.current.map_or(0, |i| i + 1);
        self.tracks[start..].shuffle(&mut rand::rng());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| Track {
                id: i.to_string(),
                title: format!("t{i}"),
                artist: "a".into(),
                duration: Duration::from_secs(60),
                source: None,
            })
            .collect()
    }

    fn current_id(q: &Queue) -> Option<&str> {
        q.current().map(|t| t.id.as_str())
    }

    #[test]
    fn advance_stops_at_end_without_repeat() {
        let mut q = Queue::default();
        q.play_list(tracks(2), 1);
        assert!(q.advance(true).is_none());
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn repeat_all_wraps_both_ways() {
        let mut q = Queue {
            repeat: RepeatMode::All,
            ..Default::default()
        };
        q.play_list(tracks(3), 2);
        q.advance(false);
        assert_eq!(current_id(&q), Some("0"));
        q.back();
        assert_eq!(current_id(&q), Some("2"));
    }

    #[test]
    fn repeat_one_replays_on_eof_but_skips_manually() {
        let mut q = Queue {
            repeat: RepeatMode::One,
            ..Default::default()
        };
        q.play_list(tracks(3), 1);
        q.advance(true);
        assert_eq!(current_id(&q), Some("1"));
        q.advance(false);
        assert_eq!(current_id(&q), Some("2"));
    }

    #[test]
    fn back_at_start_stays_without_repeat() {
        let mut q = Queue::default();
        q.play_list(tracks(3), 0);
        q.back();
        assert_eq!(current_id(&q), Some("0"));
    }

    #[test]
    fn shuffle_keeps_played_tracks_in_place() {
        let mut q = Queue::default();
        q.play_list(tracks(50), 2);
        q.shuffle();
        let ids: Vec<_> = q.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(&ids[..3], ["0", "1", "2"]);
        assert_eq!(current_id(&q), Some("2"));
        let mut rest: Vec<_> = ids[3..]
            .iter()
            .map(|s| s.parse::<usize>().unwrap())
            .collect();
        rest.sort();
        assert_eq!(rest, (3..50).collect::<Vec<_>>());
    }
}
