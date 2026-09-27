//! Optional mouse-pointer shape over pane borders, via OSC 22
//! (`ESC ] 22 ; <shape> ST`). Terminals that implement it (kitty, foot,
//! Ghostty and others) show a left-right resize pointer; the rest ignore the
//! sequence, which is why this is opt-in (`[ui] resize_cursor`).
//!
//! We only ever send the "default" reset after we changed the pointer, so a
//! terminal that doesn't support OSC 22 never sees anything from us unless the
//! user turned the setting on.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

const RESIZE: &str = "\x1b]22;ew-resize\x1b\\";
const DEFAULT: &str = "\x1b]22;default\x1b\\";

/// Whether we've switched the pointer away from the default. Global so the
/// panic hook can restore it without access to the app.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// Shows the resize pointer (`true`) or the default one, writing only when
/// the state actually changes.
pub fn set_resize(on: bool) {
    let was = CHANGED.swap(on, Ordering::SeqCst);
    if let Some(seq) = transition(was, on) {
        let mut out = std::io::stdout();
        if let Err(e) = out.write_all(seq.as_bytes()).and_then(|()| out.flush()) {
            tracing::warn!("couldn't set the pointer shape: {e}");
        }
    }
}

/// Puts the default pointer back if we changed it (on quit and on panic).
pub fn restore() {
    set_resize(false);
}

/// Whether the resize pointer is currently shown (for tests).
#[cfg(test)]
pub fn is_resize() -> bool {
    CHANGED.load(Ordering::SeqCst)
}

/// The sequence to send when going from `was` to `now`, if any.
fn transition(was: bool, now: bool) -> Option<&'static str> {
    match (was, now) {
        (false, true) => Some(RESIZE),
        (true, false) => Some(DEFAULT),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_state_changes_emit_a_sequence() {
        assert_eq!(transition(false, true), Some("\x1b]22;ew-resize\x1b\\"));
        assert_eq!(transition(true, false), Some("\x1b]22;default\x1b\\"));
        assert_eq!(transition(true, true), None);
        assert_eq!(
            transition(false, false),
            None,
            "never reset a pointer we didn't change"
        );
    }
}
