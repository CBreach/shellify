use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem};
use unicode_width::UnicodeWidthStr;

use super::text::truncate;
use super::{highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let inner_width = (area.width as usize).saturating_sub(2);
    let items: Vec<ListItem> = state
        .library
        .iter()
        .map(|p| {
            // `♫ name ········ 12`, with the count flush right.
            let count = format!(" {} ", p.tracks.len());
            let name_width = inner_width.saturating_sub(count.len() + 2);
            let name = truncate(&p.name, name_width, theme.icons.ellipsis);
            let pad = name_width.saturating_sub(name.width());
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", theme.icons.playlist),
                    Style::new().fg(theme.accent),
                ),
                Span::raw(format!("{name}{}", " ".repeat(pad))),
                Span::styled(count, Style::new().fg(theme.muted)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(pane_block("Library".into(), Pane::Library, state, theme))
        .highlight_style(highlight_style(Pane::Library, state, theme));
    frame.render_stateful_widget(list, area, &mut state.library_state);
}
