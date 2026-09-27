use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};

use super::text::truncate;
use super::{fmt_duration, highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::state::AppState;
use crate::ui::theme::Theme;

const COLUMN_SPACING: u16 = 1;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let total: Duration = state.tracks.iter().map(|t| t.duration).sum();
    let title = format!(
        "{} {sep} {} tracks {sep} {} min",
        state.tracks_title,
        state.tracks.len(),
        total.as_secs() / 60,
        sep = theme.icons.sep,
    );
    let block = pane_block(title, Pane::Tracks, state, theme);

    if state.tracks.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from("Nothing here"),
            Line::from("Press / to search, or pick a playlist"),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(Style::new().fg(theme.muted));
        frame.render_widget(msg, inner);
        return;
    }

    let widths = [
        Constraint::Length(3),
        Constraint::Fill(3),
        Constraint::Fill(2),
        Constraint::Length(7),
    ];
    // Resolve the columns ourselves so long titles can be cut with an ellipsis
    // instead of being clipped mid-word by the table.
    let inner_width = area.width.saturating_sub(2);
    let cols = Layout::horizontal(widths)
        .spacing(COLUMN_SPACING)
        .split(Rect::new(0, 0, inner_width, 1));
    let (title_w, artist_w) = (cols[1].width as usize, cols[2].width as usize);

    let current = state.queue.current().map(|t| t.id.clone());
    let paused = state.playback.paused;
    let rows: Vec<Row> = state
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let playing = current.as_deref() == Some(t.id.as_str());
            let marker = match (playing, paused) {
                (true, false) => theme.icons.playing.to_string(),
                (true, true) => theme.icons.paused.to_string(),
                _ => (i + 1).to_string(),
            };
            let marker_style = if playing {
                Style::new().fg(theme.accent)
            } else {
                Style::new().fg(theme.muted)
            };
            let row = Row::new([
                Cell::from(Line::from(marker).right_aligned()).style(marker_style),
                Cell::from(truncate(&t.title, title_w, theme.icons.ellipsis).into_owned()),
                Cell::from(truncate(&t.artist, artist_w, theme.icons.ellipsis).into_owned()),
                Cell::from(Line::from(fmt_duration(t.duration)).right_aligned())
                    .style(Style::new().fg(theme.muted)),
            ]);
            if playing {
                row.style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD))
            } else {
                row
            }
        })
        .collect();

    let header = Row::new([
        Cell::from(Line::from("#").right_aligned()),
        Cell::from("Title"),
        Cell::from("Artist"),
        Cell::from(Line::from("Time").right_aligned()),
    ])
    .style(Style::new().fg(theme.muted).add_modifier(Modifier::BOLD));

    let table = Table::new(rows, widths)
        .column_spacing(COLUMN_SPACING)
        .header(header)
        .block(block)
        .row_highlight_style(highlight_style(Pane::Tracks, state, theme));
    frame.render_stateful_widget(table, area, &mut state.tracks_state);
}
