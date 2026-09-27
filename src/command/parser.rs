use std::time::Duration;

use crate::app::action::{
    Action, Focus, Pane, RepeatMode, Resize, Seek, Select, ThemeCommand, View, Volume,
};

pub struct CommandInfo {
    pub name: &'static str,
    pub usage: &'static str,
    pub about: &'static str,
}

/// Every command, sorted by name. Drives tab completion and the help overlay.
pub const COMMANDS: &[CommandInfo] = &[
    cmd("add", "add", "Add the selection to the queue"),
    cmd("clear", "clear", "Clear the queue"),
    cmd(
        "focus",
        "focus <next|prev|library|tracks|queue>",
        "Move focus to a pane",
    ),
    cmd("help", "help", "Show this help"),
    cmd("next", "next", "Next track"),
    cmd("open", "open <url|path>", "Play a URL or local file"),
    cmd("pause", "pause", "Toggle pause"),
    cmd(
        "play",
        "play [query]",
        "Play the selection, or search and play",
    ),
    cmd(
        "prev",
        "prev",
        "Previous track (or restart the current one)",
    ),
    cmd("queue", "queue", "Focus the queue"),
    cmd("quit", "q, quit", "Quit Shellify"),
    cmd(
        "repeat",
        "repeat [off|all|one]",
        "Set repeat mode, or cycle it",
    ),
    cmd(
        "resize",
        "resize <library|queue> <N|+N|-N>, resize reset",
        "Set a side pane's width in % of the window",
    ),
    cmd("search", "search [query]", "Search, or open the / prompt"),
    cmd(
        "seek",
        "seek <1:30|90|+10|-10>",
        "Seek to a time or by an offset",
    ),
    cmd("select", "select <+N|-N|top|bottom>", "Move the selection"),
    cmd("settings", "settings", "Open the Settings tab"),
    cmd("shuffle", "shuffle", "Shuffle upcoming tracks"),
    cmd(
        "theme",
        "theme <name>, theme import <file>, theme reload",
        "Switch, import (.toml or base16 .yaml) or reload themes",
    ),
    cmd("view", "view <music|settings>", "Switch tab"),
    cmd(
        "volume",
        "vol, volume <0-100|+N|-N>",
        "Set or change the volume",
    ),
];

const fn cmd(name: &'static str, usage: &'static str, about: &'static str) -> CommandInfo {
    CommandInfo { name, usage, about }
}

/// Parses a command-mode line (without the leading `:`) into an [`Action`].
pub fn parse(input: &str) -> Result<Action, String> {
    let input = input.trim();
    let (name, arg) = match input.split_once(char::is_whitespace) {
        Some((name, arg)) => (name, arg.trim()),
        None => (input, ""),
    };

    let no_arg = |action: Action| {
        if arg.is_empty() {
            Ok(action)
        } else {
            Err(format!("{name}: takes no arguments"))
        }
    };

    match name {
        "" => Err("empty command".into()),
        "q" | "quit" => no_arg(Action::Quit),
        "help" => no_arg(Action::Help),
        "settings" => no_arg(Action::View(View::Settings)),
        "resize" => parse_resize(arg).map(Action::Resize),
        "theme" => parse_theme(arg).map(Action::Theme),
        "view" => match arg {
            "music" => Ok(Action::View(View::Music)),
            "settings" => Ok(Action::View(View::Settings)),
            _ => Err(format!("view: expected music or settings, got {arg:?}")),
        },
        "pause" => no_arg(Action::TogglePause),
        "next" => no_arg(Action::Next),
        "prev" => no_arg(Action::Prev),
        "add" => no_arg(Action::AddSelected),
        "clear" => no_arg(Action::ClearQueue),
        "shuffle" => no_arg(Action::Shuffle),
        "queue" => no_arg(Action::Focus(Focus::Pane(Pane::Queue))),
        "play" if arg.is_empty() => Ok(Action::PlaySelected),
        "play" => Ok(Action::PlayQuery(arg.to_string())),
        "search" if arg.is_empty() => Ok(Action::OpenSearch),
        "search" => Ok(Action::Search(arg.to_string())),
        "open" if arg.is_empty() => Err("open: expected a URL or file path".into()),
        "open" => Ok(Action::Open(arg.to_string())),
        "seek" => parse_seek(arg).map(Action::Seek),
        "vol" | "volume" => parse_volume(arg).map(Action::Volume),
        "repeat" => parse_repeat(arg).map(Action::Repeat),
        "select" => parse_select(arg).map(Action::Select),
        "focus" => parse_focus(arg).map(Action::Focus),
        _ => Err(format!("unknown command: {name}")),
    }
}

fn parse_seek(arg: &str) -> Result<Seek, String> {
    let err = || format!("seek: expected 1:30, 90, +10 or -10, got {arg:?}");
    if let Some(rest) = arg.strip_prefix('+') {
        parse_time(rest).map(Seek::Forward).ok_or_else(err)
    } else if let Some(rest) = arg.strip_prefix('-') {
        parse_time(rest).map(Seek::Back).ok_or_else(err)
    } else {
        parse_time(arg).map(Seek::To).ok_or_else(err)
    }
}

/// Parses `90`, `1:30` or `1:02:03` into a duration.
fn parse_time(s: &str) -> Option<Duration> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut secs: u64 = 0;
    for (i, part) in parts.iter().enumerate() {
        let n: u64 = part.parse().ok()?;
        // Everything after the leading component is minutes/seconds.
        if i > 0 && n >= 60 {
            return None;
        }
        secs = secs * 60 + n;
    }
    Some(Duration::from_secs(secs))
}

fn parse_volume(arg: &str) -> Result<Volume, String> {
    let err = || format!("volume: expected 0-100, +N or -N, got {arg:?}");
    if arg.starts_with(['+', '-']) {
        arg.parse::<i16>().map(Volume::Change).map_err(|_| err())
    } else {
        match arg.parse::<u8>() {
            Ok(v) if v <= 100 => Ok(Volume::Set(v)),
            _ => Err(err()),
        }
    }
}

fn parse_theme(arg: &str) -> Result<ThemeCommand, String> {
    match arg.split_once(char::is_whitespace) {
        Some(("import", path)) if !path.trim().is_empty() => {
            Ok(ThemeCommand::Import(path.trim().to_string()))
        }
        _ if arg == "import" => Err("theme import: expected a file path".into()),
        _ if arg == "reload" => Ok(ThemeCommand::Reload),
        _ if arg.is_empty() || arg.contains(char::is_whitespace) => {
            Err("theme: expected a theme name, `import <file>` or `reload`".into())
        }
        _ => Ok(ThemeCommand::Use(arg.to_lowercase())),
    }
}

fn parse_resize(arg: &str) -> Result<Resize, String> {
    let err = || format!("resize: expected `library|queue N`, `+N`, `-N` or `reset`, got {arg:?}");
    if arg == "reset" {
        return Ok(Resize::Reset);
    }
    let (pane, amount) = arg.split_once(char::is_whitespace).ok_or_else(err)?;
    let pane = match pane {
        "library" => Pane::Library,
        "queue" => Pane::Queue,
        _ => return Err(err()),
    };
    let amount = amount.trim();
    if amount.starts_with(['+', '-']) {
        amount
            .parse()
            .map(|d| Resize::Change(pane, d))
            .map_err(|_| err())
    } else {
        amount
            .parse()
            .map(|n| Resize::Set(pane, n))
            .map_err(|_| err())
    }
}

fn parse_repeat(arg: &str) -> Result<Option<RepeatMode>, String> {
    match arg {
        "" => Ok(None),
        "off" => Ok(Some(RepeatMode::Off)),
        "all" => Ok(Some(RepeatMode::All)),
        "one" => Ok(Some(RepeatMode::One)),
        _ => Err(format!("repeat: expected off, all or one, got {arg:?}")),
    }
}

fn parse_select(arg: &str) -> Result<Select, String> {
    match arg {
        "top" => Ok(Select::First),
        "bottom" => Ok(Select::Last),
        _ => arg
            .parse::<i32>()
            .map(Select::By)
            .map_err(|_| format!("select: expected +N, -N, top or bottom, got {arg:?}")),
    }
}

fn parse_focus(arg: &str) -> Result<Focus, String> {
    match arg {
        "next" => Ok(Focus::Next),
        "prev" => Ok(Focus::Prev),
        "library" => Ok(Focus::Pane(Pane::Library)),
        "tracks" => Ok(Focus::Pane(Pane::Tracks)),
        "queue" => Ok(Focus::Pane(Pane::Queue)),
        _ => Err(format!(
            "focus: expected next, prev, library, tracks or queue, got {arg:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn parse_seek_forms() {
        assert_eq!(parse("seek 1:30"), Ok(Action::Seek(Seek::To(secs(90)))));
        assert_eq!(parse("seek 90"), Ok(Action::Seek(Seek::To(secs(90)))));
        assert_eq!(
            parse("seek 1:02:03"),
            Ok(Action::Seek(Seek::To(secs(3723))))
        );
        assert_eq!(parse("seek +10"), Ok(Action::Seek(Seek::Forward(secs(10)))));
        assert_eq!(parse("seek -0:05"), Ok(Action::Seek(Seek::Back(secs(5)))));
        assert!(parse("seek 1:75").is_err());
        assert!(parse("seek").is_err());
    }

    #[test]
    fn parse_volume_forms() {
        assert_eq!(parse("vol 60"), Ok(Action::Volume(Volume::Set(60))));
        assert_eq!(parse("volume +5"), Ok(Action::Volume(Volume::Change(5))));
        assert_eq!(parse("vol -15"), Ok(Action::Volume(Volume::Change(-15))));
        assert!(parse("vol 101").is_err());
        assert!(parse("vol loud").is_err());
    }

    #[test]
    fn parse_open_requires_a_target() {
        assert_eq!(
            parse("open ~/Music/song one.mp3"),
            Ok(Action::Open("~/Music/song one.mp3".into()))
        );
        assert!(parse("open").is_err());
    }

    #[test]
    fn parse_play_and_search_with_and_without_query() {
        assert_eq!(parse("play"), Ok(Action::PlaySelected));
        assert_eq!(
            parse("play  daft punk "),
            Ok(Action::PlayQuery("daft punk".into()))
        );
        assert_eq!(parse("search"), Ok(Action::OpenSearch));
        assert_eq!(parse("search lofi"), Ok(Action::Search("lofi".into())));
    }

    #[test]
    fn parse_simple_commands() {
        assert_eq!(parse("q"), Ok(Action::Quit));
        assert_eq!(parse("  next "), Ok(Action::Next));
        assert_eq!(parse("repeat"), Ok(Action::Repeat(None)));
        assert_eq!(
            parse("repeat one"),
            Ok(Action::Repeat(Some(RepeatMode::One)))
        );
        assert_eq!(parse("select -1"), Ok(Action::Select(Select::By(-1))));
        assert_eq!(parse("select bottom"), Ok(Action::Select(Select::Last)));
        assert_eq!(
            parse("focus queue"),
            Ok(Action::Focus(Focus::Pane(Pane::Queue)))
        );
        assert_eq!(parse("settings"), Ok(Action::View(View::Settings)));
        assert_eq!(parse("resize reset"), Ok(Action::Resize(Resize::Reset)));
        assert_eq!(
            parse("theme Nord"),
            Ok(Action::Theme(ThemeCommand::Use("nord".into())))
        );
        assert_eq!(
            parse("theme import ~/Downloads/tokyo night.yaml"),
            Ok(Action::Theme(ThemeCommand::Import(
                "~/Downloads/tokyo night.yaml".into()
            )))
        );
        assert_eq!(
            parse("theme reload"),
            Ok(Action::Theme(ThemeCommand::Reload))
        );
        assert!(parse("theme").is_err());
        assert!(parse("theme import").is_err());
        assert_eq!(
            parse("resize library 30"),
            Ok(Action::Resize(Resize::Set(Pane::Library, 30)))
        );
        assert_eq!(
            parse("resize queue -5"),
            Ok(Action::Resize(Resize::Change(Pane::Queue, -5)))
        );
        assert!(parse("resize tracks 50").is_err());
        assert!(parse("resize library").is_err());
        assert_eq!(parse("view music"), Ok(Action::View(View::Music)));
        assert!(parse("view mixtape").is_err());
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse("").is_err());
        assert!(parse("dance").is_err());
        assert!(parse("next please").is_err());
    }

    #[test]
    fn completion_list_is_sorted() {
        assert!(COMMANDS.windows(2).all(|w| w[0].name < w[1].name));
    }
}
