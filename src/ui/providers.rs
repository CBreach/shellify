//! The Providers tab: a card per streaming service with its pixel-art logo
//! (the highlighted one bounces) and whether it's on, and the setup screen
//! that Enter or a click opens. Only YouTube Music can be switched on yet,
//! signed out; the others' screens say what they'll need.

use std::f32::consts::PI;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::logos::{Logo, Pixel};
use super::text::spinner;
use super::theme::Theme;
use crate::app::signin::time_left;
use crate::app::state::{AppState, SignIn};
use crate::provider::ProviderKind;

/// Logo sizes to try, in pixels (a pixel is half a cell tall).
const LOGO_SIZES: [usize; 3] = [32, 24, 16];
/// How high the highlighted logo hops, in pixels, and how long a hop takes.
const BOUNCE_HEIGHT: f32 = 2.0;
const BOUNCE_PERIOD: f32 = 0.9;
const GAP: u16 = 2;
/// Rows above the cards: the intro line and a blank.
const INTRO: u16 = 2;
/// The one-line-per-provider list for small windows.
const LIST_WIDTH: u16 = 30;
const LIST_LOGO: usize = 16;
const SETUP_WIDTH: u16 = 64;
/// Logo sizes for the setup screen, biggest first.
const SETUP_LOGOS: [usize; 2] = [24, 16];

/// Draws the tab. Returns each provider's clickable area (index into
/// `ProviderKind::ALL`).
pub fn draw(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) -> Vec<(Rect, usize)> {
    let block = Block::bordered()
        .border_set(if theme.mono {
            theme.icons.border_focus
        } else {
            theme.icons.border
        })
        .border_style(Style::new().fg(theme.accent))
        .title(
            Line::from(" Providers ")
                .style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return Vec::new();
    }
    // The intro goes first when short windows need every row for the list.
    let intro = if inner.height >= ProviderKind::ALL.len() as u16 + INTRO {
        let line = Line::styled(
            " Choose where your music comes from.",
            Style::new().fg(theme.muted),
        );
        frame.render_widget(line, Rect { height: 1, ..inner });
        INTRO
    } else {
        0
    };
    let rest = Rect {
        y: inner.y + intro,
        height: inner.height - intro,
        ..inner
    };
    match card_size(rest) {
        Some(logo) => cards(frame.buffer_mut(), rest, logo, state, theme),
        None => list(frame, rest, state, theme),
    }
}

/// Card width and height for a logo size.
fn card_dims(logo: usize) -> (u16, u16) {
    let logo = logo as u16;
    // Borders and padding around the logo; below it a blank row, the name
    // and the status. The logo gets one extra row to bounce into.
    (logo + 6, logo / 2 + 1 + 5)
}

/// The biggest logo whose three cards fit side by side.
fn card_size(area: Rect) -> Option<usize> {
    LOGO_SIZES.into_iter().find(|&logo| {
        let (w, h) = card_dims(logo);
        3 * w + 2 * GAP <= area.width && h <= area.height
    })
}

/// The brand color, used for the highlighted card before its theme is on.
fn brand(kind: ProviderKind, theme: &Theme) -> Color {
    if theme.mono {
        return Color::Reset;
    }
    Theme::preset(kind.theme()).map_or(theme.accent, |t| t.accent)
}

fn cards(
    buf: &mut Buffer,
    area: Rect,
    logo_size: usize,
    state: &AppState,
    theme: &Theme,
) -> Vec<(Rect, usize)> {
    let (w, h) = card_dims(logo_size);
    let row = Layout::horizontal([Constraint::Length(w); 3])
        .spacing(GAP)
        .flex(Flex::Center)
        .split(Rect { height: h, ..area });
    let mut hits = Vec::new();
    for (i, (kind, &card)) in ProviderKind::ALL.iter().zip(row.iter()).enumerate() {
        let selected = i == state.provider_cursor;
        let color = brand(*kind, theme);
        let (border, border_style) = if selected {
            (
                theme.icons.border_focus,
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            )
        } else {
            (theme.icons.border, Style::new().fg(theme.muted))
        };
        let block = Block::bordered()
            .border_set(border)
            .border_style(border_style);
        let inner = block.inner(card);
        ratatui::widgets::Widget::render(block, card, buf);

        let logo_rows = logo_size as u16 / 2 + 1;
        let logo_area = Rect::new(
            inner.x + (inner.width.saturating_sub(logo_size as u16)) / 2,
            inner.y,
            logo_size as u16,
            logo_rows,
        );
        let lift = if selected { bounce(state.bounce) } else { 0 };
        draw_logo(buf, logo_area, &Logo::new(*kind, logo_size), lift, theme);

        let name_style = if selected {
            Style::new().fg(color).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.text)
        };
        let text = [
            Line::styled(kind.name(), name_style),
            status_line(*kind, state, theme),
        ];
        for (dy, line) in text.into_iter().enumerate() {
            let y = inner.y + logo_rows + 1 + dy as u16;
            if y < inner.bottom() {
                let line = line.alignment(Alignment::Center);
                ratatui::widgets::Widget::render(line, Rect::new(inner.x, y, inner.width, 1), buf);
            }
        }
        hits.push((card, i));
    }
    hits
}

/// Too small for cards: one line per provider, and the highlighted one's
/// logo beside the list when there's room.
fn list(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) -> Vec<(Rect, usize)> {
    let mut hits = Vec::new();
    // Scroll just enough to keep the highlighted provider visible.
    let offset = (state.provider_cursor + 1).saturating_sub(usize::from(area.height));
    for (i, kind) in ProviderKind::ALL.iter().enumerate().skip(offset) {
        let y = area.y + (i - offset) as u16;
        if y >= area.bottom() {
            break;
        }
        let selected = i == state.provider_cursor;
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(theme.icons.knob, Style::new().fg(brand(*kind, theme))),
            Span::raw(format!(" {:<14}", kind.name())),
            status_span(*kind, state, theme),
        ]);
        let line = if selected {
            line.style(theme.selection())
        } else {
            line
        };
        let row = Rect::new(area.x, y, LIST_WIDTH.min(area.width), 1);
        frame.render_widget(line, row);
        hits.push((row, i));
    }

    let logo_rows = LIST_LOGO as u16 / 2 + 1;
    let side = Rect {
        x: area.x + LIST_WIDTH + GAP,
        width: area.width.saturating_sub(LIST_WIDTH + GAP),
        ..area
    };
    if side.width >= LIST_LOGO as u16 && side.height >= logo_rows {
        let kind = ProviderKind::ALL[state.provider_cursor.min(ProviderKind::ALL.len() - 1)];
        let logo_area = Rect::new(
            side.x + (side.width - LIST_LOGO as u16) / 2,
            side.y,
            LIST_LOGO as u16,
            logo_rows,
        );
        let logo = Logo::new(kind, LIST_LOGO);
        draw_logo(
            frame.buffer_mut(),
            logo_area,
            &logo,
            bounce(state.bounce),
            theme,
        );
    }
    hits
}

/// Pixels the logo is lifted `t` seconds into its bounce: a hop that slows
/// at the top and speeds up coming down, like a ball.
pub fn bounce(t: f32) -> usize {
    (BOUNCE_HEIGHT * (PI * t / BOUNCE_PERIOD).sin().abs()).round() as usize
}

/// Draws `logo` into `area` (its width, and half its height plus a row),
/// lifted by `lift` pixels. Two pixels share each cell: upper-half blocks
/// with the top pixel as foreground and the bottom one as background.
/// Without color the badge is drawn solid with the symbol cut out; with the
/// ascii pack each cell is a colored space.
fn draw_logo(buf: &mut Buffer, area: Rect, logo: &Logo, lift: usize, theme: &Theme) {
    // The logo starts two pixels (one row) down, so it has room to hop.
    let headroom = 2usize;
    let pixel = |x: usize, row: usize| -> Option<Color> {
        let y = (row + lift).checked_sub(headroom)?;
        match logo.get(x, y) {
            Pixel::Clear => None,
            Pixel::Mark(_) if theme.mono => None,
            Pixel::Body((r, g, b)) | Pixel::Mark((r, g, b)) => Some(Color::Rgb(r, g, b)),
        }
    };
    let icons = theme.icons;
    for cy in 0..area.height {
        for cx in 0..area.width.min(logo.size as u16) {
            let (x, row) = (usize::from(cx), usize::from(cy) * 2);
            let (top, bottom) = (pixel(x, row), pixel(x, row + 1));
            let pos = (area.x + cx, area.y + cy);
            if !buf.area.contains(pos.into()) {
                continue;
            }
            let cell = &mut buf[pos];
            match (top, bottom) {
                (None, None) => {}
                _ if theme.mono => {
                    let glyph = match (top, bottom) {
                        (Some(_), Some(_)) => icons.viz_ramp[8],
                        (Some(_), None) => icons.viz_half[0],
                        _ => icons.viz_half[1],
                    };
                    cell.set_symbol(glyph);
                }
                _ if !icons.braille => {
                    // No block glyphs: a space painted with the color.
                    cell.set_symbol(" ")
                        .set_style(Style::new().bg(top.or(bottom).unwrap_or_default()));
                }
                (Some(t), Some(b)) => {
                    cell.set_symbol(icons.viz_half[0])
                        .set_style(Style::new().fg(t).bg(b));
                }
                (Some(t), None) => {
                    cell.set_symbol(icons.viz_half[0])
                        .set_style(Style::new().fg(t));
                }
                (None, Some(b)) => {
                    cell.set_symbol(icons.viz_half[1])
                        .set_style(Style::new().fg(b));
                }
            }
        }
    }
}

/// `on` (in use), `off` (available) or where it is on the roadmap.
fn status_span(kind: ProviderKind, state: &AppState, theme: &Theme) -> Span<'static> {
    if state.active_provider == Some(kind) {
        let label = match state.sign_in {
            SignIn::SignedIn { .. } => "signed in",
            _ => "on",
        };
        Span::styled(
            label,
            Style::new()
                .fg(brand(kind, theme))
                .add_modifier(Modifier::BOLD),
        )
    } else if kind.available() {
        Span::styled("off", Style::new().fg(theme.muted))
    } else {
        Span::styled(kind.status(), Style::new().fg(theme.muted))
    }
}

fn status_line(kind: ProviderKind, state: &AppState, theme: &Theme) -> Line<'static> {
    Line::from(status_span(kind, state, theme))
}

/// What the setup screen says: how to use a provider that's on (and where
/// signing in has got to), or what one that isn't built yet will need.
fn setup_text(kind: ProviderKind, state: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    if state.active_provider != Some(kind) {
        return vec![
            Line::from(format!(
                "Setup for {} isn't available yet ({}).",
                kind.name(),
                kind.status()
            )),
            Line::from(kind.requirement()),
            Line::from(""),
            Line::from(
                "Your sign-in will stay on this computer, in the system keychain, \
                 never in Shellify's config file.",
            ),
        ];
    }
    let strong = Style::new()
        .fg(brand(kind, theme))
        .add_modifier(Modifier::BOLD);
    let muted = Style::new().fg(theme.muted);
    let mut lines = match &state.sign_in {
        SignIn::SignedOut => vec![
            Line::from(format!(
                "{} is on. Search with / and play any song, no account needed.",
                kind.name()
            )),
            Line::from(""),
            Line::from("To see your playlists and liked songs, sign in: run :login."),
            Line::styled(
                "Read-only access, with your own Google client (see the README). \
                 Revoke it any time at myaccount.google.com/permissions.",
                muted,
            ),
        ],
        SignIn::Working(what) => vec![Line::from(format!(
            "{} {what}{}",
            spinner(theme.icons.spinner),
            theme.icons.ellipsis
        ))],
        SignIn::Code { url, code, expires } => vec![
            Line::from("On any device, go to"),
            Line::styled(url.clone(), strong),
            Line::from("and enter the code"),
            Line::styled(code.clone(), strong),
            Line::from(""),
            Line::styled(
                format!(
                    "{} Waiting for you to approve (code expires in {}). \
                     Esc hides this; sign-in carries on.",
                    spinner(theme.icons.spinner),
                    time_left(*expires)
                ),
                muted,
            ),
        ],
        SignIn::SignedIn { account } => vec![
            Line::from(match account {
                Some(name) => format!("Signed in as {name}."),
                None => "Signed in with your Google account.".to_string(),
            }),
            Line::from("Search with / and play any song. :logout signs out."),
        ],
    };
    lines.extend([
        Line::from(""),
        Line::styled(
            format!(
                "To go back to the demo tracks, press Enter on {} again, or run :provider off.",
                kind.name()
            ),
            muted,
        ),
    ]);
    lines
}

/// The setup screen for `kind`, over everything else. Returns its area (a
/// click outside closes it).
pub fn draw_setup(frame: &mut Frame, state: &AppState, theme: &Theme, kind: ProviderKind) -> Rect {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(SETUP_WIDTH);
    let text_width = width.saturating_sub(4);
    let color = brand(kind, theme);
    let muted = Style::new().fg(theme.muted);
    let mut lines = vec![
        Line::styled(
            format!("Set up {}", kind.name()),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
    ];
    lines.extend(setup_text(kind, state, theme));
    lines.extend([
        Line::from(""),
        Line::styled(
            format!(
                "Theme switched to {}. Change it any time in Settings.",
                kind.name()
            ),
            muted,
        ),
    ]);
    let text_rows: u16 = lines
        .iter()
        .map(|l| wrapped_rows(&l.to_string(), usize::from(text_width)))
        .sum();
    // Borders plus a blank row above and below the text.
    let bare = text_rows + 4;
    let room = screen.height.saturating_sub(2);
    let logo = SETUP_LOGOS
        .into_iter()
        .find(|&size| bare + size as u16 / 2 < room && width >= size as u16 + 4);
    let logo_rows = logo.map_or(0, |size| size as u16 / 2 + 1);
    let height = (bare + logo_rows).min(room);

    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(screen);
    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    let block = Block::bordered()
        .border_set(theme.icons.border)
        .border_style(Style::new().fg(color))
        .title_bottom(
            Line::from(format!(" esc close {} enter ok ", theme.icons.sep))
                .style(muted)
                .right_aligned(),
        );
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let mut text_area = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
    };
    if let Some(size) = logo {
        let logo_area = Rect::new(
            inner.x + inner.width.saturating_sub(size as u16) / 2,
            inner.y,
            size as u16,
            logo_rows,
        );
        let logo = Logo::new(kind, size);
        draw_logo(
            frame.buffer_mut(),
            logo_area,
            &logo,
            bounce(state.bounce),
            theme,
        );
        text_area.y = logo_area.bottom() + 1;
        text_area.height = inner.bottom().saturating_sub(text_area.y);
    }
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        text_area,
    );
    popup
}

/// Lines `text` takes when word-wrapped to `width` columns.
fn wrapped_rows(text: &str, width: usize) -> u16 {
    let mut rows = 1;
    let mut used = 0;
    for word in text.split_whitespace() {
        let w = unicode_width::UnicodeWidthStr::width(word);
        if used > 0 && used + 1 + w > width {
            rows += 1;
            used = w;
        } else {
            used += if used > 0 { 1 + w } else { w };
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounce_hops_up_and_comes_back_down() {
        assert_eq!(bounce(0.0), 0);
        assert_eq!(bounce(BOUNCE_PERIOD / 2.0), BOUNCE_HEIGHT as usize);
        assert_eq!(bounce(BOUNCE_PERIOD), 0);
        assert!(bounce(BOUNCE_PERIOD * 0.25) >= 1);
        assert!(
            bounce(BOUNCE_PERIOD * 1.5) == BOUNCE_HEIGHT as usize,
            "repeats"
        );
    }

    #[test]
    fn counts_wrapped_rows() {
        assert_eq!(wrapped_rows("", 10), 1);
        assert_eq!(wrapped_rows("one two three", 13), 1);
        assert_eq!(wrapped_rows("one two three", 12), 2);
        assert_eq!(wrapped_rows("one two three", 3), 3);
    }

    #[test]
    fn picks_the_biggest_logo_that_fits() {
        assert_eq!(card_size(Rect::new(0, 0, 150, 40)), Some(32));
        assert_eq!(card_size(Rect::new(0, 0, 100, 20)), Some(24));
        assert_eq!(card_size(Rect::new(0, 0, 76, 16)), Some(16));
        assert_eq!(card_size(Rect::new(0, 0, 60, 16)), None);
        assert_eq!(card_size(Rect::new(0, 0, 150, 8)), None);
    }
}
