use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::Theme;
use crate::app::action::Pane;
use crate::app::state::AppState;

/// Top line: app name, plus a pane switcher when only one pane fits.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme, narrow: bool) {
    let mut spans = vec![Span::styled(
        format!(" {} Shellify ", theme.icons.playlist),
        Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
    )];
    if narrow {
        for pane in Pane::ALL {
            let label = format!(" {} ", pane_name(pane));
            spans.push(if pane == state.focus {
                Span::styled(label, theme.selection())
            } else {
                Span::styled(label, Style::new().fg(theme.muted))
            });
        }
    }
    frame.render_widget(Line::from(spans), area);
}

pub fn pane_name(pane: Pane) -> &'static str {
    match pane {
        Pane::Library => "Library",
        Pane::Tracks => "Tracks",
        Pane::Queue => "Queue",
    }
}
