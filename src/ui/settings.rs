use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use super::theme::{ColorMode, Theme};
use crate::app::settings::SettingRow;
use crate::app::state::AppState;

const LABEL_WIDTH: usize = 16;
const VALUE_WIDTH: usize = 10;

/// The Settings tab: one row per option, the selected one highlighted, with
/// a live preview (color swatches, icon glyphs) next to each value.
/// Returns the on-screen area of each visible row, for mouse clicks.
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) -> Vec<(Rect, usize)> {
    let block = Block::bordered()
        .border_set(if theme.mono {
            theme.icons.border_focus
        } else {
            theme.icons.border
        })
        .border_style(Style::new().fg(theme.accent))
        .title(
            Line::from(" Settings ")
                .style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)),
        );
    let inner = block.inner(area);

    let muted = Style::new().fg(theme.muted);
    let mut lines = vec![
        Line::from(""),
        Line::styled(
            " Appearance",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
    ];
    let heading = Style::new().fg(theme.accent).add_modifier(Modifier::BOLD);
    let mut selected_line = 0;
    let mut row_lines = Vec::new();
    for (i, &row) in SettingRow::ALL.iter().enumerate() {
        match row {
            SettingRow::Mouse => {
                lines.push(Line::from(""));
                lines.push(Line::styled(" Behavior", heading));
            }
            SettingRow::Reset => lines.push(Line::from("")),
            _ => {}
        }
        let selected = i == state.settings_cursor;
        if selected {
            selected_line = lines.len();
        }
        row_lines.push((lines.len(), i));
        lines.push(row_line(state, theme, row, selected));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        " Enter on a color to type a name or #hex (empty = theme default).",
        muted,
    ));
    lines.push(Line::styled(
        " Add your own theme: :theme import <file> (a Shellify .toml or a base16 .yaml).",
        muted,
    ));
    lines.push(Line::styled(
        format!(" Changes save automatically to {}", state.config_path_label),
        muted,
    ));

    // Keep the selected row on screen in short terminals.
    let scroll = (selected_line + 2).saturating_sub(inner.height as usize) as u16;
    frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), area);

    let visible = usize::from(scroll)..usize::from(scroll) + usize::from(inner.height);
    row_lines
        .into_iter()
        .filter(|(line, _)| visible.contains(line))
        .map(|(line, i)| {
            let y = inner.y + (line - usize::from(scroll)) as u16;
            (Rect::new(inner.x, y, inner.width, 1), i)
        })
        .collect()
}

fn row_line(state: &AppState, theme: &Theme, row: SettingRow, selected: bool) -> Line<'static> {
    let muted = Style::new().fg(theme.muted);
    if row == SettingRow::Reset {
        let line = Line::from(format!("   {}", row.label()));
        return if selected {
            line.style(theme.selection())
        } else {
            line
        };
    }

    let value = state.appearance.value_label(row);
    // Fixed width so the previews line up; arrows show the value can change.
    let value = if selected {
        format!("< {value:<VALUE_WIDTH$} >")
    } else {
        format!("  {value:<VALUE_WIDTH$}  ")
    };
    let mut spans = vec![
        Span::raw(format!("   {:<LABEL_WIDTH$}", row.label())),
        Span::raw(value),
        Span::raw("  "),
    ];
    spans.extend(preview(state, theme, row, muted));

    let line = Line::from(spans);
    if selected {
        line.style(theme.selection())
    } else {
        line
    }
}

/// What the current value looks like: a swatch, the icon glyphs, or a note.
fn preview(state: &AppState, theme: &Theme, row: SettingRow, muted: Style) -> Vec<Span<'static>> {
    let swatch = |color: Color| vec![Span::styled(theme.icons.swatch, Style::new().fg(color))];
    let icons = theme.icons;
    match row {
        SettingRow::Preset => {
            let id = state
                .appearance
                .theme
                .preset
                .as_deref()
                .unwrap_or("default");
            match state.user_themes.iter().find(|t| t.id == id) {
                Some(custom) => vec![Span::styled(format!("custom: {}", custom.name), muted)],
                None => Vec::new(),
            }
        }
        SettingRow::Accent => swatch(theme.accent),
        SettingRow::Text => swatch(theme.text),
        SettingRow::Muted => swatch(theme.muted),
        SettingRow::Error => swatch(theme.error),
        SettingRow::SelectionFg => vec![Span::styled(" Aa ", theme.selection())],
        SettingRow::Icons => vec![Span::raw(format!(
            "{} {} {} {} {}  {}{}{}{}",
            icons.playing,
            icons.paused,
            icons.current,
            icons.playlist,
            icons.repeat,
            icons.bar_filled,
            icons.bar_filled,
            icons.knob,
            icons.bar_empty,
        ))],
        SettingRow::ResizeCursor if !state.appearance.mouse => {
            vec![Span::styled("(needs Mouse on)", muted)]
        }
        SettingRow::ResizeCursor => vec![Span::styled(
            "pointer shape over pane borders (kitty, foot, Ghostty...)",
            muted,
        )],
        SettingRow::Mouse if state.appearance.mouse => vec![Span::styled(
            "click, double-click, scroll (Shift+drag selects text)",
            muted,
        )],
        SettingRow::Color if theme.mono && state.appearance.color == ColorMode::Auto => {
            vec![Span::styled("(off: NO_COLOR or TERM=dumb is set)", muted)]
        }
        _ => Vec::new(),
    }
}
