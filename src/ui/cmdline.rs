use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use unicode_width::UnicodeWidthStr;

use crate::app::state::{AppState, Hint, Mode, StatusLevel};
use crate::ui::theme::Theme;

/// The bottom line: the `:`/`/` prompt while typing, otherwise mode + status.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let (prefix, editor) = match state.mode {
        Mode::Command => (":".to_string(), &state.command_line),
        Mode::Search => ("/".to_string(), &state.search_line),
        Mode::EditSetting(row) => (
            format!("{} = ", row.color_key().unwrap_or("value")),
            &state.setting_line,
        ),
        Mode::Credential(field) => (field.prompt().to_string(), &state.credential_line),
        Mode::Normal => {
            let mode = Span::styled(" NORMAL ", theme.selection());
            // Errors get a symbol as well as color, so they read in monochrome too.
            let status = match &state.status {
                Some(s) if s.level == StatusLevel::Error => Span::styled(
                    format!("{} {}", theme.icons.error, s.text),
                    Style::new().fg(theme.error),
                ),
                Some(s) => Span::raw(s.text.clone()),
                None => Span::raw(""),
            };
            let left = Line::from(vec![mode, Span::raw(" "), status]);
            let room = (area.width as usize).saturating_sub(left.width() + 2);
            frame.render_widget(left, area);
            frame.render_widget(hint_line(state.hints_for_focus(), room, theme), area);
            return;
        }
    };

    let text = match state.mode {
        // Secrets show as one `*` per character, so a paste can be checked
        // for length without showing it.
        Mode::Credential(field) if field.masked() => "*".repeat(editor.text().chars().count()),
        _ => editor.text().to_string(),
    };
    frame.render_widget(Line::from(format!("{prefix}{text}")), area);
    // Hints such as tab-completion candidates show on the right while typing.
    if let Some(status) = &state.status {
        let hint = Line::from(status.text.clone())
            .style(Style::new().fg(theme.muted))
            .right_aligned();
        frame.render_widget(hint, area);
    }
    // Cursor sits after the prefix; one cell per char is fine for typical input.
    let x = area.x + prefix.width() as u16 + editor.cursor() as u16;
    frame.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
}

/// `enter play · a add · ? help`, dropping hints from the end until it fits.
fn hint_line(hints: &[Hint], room: usize, theme: &Theme) -> Line<'static> {
    let sep = format!(" {} ", theme.icons.sep);
    let width = |n: usize| -> usize {
        let items: usize = hints[..n]
            .iter()
            .map(|h| h.key.width() + 1 + h.label.width())
            .sum();
        items + sep.width() * n.saturating_sub(1)
    };
    let mut n = hints.len();
    while n > 0 && width(n) > room {
        n -= 1;
    }
    let mut spans = Vec::new();
    for (i, hint) in hints[..n].iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(sep.clone(), Style::new().fg(theme.muted)));
        }
        spans.push(Span::styled(
            hint.key.clone(),
            Style::new().fg(theme.accent),
        ));
        spans.push(Span::styled(
            format!(" {}", hint.label),
            Style::new().fg(theme.muted),
        ));
    }
    Line::from(spans).right_aligned()
}
