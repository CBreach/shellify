//! Normal-mode key bindings. Every binding maps to a command string, parsed
//! by the same parser as `:` command mode.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::action::Action;
use crate::command;

const DEFAULT_BINDINGS: &[(&str, &str)] = &[
    ("j", "select +1"),
    ("down", "select +1"),
    ("k", "select -1"),
    ("up", "select -1"),
    ("gg", "select top"),
    ("G", "select bottom"),
    ("l", "focus next"),
    ("tab", "focus next"),
    ("h", "focus prev"),
    ("backtab", "focus prev"),
    ("enter", "play"),
    ("space", "pause"),
    ("n", "next"),
    ("p", "prev"),
    ("right", "seek +5"),
    ("left", "seek -5"),
    ("+", "vol +5"),
    ("=", "vol +5"),
    ("-", "vol -5"),
    ("a", "add"),
    ("s", "shuffle"),
    ("r", "repeat"),
    ("/", "search"),
    ("?", "help"),
    ("v", "visualizer"),
    ("V", "visualizer next"),
    ("1", "view music"),
    ("2", "view settings"),
    ("q", "quit"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    code: KeyCode,
    mods: KeyModifiers,
}

impl KeyPress {
    pub fn from_event(ev: KeyEvent) -> Self {
        let mut mods =
            ev.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        // Shift is already reflected in the character ('G') and in BackTab.
        if matches!(ev.code, KeyCode::Char(_) | KeyCode::BackTab) {
            mods.remove(KeyModifiers::SHIFT);
        }
        Self {
            code: ev.code,
            mods,
        }
    }
}

pub enum KeyResult {
    Action(Action),
    /// The key starts a longer binding (e.g. the first `g` of `gg`).
    Pending,
    Unbound,
}

pub struct Keymap {
    bindings: HashMap<Vec<KeyPress>, Action>,
    /// `(keys, spec, command)` in definition order, for help and hints.
    listing: Vec<(Vec<KeyPress>, String, String)>,
    pending: Vec<KeyPress>,
}

/// One line of the help overlay: all keys bound to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpEntry {
    pub keys: String,
    pub description: String,
}

impl Keymap {
    /// Builds the default keymap with `overrides` (key spec -> command) applied.
    /// An empty command removes the binding.
    pub fn new(overrides: &HashMap<String, String>) -> Result<Self> {
        let mut bindings = HashMap::new();
        let mut listing: Vec<(Vec<KeyPress>, String, String)> = Vec::new();
        let defaults = DEFAULT_BINDINGS.iter().copied();
        // Sorted so help lists user bindings in a stable order.
        let mut user: Vec<_> = overrides.iter().collect();
        user.sort();
        let user = user.into_iter().map(|(k, c)| (k.as_str(), c.as_str()));
        for (spec, cmd) in defaults.chain(user) {
            let keys = parse_key_spec(spec).map_err(|e| anyhow!("key {spec:?}: {e}"))?;
            let cmd = cmd.trim();
            if cmd.is_empty() {
                bindings.remove(&keys);
                listing.retain(|(k, _, _)| *k != keys);
                continue;
            }
            let action = command::parse(cmd).map_err(|e| anyhow!("key {spec:?}: {e}"))?;
            bindings.insert(keys.clone(), action);
            match listing.iter_mut().find(|(k, _, _)| *k == keys) {
                Some(entry) => entry.2 = cmd.to_string(),
                None => listing.push((keys, spec.to_string(), cmd.to_string())),
            }
        }
        Ok(Self {
            bindings,
            listing,
            pending: Vec::new(),
        })
    }

    /// Bindings grouped by command, in definition order: `j ↓  Move down`.
    pub fn help_entries(&self) -> Vec<HelpEntry> {
        let mut entries: Vec<(String, Vec<String>)> = Vec::new();
        for (_, spec, cmd) in &self.listing {
            match entries.iter_mut().find(|(c, _)| c == cmd) {
                Some((_, keys)) => keys.push(pretty_spec(spec)),
                None => entries.push((cmd.clone(), vec![pretty_spec(spec)])),
            }
        }
        entries
            .into_iter()
            .map(|(cmd, keys)| HelpEntry {
                keys: keys.join(" "),
                description: describe(&cmd),
            })
            .collect()
    }

    /// The first key bound to `cmd`, for footer hints.
    pub fn key_for(&self, cmd: &str) -> Option<String> {
        self.listing
            .iter()
            .find(|(_, _, c)| c == cmd)
            .map(|(_, spec, _)| pretty_spec(spec))
    }

    pub fn press(&mut self, key: KeyPress) -> KeyResult {
        self.pending.push(key);
        if let Some(action) = self.bindings.get(&self.pending) {
            self.pending.clear();
            return KeyResult::Action(action.clone());
        }
        if self.bindings.keys().any(|k| k.starts_with(&self.pending)) {
            return KeyResult::Pending;
        }
        // Not a valid sequence: retry the last key on its own (so `gj` still moves).
        let had_prefix = self.pending.len() > 1;
        self.pending.clear();
        if had_prefix {
            self.press(key)
        } else {
            KeyResult::Unbound
        }
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

/// Human-readable label for a bound command; unknown ones show as `:cmd`.
fn describe(cmd: &str) -> String {
    let label = match cmd {
        "select +1" => "Move down",
        "select -1" => "Move up",
        "select top" => "Jump to top",
        "select bottom" => "Jump to bottom",
        "focus next" => "Next pane",
        "focus prev" => "Previous pane",
        "play" => "Play selection / open playlist",
        "pause" => "Play / pause",
        "next" => "Next track",
        "prev" => "Previous track",
        "seek +5" => "Seek forward 5s",
        "seek -5" => "Seek back 5s",
        "vol +5" => "Volume up",
        "vol -5" => "Volume down",
        "add" => "Add selection to queue",
        "shuffle" => "Shuffle queue",
        "repeat" => "Cycle repeat (off/all/one)",
        "search" => "Search",
        "help" => "Show this help",
        "visualizer" => "Toggle the visualizer",
        "visualizer next" => "Next visualizer style",
        "view music" => "Music tab",
        "view settings" => "Settings tab",
        "quit" => "Quit",
        other => return format!(":{other}"),
    };
    label.to_string()
}

/// Display form of a key spec: `"down"` → `↓`, `"ctrl-d"` → `C-d`.
fn pretty_spec(spec: &str) -> String {
    spec.split_whitespace()
        .map(|token| {
            let mut out = String::new();
            let mut rest = token;
            while let Some((prefix, tail)) = rest.split_once('-').filter(|(_, t)| !t.is_empty()) {
                out.push_str(match prefix {
                    "ctrl" | "c" => "C-",
                    "alt" | "a" | "m" => "M-",
                    "shift" | "s" => "S-",
                    other => other,
                });
                rest = tail;
            }
            out.push_str(match rest {
                "down" => "↓",
                "up" => "↑",
                "left" => "←",
                "right" => "→",
                "space" => "Space",
                "enter" => "Enter",
                "tab" => "Tab",
                "backtab" => "S-Tab",
                "esc" => "Esc",
                "backspace" => "Bksp",
                "delete" => "Del",
                "pageup" => "PgUp",
                "pagedown" => "PgDn",
                "home" => "Home",
                "end" => "End",
                other => other,
            });
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses `"j"`, `"gg"`, `"G"`, `"space"`, `"ctrl-d"` or `"g g"` into a key sequence.
fn parse_key_spec(spec: &str) -> Result<Vec<KeyPress>> {
    let mut keys = Vec::new();
    for token in spec.split_whitespace() {
        match parse_key(token) {
            Ok(key) => keys.push(key),
            // "gg" is not a key name: treat it as a sequence of plain chars.
            Err(_) if token.chars().count() > 1 && !token.contains('-') => {
                keys.extend(token.chars().map(|c| KeyPress {
                    code: KeyCode::Char(c),
                    mods: KeyModifiers::NONE,
                }));
            }
            Err(e) => return Err(e),
        }
    }
    if keys.is_empty() {
        bail!("empty key");
    }
    Ok(keys)
}

fn parse_key(token: &str) -> Result<KeyPress> {
    let mut mods = KeyModifiers::NONE;
    let mut rest = token;
    // Split off modifiers, but keep a lone "-" as the minus key.
    while let Some((prefix, tail)) = rest.split_once('-').filter(|(_, t)| !t.is_empty()) {
        mods |= match prefix {
            "ctrl" | "c" => KeyModifiers::CONTROL,
            "alt" | "a" | "m" => KeyModifiers::ALT,
            "shift" | "s" => KeyModifiers::SHIFT,
            _ => bail!("unknown modifier {prefix:?}"),
        };
        rest = tail;
    }
    let code = match rest {
        "space" => KeyCode::Char(' '),
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        _ => {
            let mut chars = rest.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => bail!("unknown key {rest:?}"),
            }
        }
    };
    Ok(KeyPress { code, mods })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::action::Select;

    fn key(c: char) -> KeyPress {
        KeyPress::from_event(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    fn action(r: KeyResult) -> Option<Action> {
        match r {
            KeyResult::Action(a) => Some(a),
            _ => None,
        }
    }

    #[test]
    fn default_single_key() {
        let mut km = Keymap::new(&HashMap::new()).unwrap();
        assert_eq!(action(km.press(key('n'))), Some(Action::Next));
    }

    #[test]
    fn gg_sequence_and_fallback() {
        let mut km = Keymap::new(&HashMap::new()).unwrap();
        assert!(matches!(km.press(key('g')), KeyResult::Pending));
        assert_eq!(
            action(km.press(key('g'))),
            Some(Action::Select(Select::First))
        );
        // `g` then `j` falls back to `j` alone.
        km.press(key('g'));
        assert_eq!(
            action(km.press(key('j'))),
            Some(Action::Select(Select::By(1)))
        );
    }

    #[test]
    fn shifted_char_matches_uppercase_binding() {
        let mut km = Keymap::new(&HashMap::new()).unwrap();
        let g = KeyPress::from_event(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(action(km.press(g)), Some(Action::Select(Select::Last)));
    }

    #[test]
    fn overrides_rebind_and_unbind() {
        let overrides = HashMap::from([
            ("ctrl-n".to_string(), "next".to_string()),
            ("n".to_string(), String::new()),
        ]);
        let mut km = Keymap::new(&overrides).unwrap();
        assert!(matches!(km.press(key('n')), KeyResult::Unbound));
        let ctrl_n = KeyPress::from_event(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(action(km.press(ctrl_n)), Some(Action::Next));
    }

    #[test]
    fn invalid_override_is_an_error() {
        let bad_cmd = HashMap::from([("x".to_string(), "dance".to_string())]);
        assert!(Keymap::new(&bad_cmd).is_err());
        let bad_key = HashMap::from([("hyper-x".to_string(), "next".to_string())]);
        assert!(Keymap::new(&bad_key).is_err());
    }

    #[test]
    fn help_groups_keys_by_command() {
        let km = Keymap::new(&HashMap::new()).unwrap();
        let entries = km.help_entries();
        let down = entries
            .iter()
            .find(|e| e.description == "Move down")
            .unwrap();
        assert_eq!(down.keys, "j ↓");
        assert_eq!(km.key_for("help").as_deref(), Some("?"));
    }

    #[test]
    fn help_reflects_overrides() {
        let overrides = HashMap::from([
            ("ctrl-n".to_string(), "next".to_string()),
            ("n".to_string(), String::new()),
            ("x".to_string(), "vol 50".to_string()),
        ]);
        let km = Keymap::new(&overrides).unwrap();
        let entries = km.help_entries();
        let next = entries
            .iter()
            .find(|e| e.description == "Next track")
            .unwrap();
        assert_eq!(next.keys, "C-n");
        assert!(
            entries
                .iter()
                .any(|e| e.keys == "x" && e.description == ":vol 50")
        );
    }

    #[test]
    fn minus_key_parses() {
        assert_eq!(
            parse_key_spec("-").unwrap(),
            vec![KeyPress {
                code: KeyCode::Char('-'),
                mods: KeyModifiers::NONE
            }]
        );
        assert_eq!(
            parse_key_spec("ctrl--").unwrap(),
            vec![KeyPress {
                code: KeyCode::Char('-'),
                mods: KeyModifiers::CONTROL
            }]
        );
    }
}
