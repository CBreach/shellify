mod cmdline;
mod header;
mod help;
pub mod icons;
pub mod layout;
mod library;
mod now_playing;
mod queue;
mod settings;
mod text;
pub mod theme;
mod tracks;

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::action::{Pane, View};
use crate::app::state::AppState;
use layout::Screen;
use theme::Theme;

pub fn draw(frame: &mut Frame, state: &mut AppState, theme: &Theme) {
    // Base style so `text` applies everywhere widgets don't override it.
    frame.render_widget(
        Block::new().style(Style::new().fg(theme.text)),
        frame.area(),
    );

    let areas = match layout::compute(frame.area(), state.focus) {
        Screen::Normal(areas) => areas,
        Screen::TooSmall => {
            draw_too_small(frame, theme);
            return;
        }
    };

    header::draw(frame, areas.header, state, theme, areas.narrow);
    match state.view {
        View::Settings => settings::draw(frame, areas.main, state, theme),
        View::Music => {
            for &(pane, area) in &areas.panes {
                match pane {
                    Pane::Library => library::draw(frame, area, state, theme),
                    Pane::Tracks => tracks::draw(frame, area, state, theme),
                    Pane::Queue => queue::draw(frame, area, state, theme),
                }
            }
        }
    }
    now_playing::draw(frame, areas.now_playing, state, theme);
    cmdline::draw(frame, areas.cmdline, state, theme);
    if state.help_open {
        help::draw(frame, state, theme);
    }
}

/// Shown instead of a garbled layout when the window can't fit the UI.
fn draw_too_small(frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    let msg = Paragraph::new(vec![
        Line::from("Terminal too small").style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(format!(
            "need {}x{}, have {}x{}",
            layout::MIN_WIDTH,
            layout::MIN_HEIGHT,
            area.width,
            area.height
        ))
        .style(Style::new().fg(theme.muted)),
    ])
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    let [middle] = Layout::vertical([Constraint::Length(2)])
        .flex(Flex::Center)
        .areas(area);
    frame.render_widget(msg, middle);
}

/// A bordered pane, highlighted when focused.
fn pane_block(title: String, pane: Pane, state: &AppState, theme: &Theme) -> Block<'static> {
    let focused = state.focus == pane;
    let (border, title_style) = if focused {
        let accent = Style::new().fg(theme.accent);
        (accent, accent.add_modifier(Modifier::BOLD))
    } else {
        (Style::new().fg(theme.muted), Style::new().fg(theme.text))
    };
    // Without color, a heavier border marks the focused pane.
    let border_set = if focused && theme.mono {
        theme.icons.border_focus
    } else {
        theme.icons.border
    };
    Block::bordered()
        .border_set(border_set)
        .border_style(border)
        .title(Line::from(format!(" {title} ")).style(title_style))
}

fn highlight_style(pane: Pane, state: &AppState, theme: &Theme) -> Style {
    if state.focus == pane {
        theme.selection()
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::provider::{Playlist, Track};

    fn render(w: u16, h: u16, theme: &Theme) -> String {
        let track = Track {
            id: "1".into(),
            title: "A Rather Long Song Title For Truncation".into(),
            artist: "Some Artist".into(),
            duration: Duration::from_secs(200),
            source: None,
        };
        let mut state = AppState::new(vec![Playlist {
            name: "Liked Songs".into(),
            tracks: vec![track.clone()],
        }]);
        state.queue.play_list(vec![track], 0);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, &mut state, theme)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .chunks(w as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_at_every_contract_size() {
        let theme = Theme::default();
        for (w, h) in [(160, 48), (100, 30), (80, 24), (60, 24), (40, 12)] {
            let screen = render(w, h, &theme);
            assert!(screen.contains("Now Playing"), "{w}x{h}:\n{screen}");
            assert!(screen.contains("Music"), "{w}x{h}");
        }
    }

    #[test]
    fn narrow_header_lists_panes_and_tiny_shows_message() {
        let theme = Theme::default();
        assert!(render(60, 24, &theme).contains("Library  Tracks  Queue"));
        assert!(!render(100, 30, &theme).contains("Library  Tracks  Queue"));
        for (w, h) in [(39, 24), (80, 11), (10, 3)] {
            assert!(render(w, h, &theme).contains("too small"), "{w}x{h}");
        }
    }

    #[test]
    fn settings_tab_renders_and_keeps_selection_visible() {
        let theme = Theme::default();
        let mut state = AppState::new(Vec::new());
        state.view = View::Settings;
        for (w, h) in [(100, 30), (60, 24), (40, 12)] {
            state.settings_cursor = crate::app::settings::SettingRow::ALL.len() - 1;
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &mut state, &theme)).unwrap();
            let screen: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(screen.contains("Settings"), "{w}x{h}");
            assert!(
                screen.contains("Reset all settings"),
                "{w}x{h}: selected row scrolled off"
            );
        }
    }

    #[test]
    fn ascii_pack_renders_only_ascii() {
        let theme = Theme {
            icons: icons::IconPack::Ascii.icons(),
            ..Theme::default()
        };
        for (w, h) in [(100, 30), (60, 24)] {
            let screen = render(w, h, &theme);
            let bad: String = screen.chars().filter(|c| !c.is_ascii()).collect();
            assert!(bad.is_empty(), "{w}x{h} non-ascii: {bad:?}");
        }
    }

    #[test]
    fn formats_durations() {
        assert_eq!(fmt_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(fmt_duration(Duration::from_secs(224)), "3:44");
        assert_eq!(fmt_duration(Duration::from_secs(3723)), "1:02:03");
    }
}
