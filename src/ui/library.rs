use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem};

use super::{highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::state::AppState;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let items: Vec<ListItem> = state
        .library
        .iter()
        .map(|p| ListItem::new(p.name.clone()))
        .collect();
    let list = List::new(items)
        .block(pane_block("Library".into(), Pane::Library, state))
        .highlight_style(highlight_style(Pane::Library, state));
    frame.render_stateful_widget(list, area, &mut state.library_state);
}
