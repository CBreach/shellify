//! Pixel-art provider logos in a 32-bit-era style: banded lighting, a dark
//! rim, a glint and drop shadows under the symbol. They're drawn from simple
//! geometry at whatever size fits, so there are no image files and every
//! size is crisp. `ui::providers` turns the pixels into half-block cells.

use crate::provider::ProviderKind;

pub type Rgb = (u8, u8, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pixel {
    Clear,
    /// The logo's badge (circle or rounded square).
    Body(Rgb),
    /// The symbol on the badge (play button, sound waves, note). Without
    /// color it's left as a hole in the badge so the shape still reads.
    Mark(Rgb),
}

pub struct Logo {
    pub size: usize,
    pixels: Vec<Pixel>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Outside,
    Body,
    Mark,
}

impl Logo {
    /// `kind`'s logo, `size` pixels square.
    pub fn new(kind: ProviderKind, size: usize) -> Self {
        let px = 2.0 / size as f32;
        let center = |i: usize| -1.0 + (i as f32 + 0.5) * px;
        let shapes: Vec<Shape> = (0..size * size)
            .map(|i| shape(kind, center(i % size), center(i / size), px))
            .collect();
        let at = |x: isize, y: isize| {
            if x < 0 || y < 0 || x >= size as isize || y >= size as isize {
                Shape::Outside
            } else {
                shapes[y as usize * size + x as usize]
            }
        };

        let pixels = (0..size * size)
            .map(|i| {
                let (x, y) = ((i % size) as isize, (i / size) as isize);
                let (u, v) = (center(i % size), center(i / size));
                match at(x, y) {
                    Shape::Outside => Pixel::Clear,
                    Shape::Mark => Pixel::Mark(mark_color(kind, v)),
                    Shape::Body => {
                        // Light from the top left, in visible bands.
                        let light = quantize(1.0 - 0.22 * (u + v) / 2.0, 0.07);
                        let mut color = scale(body_color(kind, v), light);
                        let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                            .iter()
                            .any(|(dx, dy)| at(x + dx, y + dy) == Shape::Outside);
                        if edge {
                            color = scale(color, 0.72);
                        } else if at(x - 1, y - 1) == Shape::Mark {
                            // The symbol casts a shadow down and to the right.
                            color = scale(color, 0.78);
                        }
                        // A small glint near the top-left edge.
                        let (gu, gv) = (u + 0.52, v + 0.52);
                        if !edge && gu * gu + gv * gv < 0.17f32.max(px * 0.8).powi(2) {
                            color = mix(color, (255, 255, 255), 0.45);
                        }
                        Pixel::Body(color)
                    }
                }
            })
            .collect();
        Self { size, pixels }
    }

    pub fn get(&self, x: usize, y: usize) -> Pixel {
        if x < self.size && y < self.size {
            self.pixels[y * self.size + x]
        } else {
            Pixel::Clear
        }
    }
}

/// Which part of the logo the point (`u`, `v`) falls in; both run -1..1,
/// `v` downwards. `px` is one pixel's width, so thin strokes never vanish
/// at small sizes (a stroke at least a pixel wide always hits a sample).
fn shape(kind: ProviderKind, u: f32, v: f32, px: f32) -> Shape {
    let in_circle = u * u + v * v <= 1.0;
    let (body, mark) = match kind {
        ProviderKind::YouTubeMusic => {
            // A ring around a play button.
            let r = (u * u + v * v).sqrt();
            let ring = (r - 0.6).abs() <= 0.075f32.max(px * 0.5);
            let play = u >= -0.2 && v.abs() <= 0.33 * (0.36 - u) / 0.56;
            (in_circle, ring || play)
        }
        ProviderKind::Spotify => {
            // Three sound waves, widest at the top, tilting down slightly.
            let arcs = [
                (-0.30, 0.36, 0.64, 0.16),
                (0.03, 0.30, 0.54, 0.13),
                (0.33, 0.24, 0.44, 0.11),
            ];
            let wave = arcs.iter().any(|&(y, curve, half_width, thick)| {
                let center = y + curve * u * u + 0.08 * u;
                // Thickness across the curve, not just vertically, so the
                // steeper ends don't break up.
                let slope = 2.0 * curve * u + 0.08;
                let reach = f32::max(thick, px) / 2.0 * (1.0 + slope * slope).sqrt();
                u.abs() <= half_width && (v - center).abs() <= reach
            });
            (in_circle, wave)
        }
        ProviderKind::AppleMusic => {
            // A rounded square with a pair of beamed eighth notes.
            let corner = 0.42;
            let qx = (u.abs() - (1.0 - corner)).max(0.0);
            let qy = (v.abs() - (1.0 - corner)).max(0.0);
            let square = qx * qx + qy * qy <= corner * corner;
            let stem_w = 0.11f32.max(px);
            let stem = |x: f32, top: f32, bottom: f32| {
                u >= x && u <= x + stem_w && v >= top && v <= bottom
            };
            let (left, right) = (-0.19, 0.35);
            let beam_top = -0.44 + (u - left) * (-0.12 / (right - left));
            let beam = u >= left
                && u <= right + stem_w
                && v >= beam_top
                && v <= beam_top + 0.17f32.max(px);
            let (head_w, head_h) = (0.21f32.max(px * 1.6), 0.15f32.max(px * 1.2));
            let head = |cx: f32, cy: f32| {
                let (dx, dy) = ((u - cx) / head_w, (v - cy) / head_h);
                dx * dx + dy * dy <= 1.0
            };
            // Each head sits at the bottom of its stem, to the left.
            let note = stem(left, -0.44, 0.34)
                || stem(right, -0.56, 0.22)
                || beam
                || head(left + stem_w - head_w, 0.34)
                || head(right + stem_w - head_w, 0.22);
            (square, note)
        }
    };
    match (body, mark) {
        (false, _) => Shape::Outside,
        (true, true) => Shape::Mark,
        (true, false) => Shape::Body,
    }
}

/// The badge's color at height `v` (Apple Music's runs light to deep pink).
fn body_color(kind: ProviderKind, v: f32) -> Rgb {
    match kind {
        ProviderKind::YouTubeMusic => (0xff, 0x00, 0x22),
        ProviderKind::Spotify => (0x1e, 0xd7, 0x60),
        ProviderKind::AppleMusic => mix((0xff, 0x70, 0x9a), (0xf5, 0x2a, 0x5e), (v + 1.0) / 2.0),
    }
}

fn mark_color(kind: ProviderKind, v: f32) -> Rgb {
    match kind {
        ProviderKind::Spotify => (0x12, 0x12, 0x12),
        // White, shading slightly grey towards the bottom.
        _ => {
            let c = (255.0 * quantize(1.0 - 0.1 * (v + 1.0) / 2.0, 0.04)) as u8;
            (c, c, c)
        }
    }
}

fn quantize(x: f32, step: f32) -> f32 {
    (x / step).round() * step
}

fn scale((r, g, b): Rgb, k: f32) -> Rgb {
    let f = |c: u8| (f32::from(c) * k).clamp(0.0, 255.0) as u8;
    (f(r), f(g), f(b))
}

/// `a` blended toward `b` by `t` (0..1).
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(logo: &Logo, pred: impl Fn(Pixel) -> bool) -> usize {
        (0..logo.size)
            .flat_map(|y| (0..logo.size).map(move |x| (x, y)))
            .filter(|&(x, y)| pred(logo.get(x, y)))
            .count()
    }

    /// Text preview of a logo, for eyeballing in test output.
    fn preview(logo: &Logo) -> String {
        (0..logo.size)
            .map(|y| {
                (0..logo.size)
                    .map(|x| match logo.get(x, y) {
                        Pixel::Clear => "  ",
                        Pixel::Body(_) => "██",
                        Pixel::Mark(_) => "░░",
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_logo_keeps_its_symbol_at_every_size() {
        for kind in ProviderKind::ALL {
            for size in [16, 24, 32] {
                let logo = Logo::new(kind, size);
                let total = size * size;
                let body = count(&logo, |p| matches!(p, Pixel::Body(_)));
                let mark = count(&logo, |p| matches!(p, Pixel::Mark(_)));
                let text = preview(&logo);
                assert!(body > total / 3, "{kind:?} {size}: badge\n{text}");
                assert!(mark > total / 20, "{kind:?} {size}: symbol\n{text}");
                assert!(body + mark < total, "{kind:?} {size}: has corners\n{text}");
            }
        }
    }

    #[test]
    fn logos_are_shaded_not_flat() {
        for kind in ProviderKind::ALL {
            let logo = Logo::new(kind, 24);
            let mut colors: Vec<Rgb> = (0..24)
                .flat_map(|y| (0..24).map(move |x| (x, y)))
                .filter_map(|(x, y)| match logo.get(x, y) {
                    Pixel::Body(c) => Some(c),
                    _ => None,
                })
                .collect();
            colors.sort();
            colors.dedup();
            assert!(colors.len() >= 5, "{kind:?}: {} shades", colors.len());
        }
    }

    #[test]
    fn play_button_points_right() {
        let logo = Logo::new(ProviderKind::YouTubeMusic, 32);
        // In the middle row, the triangle is solid from its flat left edge
        // to its tip; the column just left of the flat edge is badge.
        let row = 16;
        let marks: Vec<usize> = (8..24)
            .filter(|&x| matches!(logo.get(x, row), Pixel::Mark(_)))
            .collect();
        assert!(marks.len() >= 6, "{}", preview(&logo));
        let top_row_marks = (8..24)
            .filter(|&x| matches!(logo.get(x, 12), Pixel::Mark(_)))
            .count();
        assert!(top_row_marks < marks.len(), "narrows away from the middle");
    }
}
