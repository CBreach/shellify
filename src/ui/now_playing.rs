use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, LineGauge, Paragraph};

use super::{ACCENT, fmt_duration};
use crate::app::state::AppState;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::DarkGray))
        .title(" Now Playing ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info, progress] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    let playback = &state.playback;
    let settings = Line::from(format!(
        "vol {}%  repeat {}",
        playback.volume,
        state.queue.repeat.label()
    ))
    .style(Style::new().fg(Color::DarkGray))
    .right_aligned();

    let Some(track) = state.queue.current() else {
        frame.render_widget(
            Paragraph::new("Nothing playing").style(Style::new().fg(Color::DarkGray)),
            info,
        );
        frame.render_widget(settings, info);
        return;
    };

    let icon = if playback.loading {
        format!("{} ", spinner_frame())
    } else if playback.paused {
        "⏸ ".to_string()
    } else {
        "▶ ".to_string()
    };
    let mut spans = vec![
        Span::styled(icon, Style::new().fg(ACCENT)),
        Span::styled(
            track.title.clone(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
    ];
    if !track.artist.is_empty() {
        spans.push(Span::raw(" — "));
        spans.push(Span::raw(track.artist.clone()));
    }
    frame.render_widget(Line::from(spans), info);
    frame.render_widget(settings, info);

    // The player's duration is exact; metadata may be missing or approximate.
    let duration = playback.duration.unwrap_or(track.duration);
    let ratio = if duration.is_zero() {
        0.0
    } else {
        (playback.position.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0)
    };
    let label = if playback.loading {
        "Loading…".to_string()
    } else if duration.is_zero() {
        fmt_duration(playback.position)
    } else {
        format!(
            "{} / {}",
            fmt_duration(playback.position),
            fmt_duration(duration)
        )
    };
    let gauge = LineGauge::default()
        .ratio(ratio)
        .label(label)
        .filled_style(Style::new().fg(ACCENT))
        .unfilled_style(Style::new().fg(Color::DarkGray));
    frame.render_widget(gauge, progress);
}

/// Braille spinner driven by the clock; the app redraws on every tick.
fn spinner_frame() -> char {
    const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    FRAMES[(millis / 250 % FRAMES.len() as u128) as usize]
}
