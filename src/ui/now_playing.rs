use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType};
use unicode_width::UnicodeWidthStr;

use super::fmt_duration;
use super::text::{progress_split, truncate};
use super::theme::Theme;
use crate::app::action::RepeatMode;
use crate::app::state::AppState;

const VOLUME_CELLS: usize = 10;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.muted))
        .title(" Now Playing ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info, progress] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    let settings = settings_line(state, theme);
    let settings_width = settings.width();
    frame.render_widget(settings.right_aligned(), info);

    let muted = Style::new().fg(theme.muted);
    let Some(track) = state.queue.current() else {
        frame.render_widget(
            Line::styled("Nothing playing · select a track and press Enter", muted),
            info,
        );
        frame.render_widget(
            progress_line(0.0, "-:--", "-:--", progress.width, theme),
            progress,
        );
        return;
    };

    let playback = &state.playback;
    let icon = if playback.paused { " ⏸  " } else { " ▶  " };
    // Leave room for the right-aligned settings.
    let room = (info.width as usize).saturating_sub(settings_width + icon.width() + 2);
    let artist = truncate(&track.artist, room / 3);
    let title = truncate(&track.title, room.saturating_sub(artist.width() + 3));
    let title_line = Line::from(vec![
        Span::styled(icon, Style::new().fg(theme.accent)),
        Span::styled(
            title.into_owned(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", muted),
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

/// `⟳ all   vol ━━━━━━━─── 70%`
fn settings_line(state: &AppState, theme: &Theme) -> Line<'static> {
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    let repeat = state.queue.repeat;
    let repeat_style = if repeat == RepeatMode::Off {
        muted
    } else {
        accent
    };

    let volume = usize::from(state.playback.volume);
    let filled = (volume * VOLUME_CELLS).div_ceil(100);
    Line::from(vec![
        Span::styled(format!("⟳ {}", repeat.label()), repeat_style),
        Span::styled("   vol ", muted),
        Span::styled("━".repeat(filled), accent),
        Span::styled("─".repeat(VOLUME_CELLS - filled), muted),
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
    let labels = elapsed.width() + total.width() + 4;
    let bar = (width as usize).saturating_sub(labels);
    let (filled, knob, empty) = progress_split(ratio, bar);
    let muted = Style::new().fg(theme.muted);
    let accent = Style::new().fg(theme.accent);
    Line::from(vec![
        Span::styled(format!(" {elapsed} "), muted),
        Span::styled("━".repeat(filled), accent),
        Span::styled("●".repeat(knob), accent),
        Span::styled("─".repeat(empty), muted),
        Span::styled(format!(" {total} "), muted),
    ])
}
