use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{List, ListItem};

use super::{highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let current = state.queue.current_index();
    let items: Vec<ListItem> = state
        .queue
        .tracks()
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let playing = current == Some(i);
            let marker = if playing { "♪ " } else { "  " };
            let item = ListItem::new(format!("{marker}{} — {}", t.title, t.artist));
            if playing {
                item.style(Style::new().fg(theme.accent))
            } else {
                item
            }
        })
        .collect();
    let title = format!("Queue ({})", state.queue.tracks().len());
    let list = List::new(items)
        .block(pane_block(title, Pane::Queue, state, theme))
        .highlight_style(highlight_style(Pane::Queue, state, theme));
    frame.render_stateful_widget(list, area, &mut state.queue_state);
}
