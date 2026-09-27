use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::text::{spinner, truncate, wrap};
use super::{highlight_style, pane_block};
use crate::app::action::Pane;
use crate::app::providers_hint;
use crate::app::state::AppState;
use crate::ui::theme::Theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let block = pane_block("Library".into(), Pane::Library, state, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.library.is_empty() {
        let msg = if state.library_loading {
            format!(
                "{} Loading your library{}",
                spinner(theme.icons.spinner),
                theme.icons.ellipsis
            )
        } else {
            "No playlists yet".to_string()
        };
        let msg = Paragraph::new(vec![Line::from(""), Line::from(msg)])
            .alignment(Alignment::Center)
            .style(Style::new().fg(theme.muted));
        frame.render_widget(msg, inner);
        return;
    }

    let list_area = if state.demo {
        draw_demo_notice(frame, inner, state, theme)
    } else {
        inner
    };
    let inner_width = inner.width as usize;
    let items: Vec<ListItem> = state
        .library
        .iter()
        .map(|p| {
            // `♫ name ········ 12`, with the count flush right (when known).
            let count = p
                .tracks
                .as_ref()
                .map_or_else(String::new, |t| format!(" {} ", t.len()));
            let name_width = inner_width.saturating_sub(count.width() + 2);
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
    let list = List::new(items).highlight_style(highlight_style(Pane::Library, state, theme));
    frame.render_stateful_widget(list, list_area, &mut state.library_state);
}

/// Says the library is a demo and how to add a provider, at the bottom of
/// the pane when there's room. Returns the area left for the playlists.
fn draw_demo_notice(frame: &mut Frame, inner: Rect, state: &AppState, theme: &Theme) -> Rect {
    // One column of padding each side.
    let width = (inner.width as usize).saturating_sub(2);
    let ellipsis = theme.icons.ellipsis;
    let hint = providers_hint(state);
    let long = format!("For your own library and full search, add a provider: {hint}.");
    let mut text = wrap(&long, width, ellipsis);
    if text.len() > 3 {
        // A narrow pane: keep it to the point.
        text = wrap(&format!("Add a provider: {hint}."), width, ellipsis);
    }
    let heading = Style::new().fg(theme.accent).add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let lines: Vec<Line> = wrap("Demo tracks", width, ellipsis)
        .into_iter()
        .map(|l| Line::from(Span::styled(l, heading)))
        .chain(text.into_iter().map(|l| Line::from(Span::styled(l, muted))))
        .collect();
    // A blank line above it; keep at least a few playlist rows.
    let height = lines.len() as u16 + 1;
    let wanted = height + state.library.len().min(3) as u16;
    if width < 10 || inner.height < wanted {
        return inner;
    }
    let [list, notice] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(height)]).areas(inner);
    let notice = Rect {
        x: notice.x + 1,
        y: notice.y + 1,
        width: notice.width.saturating_sub(2),
        height: notice.height - 1,
    };
    frame.render_widget(Paragraph::new(lines), notice);
    list
}
