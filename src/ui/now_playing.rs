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

    let icon = if playback.paused { "⏸ " } else { "▶ " };
    let title = Line::from(vec![
        Span::styled(icon, Style::new().fg(ACCENT)),
        Span::styled(
            track.title.clone(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" — "),
        Span::raw(track.artist.clone()),
    ]);
    frame.render_widget(title, info);
    frame.render_widget(settings, info);

    let ratio = if track.duration.is_zero() {
        0.0
    } else {
        (playback.position.as_secs_f64() / track.duration.as_secs_f64()).clamp(0.0, 1.0)
    };
    let gauge = LineGauge::default()
        .ratio(ratio)
        .label(format!(
            "{} / {}",
            fmt_duration(playback.position),
            fmt_duration(track.duration)
        ))
        .filled_style(Style::new().fg(ACCENT))
        .unfilled_style(Style::new().fg(Color::DarkGray));
    frame.render_widget(gauge, progress);
}
