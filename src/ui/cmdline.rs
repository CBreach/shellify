use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::state::{AppState, Mode, StatusLevel};

/// The bottom line: the `:`/`/` prompt while typing, otherwise mode + status.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let (prefix, editor) = match state.mode {
        Mode::Command => (":", &state.command_line),
        Mode::Search => ("/", &state.search_line),
        Mode::Normal => {
            let mode = Span::styled(
                " NORMAL ",
                Style::new()
                    .fg(Color::Black)
                    .bg(super::ACCENT)
                    .add_modifier(Modifier::BOLD),
            );
            let status = match &state.status {
                Some(s) if s.level == StatusLevel::Error => {
                    Span::styled(s.text.clone(), Style::new().fg(Color::Red))
                }
                Some(s) => Span::raw(s.text.clone()),
                None => Span::raw(""),
            };
            frame.render_widget(Line::from(vec![mode, Span::raw(" "), status]), area);
            return;
        }
    };

    frame.render_widget(Line::from(format!("{prefix}{}", editor.text())), area);
    // Hints such as tab-completion candidates show on the right while typing.
    if let Some(status) = &state.status {
        let hint = Line::from(status.text.clone())
            .style(Style::new().fg(Color::DarkGray))
            .right_aligned();
        frame.render_widget(hint, area);
    }
    // Cursor sits after the prefix; one cell per char is fine for typical input.
    let x = area.x + 1 + editor.cursor() as u16;
    frame.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
}
