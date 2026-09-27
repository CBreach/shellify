//! Draws the visualizer model (`app::visualizer`) in one of four styles.
//! Glyphs come from the icon pack (eighth blocks and braille, or plain
//! ASCII), colors from the theme: accent for the body, the error color for
//! the loudest parts, text for peak caps. With `NO_COLOR` the shapes alone
//! carry it.
//!
//! With fade on, every cell's brightness follows the model's glow grid: new
//! parts of a bar ease in, and parts it drops below fade out, leaving a
//! trail. True-color themes blend smoothly toward the muted color; others
//! step through dim styles and lighter shade glyphs (which also carry the
//! fade without color).

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};

use super::theme::Theme;
use crate::app::visualizer::{BANDS, Visualizer, VizStyle};

/// Below this a fading cell isn't drawn at all.
const FADE_FLOOR: f32 = 0.06;
/// A bar's own cells never dim below this, so a bar that's there shows.
const BODY_FLOOR: f32 = 0.35;
/// Trails are a little dimmer than the bars that left them.
const TRAIL: f32 = 0.85;

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    viz: &Visualizer,
    style: VizStyle,
    fade: bool,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let fade = fade.then_some(Fade { theme });
    match style {
        VizStyle::Bars => bars(frame.buffer_mut(), area, viz, theme, fade),
        VizStyle::Mirror => mirror(frame.buffer_mut(), area, viz, theme, fade),
        VizStyle::Wave if theme.icons.braille => wave_braille(frame, area, viz, theme, fade),
        VizStyle::Wave => wave_cells(frame.buffer_mut(), area, viz, theme, fade),
        VizStyle::Dots if theme.icons.braille => dots_braille(frame, area, viz, theme, fade),
        VizStyle::Dots => dots_cells(frame.buffer_mut(), area, viz, theme, fade),
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

/// How cells look as they fade in and out.
#[derive(Clone, Copy)]
struct Fade<'a> {
    theme: &'a Theme,
}

impl Fade<'_> {
    /// Whether colors can be blended (both are true-color).
    fn smooth(self, color: Color) -> bool {
        matches!((color, self.theme.muted), (Color::Rgb(..), Color::Rgb(..)))
    }

    /// `color` at `intensity` (0..1): blended toward muted, or stepped
    /// through dim and muted when the colors can't be blended.
    fn color(self, color: Color, intensity: f32) -> Color {
        let muted = self.theme.muted;
        match (color, muted) {
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                let t = intensity.clamp(0.0, 1.0);
                let mix = |a: u8, b: u8| (f32::from(b) + (f32::from(a) - f32::from(b)) * t) as u8;
                Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
            }
            _ if intensity >= 0.45 => color,
            _ => muted,
        }
    }

    fn style(self, color: Color, intensity: f32) -> Style {
        let style = Style::new().fg(self.color(color, intensity));
        if !self.smooth(color) && (0.45..0.7).contains(&intensity) {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }

    /// `full` while bright; lighter shade glyphs as it fades (earlier when
    /// color can't show the fade by itself).
    fn glyph(self, full: &'static str, color: Color, intensity: f32) -> &'static str {
        let shade = self.theme.icons.viz_shade;
        if self.smooth(color) {
            // The color does the fading; only the last wisp gets lighter.
            return if intensity >= 0.25 { full } else { shade[0] };
        }
        match intensity {
            i if i >= 0.6 => full,
            i if i >= 0.4 => shade[2],
            i if i >= 0.2 => shade[1],
            _ => shade[0],
        }
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

fn bars(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme, fade: Option<Fade>) {
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
            let fraction = f32::from(row + 1) / f32::from(rows);
            let color = heat(theme, fraction);
            let cell = match fade {
                None if filled => Some((glyph, Style::new().fg(color))),
                None => None,
                Some(fade) => {
                    let glow = viz.glow_at(i, n, (f32::from(row) + 0.5) / f32::from(rows));
                    if filled {
                        Some((glyph, fade.style(color, glow.max(BODY_FLOOR))))
                    } else if glow >= FADE_FLOOR {
                        let glow = glow * TRAIL;
                        let full = icons.viz_ramp[8];
                        Some((fade.glyph(full, color, glow), fade.style(color, glow)))
                    } else {
                        None
                    }
                }
            };
            if let Some((glyph, style)) = cell {
                buf[(x, y)].set_symbol(glyph).set_style(style);
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

fn mirror(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme, fade: Option<Fade>) {
    let icons = theme.icons;
    let (n, step) = columns(area.width);
    // A center row with the same number of rows above and below it, so both
    // halves mirror exactly (an even height leaves the bottom row unused).
    let arm = (area.height - 1) / 2;
    let center = area.y + arm;
    let full = icons.viz_ramp[8];
    for i in 0..n {
        let x = area.x + i as u16 * step;
        let value = Visualizer::sample(&viz.bands, i, n);
        // Fade intensity at `height` (0..1 along an arm), for a cell the bar
        // covers or for one it has left.
        let glow = |height: f32| viz.glow_at(i, n, height);
        let center_glow = fade.map_or(0.0, |_| glow(0.0));
        if value < 0.02 && center_glow < FADE_FLOOR {
            // Keep a faint center line where it's silent.
            buf[(x, center)]
                .set_symbol(icons.bar_empty)
                .set_style(Style::new().fg(theme.muted));
            continue;
        }
        let center_style = match fade {
            Some(fade) if value < 0.02 => fade.style(theme.accent, center_glow * TRAIL),
            Some(fade) => fade.style(theme.accent, center_glow.max(BODY_FLOOR)),
            None => Style::new().fg(theme.accent),
        };
        buf[(x, center)].set_symbol(full).set_style(center_style);
        // Each arm grows in half cells: a partial end is a half block facing
        // the center (lower half going up, upper half going down).
        let halves = (value * f32::from(arm) * 2.0).round() as u16;
        for row in 0..arm {
            let fraction = f32::from(row + 1) / f32::from(arm);
            let color = heat(theme, fraction);
            let covered = halves.saturating_sub(row * 2);
            let (up, down, style) = if covered > 0 {
                let (up, down) = if covered >= 2 {
                    (full, full)
                } else {
                    (icons.viz_half[1], icons.viz_half[0])
                };
                let style = match fade {
                    Some(fade) => {
                        let g = glow((f32::from(row) + 0.5) / f32::from(arm));
                        fade.style(color, g.max(BODY_FLOOR))
                    }
                    None => Style::new().fg(color),
                };
                (up, down, style)
            } else if let Some(fade) = fade {
                let g = glow((f32::from(row) + 0.5) / f32::from(arm)) * TRAIL;
                if g < FADE_FLOOR {
                    continue;
                }
                let glyph = fade.glyph(full, color, g);
                (glyph, glyph, fade.style(color, g))
            } else {
                break;
            };
            buf[(x, center - 1 - row)].set_symbol(up).set_style(style);
            buf[(x, center + 1 + row)].set_symbol(down).set_style(style);
        }
    }
}

/// The wave's height (-1..1) at horizontal position `t` (0..1): a few
/// travelling sines whose amplitude follows the bands under that position.
/// `lag` looks back in time (for the fading echoes).
fn wave_at(viz: &Visualizer, t: f32, lag: f32) -> f32 {
    let band = Visualizer::sample(&viz.bands, (t * (BANDS - 1) as f32) as usize, BANDS);
    let p = viz.phase - lag;
    let shape = 0.6 * (t * 14.0 - p * 3.1).sin()
        + 0.3 * (t * 31.0 + p * 2.3).sin()
        + 0.1 * (t * 67.0 - p * 5.7).sin();
    // Silent means a flat line; a little motion grows with overall loudness.
    (shape * (0.15 * viz.level + 0.85 * band)).clamp(-1.0, 1.0)
}

/// With fade on, the wave leaves echoes of where it just was (oldest and
/// faintest first), and the line itself dims as the music gets quieter.
const ECHOES: [(f32, f32); 2] = [(0.16, 0.3), (0.08, 0.55)];

fn wave_brightness(viz: &Visualizer) -> f32 {
    (0.4 + viz.level * 1.5).min(1.0)
}

fn wave_braille(
    frame: &mut Frame,
    area: Rect,
    viz: &Visualizer,
    theme: &Theme,
    fade: Option<Fade>,
) {
    // Braille gives 2 dots per cell across; sample at that resolution.
    let samples = usize::from(area.width) * 2;
    let color = heat(theme, viz.level);
    let mut passes = vec![(0.0, color)];
    if let Some(fade) = fade {
        let bright = wave_brightness(viz);
        passes = ECHOES
            .iter()
            .map(|&(lag, dim)| (lag, fade.color(color, bright * dim)))
            .collect();
        passes.push((0.0, fade.color(color, bright)));
    }
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, samples as f64])
        .y_bounds([-1.0, 1.0])
        .paint(|ctx| {
            for &(lag, color) in &passes {
                let mut prev = None;
                for s in 0..=samples {
                    let y = f64::from(wave_at(viz, s as f32 / samples as f32, lag));
                    if let Some((px, py)) = prev {
                        ctx.draw(&CanvasLine::new(px, py, s as f64, y, color));
                    }
                    prev = Some((s as f64, y));
                }
                // Each pass on its own layer, so newer lines draw on top.
                ctx.layer();
            }
        });
    frame.render_widget(canvas, area);
}

fn wave_cells(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme, fade: Option<Fade>) {
    let color = heat(theme, viz.level);
    let mut passes = vec![(0.0, theme.icons.viz_mark, Style::new().fg(color))];
    if let Some(fade) = fade {
        let bright = wave_brightness(viz);
        passes = ECHOES
            .iter()
            .map(|&(lag, dim)| {
                let i = bright * dim;
                (
                    lag,
                    fade.glyph(theme.icons.viz_mark, color, i),
                    fade.style(color, i),
                )
            })
            .collect();
        passes.push((0.0, theme.icons.viz_mark, fade.style(color, bright)));
    }
    for (lag, glyph, style) in passes {
        for col in 0..area.width {
            let t = f32::from(col) / f32::from(area.width.max(2) - 1);
            let y = wave_at(viz, t, lag);
            // -1 at the bottom row, 1 at the top.
            let row = ((1.0 - (y + 1.0) / 2.0) * f32::from(area.height - 1)).round() as u16;
            buf[(area.x + col, area.y + row.min(area.height - 1))]
                .set_symbol(glyph)
                .set_style(style);
        }
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

fn dots_braille(
    frame: &mut Frame,
    area: Rect,
    viz: &Visualizer,
    theme: &Theme,
    fade: Option<Fade>,
) {
    // One dot per cell across (braille would allow two; one reads cleaner).
    let n = usize::from(area.width);
    let rows = f64::from(area.height) * 4.0;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n as f64])
        .y_bounds([0.0, rows])
        .paint(|ctx| {
            if let Some(fade) = fade {
                // Falling dots leave a fading tail above them (every other
                // braille row, so it reads as a trail rather than a bar).
                for (i, h) in dot_heights(viz, n) {
                    let top = f64::from(h) * (rows - 1.0);
                    let mut y = top.ceil() + 2.0;
                    while y < rows {
                        let height = (y / (rows - 1.0)) as f32;
                        let g = viz.glow_at(i, n, height) * TRAIL;
                        if g >= 0.2 {
                            let color = fade.color(heat(theme, height), g);
                            ctx.draw(&Points {
                                coords: &[(i as f64 + 0.5, y)],
                                color,
                            });
                        }
                        y += 2.0;
                    }
                }
                ctx.layer();
            }
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

fn dots_cells(buf: &mut Buffer, area: Rect, viz: &Visualizer, theme: &Theme, fade: Option<Fade>) {
    let (n, step) = columns(area.width);
    let rows = area.height;
    for (i, h) in dot_heights(viz, n) {
        let x = area.x + i as u16 * step;
        let dot_row = (h * f32::from(rows - 1)).round() as u16;
        if let Some(fade) = fade {
            // A fading tail in the cells above the dot.
            for row in dot_row + 1..rows {
                let height = (f32::from(row) + 0.5) / f32::from(rows);
                let g = viz.glow_at(i, n, height) * TRAIL;
                if g >= 0.2 {
                    let color = heat(theme, height);
                    buf[(x, area.bottom() - 1 - row)]
                        .set_symbol(theme.icons.viz_shade[0])
                        .set_style(fade.style(color, g));
                }
            }
        }
        if h < 0.02 {
            continue;
        }
        buf[(x, area.bottom() - 1 - dot_row)]
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
        render_fade(viz, style, false, theme, w, h)
    }

    fn render_fade(
        viz: &Visualizer,
        style: VizStyle,
        fade: bool,
        theme: &Theme,
        w: u16,
        h: u16,
    ) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| draw(f, f.area(), viz, style, fade, theme))
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
            for fade in [false, true] {
                let lines = render_fade(&falling(), style, fade, &theme, 60, 5);
                let all: String = lines.concat();
                assert!(all.is_ascii(), "{style:?}:\n{}", lines.join("\n"));
                assert!(inked(&lines) > 0, "{style:?} drew nothing");
            }
        }
    }

    /// Loud, then the music stops for a moment: bars are dropping.
    fn falling() -> Visualizer {
        let mut v = loud();
        for _ in 0..10 {
            v.tick(1.0 / 30.0, false);
        }
        v
    }

    #[test]
    fn fade_leaves_a_trail_as_bars_drop_and_nothing_at_rest() {
        let theme = Theme::default();
        for style in [VizStyle::Bars, VizStyle::Mirror, VizStyle::Dots] {
            let plain = inked(&render_fade(&falling(), style, false, &theme, 60, 7));
            let faded = render_fade(&falling(), style, true, &theme, 60, 7);
            assert!(
                inked(&faded) > plain,
                "{style:?}: {} vs {plain}:\n{}",
                inked(&faded),
                faded.join("\n")
            );
        }
        let quiet = Visualizer::default();
        for style in VizStyle::ALL {
            let plain = render_fade(&quiet, style, false, &theme, 80, 5);
            let faded = render_fade(&quiet, style, true, &theme, 80, 5);
            assert_eq!(plain, faded, "{style:?} at rest looks the same either way");
        }
    }

    #[test]
    fn fade_blends_true_colors_and_steps_named_ones() {
        let rgb = Theme::preset("nord").unwrap();
        let fade = Fade { theme: &rgb };
        assert_eq!(fade.color(rgb.accent, 1.0), rgb.accent);
        assert_eq!(fade.color(rgb.accent, 0.0), rgb.muted);
        let Color::Rgb(r, ..) = fade.color(rgb.accent, 0.5) else {
            panic!("blended color is rgb");
        };
        let (Color::Rgb(hi, ..), Color::Rgb(lo, ..)) = (rgb.accent, rgb.muted) else {
            unreachable!()
        };
        assert!((lo.min(hi)..=lo.max(hi)).contains(&r));

        let named = Theme::default();
        let fade = Fade { theme: &named };
        assert_eq!(fade.color(named.accent, 0.9), named.accent);
        assert_eq!(fade.color(named.accent, 0.2), named.muted);
        assert!(
            fade.style(named.accent, 0.5)
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert_eq!(fade.glyph("█", named.accent, 0.1), "░");
    }
}
