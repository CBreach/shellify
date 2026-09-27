//! Draws the visualizer model (`app::visualizer`) in one of four styles.
//! Glyphs come from the icon pack (eighth blocks and braille, or plain
//! ASCII), colors from the theme: accent for the body, the error color for
//! the loudest parts, text for peak caps. With `NO_COLOR` the shapes alone
//! carry it.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};

use super::theme::Theme;
use crate::app::visualizer::{BANDS, Visualizer, VizStyle};

pub fn draw(frame: &mut Frame, area: Rect, viz: &Visualizer, style: VizStyle, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    match style {
        VizStyle::Bars => bars(frame.buffer_mut(), area, viz, theme),
        VizStyle::Mirror => mirror(frame.buffer_mut(), area, viz, theme),
        VizStyle::Wave if theme.icons.braille => wave_braille(frame, area, viz, theme),
        VizStyle::Wave => wave_cells(frame.buffer_mut(), area, viz, theme),
        VizStyle::Dots if theme.icons.braille => dots_braille(frame, area, viz, theme),
        VizStyle::Dots => dots_cells(frame.buffer_mut(), area, viz, theme),
    }
}

/// Accent for most of the height, the error color for the loudest part.
fn heat(theme: &Theme, fraction: f32) -> Color {
    if fraction > 0.82 {
        theme.error
    } else {
        theme.accent
    }
}

/// One-cell-wide bars with a one-cell gap, when there's room for enough.
fn columns(width: u16) -> (usize, u16) {
    if width >= 32 {
        (usize::from(width.div_ceil(2)), 2)
    } else {
        (usize::from(width), 1)
    }
}

fn bars(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme) {
    let icons = theme.icons;
    let (n, step) = columns(area.width);
    let rows = area.height;
    let eighths_total = u32::from(rows) * 8;
    for i in 0..n {
        let x = area.x + i as u16 * step;
        let value = Visualizer::sample(&viz.bands, i, n);
        let mut eighths = (value * eighths_total as f32).round() as u32;
        for row in 0..rows {
            let y = area.bottom() - 1 - row;
            let (glyph, filled) = if eighths >= 8 {
                eighths -= 8;
                (icons.viz_ramp[8], true)
            } else {
                let g = icons.viz_ramp[eighths as usize];
                let had = eighths > 0;
                eighths = 0;
                (g, had)
            };
            if filled {
                let fraction = f32::from(row + 1) / f32::from(rows);
                buf[(x, y)]
                    .set_symbol(glyph)
                    .set_style(Style::new().fg(heat(theme, fraction)));
            }
        }
        // Peak cap in the cell just above where the peak sits.
        let peak = Visualizer::sample(&viz.peaks, i, n);
        let peak_row = (peak * f32::from(rows)).floor() as u16;
        if peak > 0.02 && peak_row < rows && peak > value + 0.02 {
            let y = area.bottom() - 1 - peak_row;
            buf[(x, y)]
                .set_symbol(icons.viz_peak)
                .set_style(Style::new().fg(theme.text));
        }
    }
}

fn mirror(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme) {
    let icons = theme.icons;
    let (n, step) = columns(area.width);
    // A center row with the same number of rows above and below it, so both
    // halves mirror exactly (an even height leaves the bottom row unused).
    let arm = (area.height - 1) / 2;
    let center = area.y + arm;
    for i in 0..n {
        let x = area.x + i as u16 * step;
        let value = Visualizer::sample(&viz.bands, i, n);
        let color = |fraction: f32| Style::new().fg(heat(theme, fraction));
        if value < 0.02 {
            // Keep a faint center line where it's silent.
            buf[(x, center)]
                .set_symbol(icons.bar_empty)
                .set_style(Style::new().fg(theme.muted));
            continue;
        }
        buf[(x, center)]
            .set_symbol(icons.viz_ramp[8])
            .set_style(color(0.0));
        // Each arm grows in half cells: a partial end is a half block facing
        // the center (lower half going up, upper half going down).
        let mut halves = (value * f32::from(arm) * 2.0).round() as u16;
        for row in 0..arm {
            if halves == 0 {
                break;
            }
            let full = halves >= 2;
            halves = halves.saturating_sub(2);
            let style = color(f32::from(row + 1) / f32::from(arm));
            let up = if full {
                icons.viz_ramp[8]
            } else {
                icons.viz_half[1]
            };
            let down = if full {
                icons.viz_ramp[8]
            } else {
                icons.viz_half[0]
            };
            buf[(x, center - 1 - row)].set_symbol(up).set_style(style);
            buf[(x, center + 1 + row)].set_symbol(down).set_style(style);
        }
    }
}

/// The wave's height (-1..1) at horizontal position `t` (0..1): a few
/// travelling sines whose amplitude follows the bands under that position.
fn wave_at(viz: &Visualizer, t: f32) -> f32 {
    let band = Visualizer::sample(&viz.bands, (t * (BANDS - 1) as f32) as usize, BANDS);
    let p = viz.phase;
    let shape = 0.6 * (t * 14.0 - p * 3.1).sin()
        + 0.3 * (t * 31.0 + p * 2.3).sin()
        + 0.1 * (t * 67.0 - p * 5.7).sin();
    // Silent means a flat line; a little motion grows with overall loudness.
    (shape * (0.15 * viz.level + 0.85 * band)).clamp(-1.0, 1.0)
}

fn wave_braille(frame: &mut Frame, area: Rect, viz: &Visualizer, theme: &Theme) {
    // Braille gives 2 dots per cell across; sample at that resolution.
    let samples = usize::from(area.width) * 2;
    let color = heat(theme, viz.level);
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, samples as f64])
        .y_bounds([-1.0, 1.0])
        .paint(|ctx| {
            let mut prev = None;
            for s in 0..=samples {
                let y = f64::from(wave_at(viz, s as f32 / samples as f32));
                if let Some((px, py)) = prev {
                    ctx.draw(&CanvasLine::new(px, py, s as f64, y, color));
                }
                prev = Some((s as f64, y));
            }
        });
    frame.render_widget(canvas, area);
}

fn wave_cells(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme) {
    let style = Style::new().fg(heat(theme, viz.level));
    for col in 0..area.width {
        let t = f32::from(col) / f32::from(area.width.max(2) - 1);
        let y = wave_at(viz, t);
        // -1 at the bottom row, 1 at the top.
        let row = ((1.0 - (y + 1.0) / 2.0) * f32::from(area.height - 1)).round() as u16;
        buf[(area.x + col, area.y + row.min(area.height - 1))]
            .set_symbol(theme.icons.viz_mark)
            .set_style(style);
    }
}

/// Each column's dot height: its band, bobbing a little with the phase.
fn dot_heights(viz: &Visualizer, n: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
    (0..n).map(move |i| {
        let v = Visualizer::sample(&viz.bands, i, n);
        let bob = 0.04 * (viz.phase * 4.0 + i as f32 * 0.9).sin() * v;
        (i, (v + bob).clamp(0.0, 1.0))
    })
}

fn dots_braille(frame: &mut Frame, area: Rect, viz: &Visualizer, theme: &Theme) {
    // One dot per cell across (braille would allow two; one reads cleaner).
    let n = usize::from(area.width);
    let rows = f64::from(area.height) * 4.0;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n as f64])
        .y_bounds([0.0, rows])
        .paint(|ctx| {
            for (i, h) in dot_heights(viz, n) {
                if h < 0.02 {
                    continue;
                }
                let y = f64::from(h) * (rows - 1.0);
                ctx.draw(&Points {
                    coords: &[(i as f64 + 0.5, y)],
                    color: heat(theme, h),
                });
            }
            // Peaks as a sparse line of dots above.
            for i in (0..n).step_by(3) {
                let p = Visualizer::sample(&viz.peaks, i, n);
                if p > 0.05 {
                    ctx.draw(&Points {
                        coords: &[(i as f64 + 0.5, f64::from(p) * (rows - 1.0))],
                        color: theme.text,
                    });
                }
            }
        });
    frame.render_widget(canvas, area);
}

fn dots_cells(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme) {
    let (n, step) = columns(area.width);
    for (i, h) in dot_heights(viz, n) {
        if h < 0.02 {
            continue;
        }
        let row = (h * f32::from(area.height - 1)).round() as u16;
        let (x, y) = (area.x + i as u16 * step, area.bottom() - 1 - row);
        buf[(x, y)]
            .set_symbol(theme.icons.viz_mark)
            .set_style(Style::new().fg(heat(theme, h)));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::ui::icons::IconPack;

    fn loud() -> Visualizer {
        let mut v = Visualizer::default();
        for _ in 0..20 {
            v.meter(-10.0, -4.0);
            v.tick(1.0 / 30.0, true);
        }
        v
    }

    fn render(viz: &Visualizer, style: VizStyle, theme: &Theme, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| draw(f, f.area(), viz, style, theme))
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    fn inked(lines: &[String]) -> usize {
        lines
            .iter()
            .flat_map(|l| l.chars())
            .filter(|c| *c != ' ')
            .count()
    }

    #[test]
    fn every_style_draws_something_when_loud_and_nothing_at_rest() {
        let theme = Theme::default();
        let quiet = Visualizer::default();
        for style in VizStyle::ALL {
            for (w, h) in [(80, 5), (40, 5), (10, 1)] {
                let lines = render(&loud(), style, &theme, w, h);
                assert!(inked(&lines) > 0, "{style:?} {w}x{h} drew nothing");
            }
            let lines = render(&quiet, style, &theme, 80, 5);
            if style == VizStyle::Wave {
                let busy_rows = lines.iter().filter(|l| !l.trim().is_empty()).count();
                assert_eq!(
                    busy_rows,
                    1,
                    "a silent wave is one flat line:\n{}",
                    lines.join("\n")
                );
            }
            // At rest: empty, apart from mirror's faint center line and the
            // wave's flat line.
            let limit = match style {
                VizStyle::Mirror => 40,
                VizStyle::Wave => 80,
                _ => 0,
            };
            assert!(
                inked(&lines) <= limit,
                "{style:?} at rest:\n{}",
                lines.join("\n")
            );
        }
    }

    #[test]
    fn mirror_is_symmetric_around_its_center_row() {
        for h in [5u16, 6, 7] {
            let lines = render(&loud(), VizStyle::Mirror, &Theme::default(), 40, h);
            let arm = usize::from((h - 1) / 2);
            for d in 1..=arm {
                let up: usize = lines[arm - d].chars().filter(|c| *c != ' ').count();
                let down: usize = lines[arm + d].chars().filter(|c| *c != ' ').count();
                assert_eq!(up, down, "row ±{d} at height {h}:\n{}", lines.join("\n"));
            }
        }
    }

    #[test]
    fn bars_rise_from_the_bottom() {
        let lines = render(&loud(), VizStyle::Bars, &Theme::default(), 40, 5);
        let bottom = lines[4].chars().filter(|c| *c != ' ').count();
        let top = lines[0].chars().filter(|c| *c != ' ').count();
        assert!(bottom > top, "\n{}", lines.join("\n"));
    }

    #[test]
    fn ascii_pack_draws_only_ascii_in_every_style() {
        let theme = Theme {
            icons: IconPack::Ascii.icons(),
            ..Theme::default()
        };
        for style in VizStyle::ALL {
            let lines = render(&loud(), style, &theme, 60, 5);
            let all: String = lines.concat();
            assert!(all.is_ascii(), "{style:?}:\n{}", lines.join("\n"));
            assert!(inked(&lines) > 0, "{style:?} drew nothing");
        }
    }
}
