use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthStr;

use super::fmt_duration;
use super::text::{progress_split, truncate};
use super::theme::Theme;
use crate::app::action::RepeatMode;
use crate::app::state::AppState;

const VOLUME_CELLS: usize = 10;
const COMPACT_BELOW: u16 = 70;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let icons = theme.icons;
    let block = Block::bordered()
        .border_set(theme.icons.border)
        .border_style(Style::new().fg(theme.muted))
        .title(" Now Playing ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info, progress] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    // Narrow windows drop the volume meter to leave room for the title.
    let compact = inner.width < COMPACT_BELOW;
    let settings = settings_line(state, theme, compact);
    let settings_width = settings.width();
    frame.render_widget(settings.right_aligned(), info);

    let muted = Style::new().fg(theme.muted);
    let Some(track) = state.queue.current() else {
        frame.render_widget(
            Line::styled(
                format!(
                    "Nothing playing {} select a track and press Enter",
                    icons.sep
                ),
                muted,
            ),
            info,
        );
        frame.render_widget(
            progress_line(0.0, "-:--", "-:--", progress.width, theme),
            progress,
        );
        return;
    };

    let playback = &state.playback;
    let icon = format!(
        " {}  ",
        if playback.paused {
            icons.paused
        } else {
            icons.playing
        }
    );
    // Leave room for the right-aligned settings.
    let room = (info.width as usize).saturating_sub(settings_width + icon.width() + 2);
    let artist = truncate(&track.artist, room / 3, theme.icons.ellipsis);
    let title = truncate(
        &track.title,
        room.saturating_sub(artist.width() + 3),
        theme.icons.ellipsis,
    );
    let title_line = Line::from(vec![
        Span::styled(icon, Style::new().fg(theme.accent)),
        Span::styled(
            title.into_owned(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {} ", icons.sep), muted),
        Span::styled(artist.into_owned(), muted),
    ]);
    frame.render_widget(title_line, info);

    let ratio = if track.duration.is_zero() {
        0.0
    } else {
        playback.position.as_secs_f64() / track.duration.as_secs_f64()
    };
    let line = progress_line(
        ratio,
        &fmt_duration(playback.position),
        &fmt_duration(track.duration),
        progress.width,
        theme,
    );
    frame.render_widget(line, progress);
}

/// `⟳ all   vol ━━━━━━━─── 70%` (glyphs from the icon pack)
fn settings_line(state: &AppState, theme: &Theme, compact: bool) -> Line<'static> {
    let icons = theme.icons;
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    let repeat = state.queue.repeat;
    let repeat_style = if repeat == RepeatMode::Off {
        muted
    } else {
        accent
    };

    let volume = usize::from(state.playback.volume);
    let repeat_span = Span::styled(format!("{} {}", icons.repeat, repeat.label()), repeat_style);
    if compact {
        return Line::from(vec![
            repeat_span,
            Span::styled(format!("  vol {volume}% "), muted),
        ]);
    }
    let filled = (volume * VOLUME_CELLS).div_ceil(100);
    Line::from(vec![
        repeat_span,
        Span::styled("   vol ", muted),
        Span::styled(icons.bar_filled.repeat(filled), accent),
        Span::styled(icons.bar_empty.repeat(VOLUME_CELLS - filled), muted),
        Span::styled(format!(" {volume:>3}% "), muted),
    ])
}

/// ` 1:23 ━━━━━━━━●──────────── 3:44 `
fn progress_line(
    ratio: f64,
    elapsed: &str,
    total: &str,
    width: u16,
    theme: &Theme,
) -> Line<'static> {
    let icons = theme.icons;
    let labels = elapsed.width() + total.width() + 4;
    let bar = (width as usize).saturating_sub(labels);
    let (filled, knob, empty) = progress_split(ratio, bar);
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    Line::from(vec![
        Span::styled(format!(" {elapsed} "), muted),
        Span::styled(icons.bar_filled.repeat(filled), accent),
        Span::styled(icons.knob.repeat(knob), accent),
        Span::styled(icons.bar_empty.repeat(empty), muted),
        Span::styled(format!(" {total} "), muted),
    ])
}
