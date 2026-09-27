use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph, Wrap};

use super::text::truncate;
use super::{highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let title = format!("Queue {} {}", theme.icons.sep, state.queue.tracks().len());
    let block = pane_block(title, Pane::Queue, state, theme);

    if state.queue.tracks().is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let msg = Paragraph::new(vec![
            Line::from(""),
            Line::from("Queue is empty"),
            Line::from("Press a to add the selection"),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(Style::new().fg(theme.muted));
        frame.render_widget(msg, inner);
        return;
    }

    // Two lines per entry (title, then artist) reads better in a narrow pane.
    let text_width = (area.width as usize).saturating_sub(2 + 3);
    let current = state.queue.current_index();
    let items: Vec<ListItem> = state
        .queue
        .tracks()
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let playing = current == Some(i);
            let (marker, title_style) = if playing {
                (
                    Span::styled(
                        format!(" {} ", theme.icons.current),
                        Style::new().fg(theme.accent),
                    ),
                    Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
                )
            } else {
                (
                    Span::styled(format!("{:>2} ", i + 1), Style::new().fg(theme.muted)),
                    Style::new(),
                )
            };
            ListItem::new(vec![
                Line::from(vec![
                    marker,
                    Span::styled(
                        truncate(&t.title, text_width, theme.icons.ellipsis).into_owned(),
                        title_style,
                    ),
                ]),
                Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        truncate(&t.artist, text_width, theme.icons.ellipsis).into_owned(),
                        Style::new().fg(theme.muted),
                    ),
                ]),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(highlight_style(Pane::Queue, state, theme));
    frame.render_stateful_widget(list, area, &mut state.queue_state);
}
