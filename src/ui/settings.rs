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
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
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
    let mut selected_line = 0;
    for (i, &row) in SettingRow::ALL.iter().enumerate() {
        if row == SettingRow::Reset {
            lines.push(Line::from(""));
        }
        let selected = i == state.settings_cursor;
        if selected {
            selected_line = lines.len();
        }
        lines.push(row_line(state, theme, row, selected));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        " Enter on a color to type a name or #hex (empty = theme default).",
        muted,
    ));
    lines.push(Line::styled(
        format!(" Changes save automatically to {}", state.config_path_label),
        muted,
    ));

    // Keep the selected row on screen in short terminals.
    let scroll = (selected_line + 2).saturating_sub(inner.height as usize) as u16;
    frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), area);
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
        SettingRow::Color if theme.mono && state.appearance.color == ColorMode::Auto => {
            vec![Span::styled("(off: NO_COLOR or TERM=dumb is set)", muted)]
        }
        _ => Vec::new(),
    }
}
