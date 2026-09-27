use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use crate::app::action::{Pane, View};
use crate::app::state::AppState;

/// Top line: app name, the Music/Settings/Providers tabs, and a pane switcher when
/// only one pane fits. Returns where each tab was drawn, for mouse clicks.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &Theme,
    narrow: bool,
) -> Vec<(Rect, View)> {
    let mut tabs = Vec::new();
    let muted = Style::new().fg(theme.muted);
    let mut spans = Vec::new();
    if !narrow {
        spans.push(Span::styled(
            format!(" {} Shellify ", theme.icons.playlist),
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ));
    }
    for (view, label) in [View::Music, View::Settings, View::Providers]
        .into_iter()
        .zip(&state.tab_labels)
    {
        let style = if state.view == view {
            theme.selection()
        } else {
            muted
        };
        let x = area.x + Line::from(spans.clone()).width() as u16;
        let label = format!(" {label} ");
        let width = (label.width() as u16).min(area.right().saturating_sub(x));
        tabs.push((Rect::new(x, area.y, width, 1), view));
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }

    if narrow && state.view == View::Music {
        spans.push(Span::styled(format!("{} ", theme.icons.sep), muted));
        let used = Line::from(spans.clone()).width();
        let all: usize = Pane::ALL.iter().map(|p| pane_name(*p).len() + 2).sum();
        // Show all three pane names if they fit, else just the focused one.
        let shown: Vec<Pane> = if used + all <= area.width as usize {
            Pane::ALL.to_vec()
        } else {
            vec![state.focus]
        };
        for pane in shown {
            let label = format!(" {} ", pane_name(pane));
            spans.push(if pane == state.focus {
                Span::styled(label, theme.selection())
            } else {
                Span::styled(label, muted)
            });
        }
    }
    frame.render_widget(Line::from(spans), area);
    tabs
}

pub fn pane_name(pane: Pane) -> &'static str {
    match pane {
        Pane::Library => "Library",
        Pane::Tracks => "Tracks",
        Pane::Queue => "Queue",
    }
}
