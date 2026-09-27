use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Row, Table};

use super::{fmt_duration, highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::state::AppState;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let playing_id = state.queue.current().map(|t| t.id.clone());
    let rows: Vec<Row> = state
        .tracks
        .iter()
        .map(|t| {
            let row = Row::new([t.title.clone(), t.artist.clone(), fmt_duration(t.duration)]);
            if playing_id.as_deref() == Some(t.id.as_str()) {
                row.style(Style::new().fg(super::ACCENT))
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Percentage(55),
        Constraint::Percentage(35),
        Constraint::Length(8),
    ];
    let header = Row::new(["Title", "Artist", "Time"]).style(
        Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let title = format!("{} ({})", state.tracks_title, state.tracks.len());
    let table = Table::new(rows, widths)
        .header(header)
        .block(pane_block(title, Pane::Tracks, state))
        .row_highlight_style(highlight_style(Pane::Tracks, state));
    frame.render_stateful_widget(table, area, &mut state.tracks_state);
}
