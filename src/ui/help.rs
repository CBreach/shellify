use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;
use crate::app::state::AppState;
use crate::command::parser::COMMANDS;

const MAX_WIDTH: u16 = 76;
/// Usages wider than this wrap their description onto the next line.
const USAGE_COLUMN: usize = 26;

/// Centered overlay listing every key binding (from the live keymap, so user
/// overrides show up) and every `:` command.
pub fn draw(frame: &mut Frame, state: &mut AppState, theme: &Theme) {
    let area = popup_area(frame.area());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.accent))
        .title(
            Line::from(" Help ").style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)),
        )
        .title_bottom(
            Line::from(" j/k scroll · esc close ")
                .style(Style::new().fg(theme.muted))
                .right_aligned(),
        );
    let inner = block.inner(area);

    let lines = content(state, theme);
    // Clamp here so `G` (scroll = MAX) lands on the last page.
    let max_scroll = (lines.len() as u16).saturating_sub(inner.height);
    state.help_scroll = state.help_scroll.min(max_scroll);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((state.help_scroll, 0)),
        area,
    );
}

fn popup_area(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let height = area.height.saturating_sub(2);
    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    popup
}

fn content(state: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let heading = Style::new().fg(theme.accent).add_modifier(Modifier::BOLD);
    let key_style = Style::new().fg(theme.accent);
    let muted = Style::new().fg(theme.muted);

    let mut lines = vec![Line::styled(" Keys", heading)];
    let key_w = state
        .help_entries
        .iter()
        .map(|e| e.keys.width())
        .max()
        .unwrap_or(0);
    for entry in &state.help_entries {
        let pad = key_w - entry.keys.width();
        lines.push(Line::from(vec![
            Span::raw(format!("   {}", " ".repeat(pad))),
            Span::styled(entry.keys.clone(), key_style),
            Span::raw("   "),
            Span::raw(entry.description.clone()),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(" Commands  (press : then type)", heading));
    // Long usages (e.g. `focus <next|prev|...>`) put their description on
    // the next line instead of pushing every description off-screen.
    let usage_w = COMMANDS
        .iter()
        .map(|c| c.usage.width())
        .filter(|&w| w <= USAGE_COLUMN)
        .max()
        .unwrap_or(USAGE_COLUMN);
    for cmd in COMMANDS {
        let usage = Span::styled(format!("   :{}", cmd.usage), key_style);
        if cmd.usage.width() > usage_w {
            lines.push(Line::from(usage));
            lines.push(Line::from(format!(
                "    {}{}",
                " ".repeat(usage_w + 3),
                cmd.about
            )));
        } else {
            let pad = usage_w - cmd.usage.width();
            lines.push(Line::from(vec![
                usage,
                Span::raw(" ".repeat(pad + 3)),
                Span::raw(cmd.about),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(
        " Rebind keys in ~/.config/shellify/config.toml under [keys].",
        muted,
    ));
    lines
}
