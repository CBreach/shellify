mod cmdline;
mod library;
mod now_playing;
mod queue;
mod tracks;

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType};

use crate::app::action::Pane;
use crate::app::state::AppState;

const ACCENT: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, state: &mut AppState) {
    let [main, now_playing, cmdline] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [library, tracks, queue] = Layout::horizontal([
        Constraint::Percentage(22),
        Constraint::Percentage(50),
        Constraint::Percentage(28),
    ])
    .areas(main);

    library::draw(frame, library, state);
    tracks::draw(frame, tracks, state);
    queue::draw(frame, queue, state);
    now_playing::draw(frame, now_playing, state);
    cmdline::draw(frame, cmdline, state);
}

/// A bordered pane, highlighted when focused.
fn pane_block(title: String, pane: Pane, state: &AppState) -> Block<'static> {
    let focused = state.focus == pane;
    let border = if focused {
        Style::new().fg(ACCENT)
    } else {
        Style::new().fg(Color::DarkGray)
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(format!(" {title} "))
}

fn highlight_style(pane: Pane, state: &AppState) -> Style {
    if state.focus == pane {
        Style::new()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::REVERSED)
    }
}

/// `m:ss`, or `h:mm:ss` for long tracks.
fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(fmt_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(fmt_duration(Duration::from_secs(224)), "3:44");
        assert_eq!(fmt_duration(Duration::from_secs(3723)), "1:02:03");
    }
}
