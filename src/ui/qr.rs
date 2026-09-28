//! QR codes drawn with half blocks (two modules per cell, top and bottom),
//! and clickable links (OSC 8 hyperlinks).

use qrcode::{EcLevel, QrCode};
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Light modules around the code; scanners need a margin (the spec asks for
/// 4, but 2 scans fine on a screen and saves rows).
const QUIET: usize = 2;

/// A QR code's modules, `true` for dark.
pub struct Qr {
    width: usize,
    dark: Vec<bool>,
}

impl Qr {
    pub fn new(text: &str) -> Option<Self> {
        let code = QrCode::with_error_correction_level(text, EcLevel::L).ok()?;
        Some(Self {
            width: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|c| c == qrcode::Color::Dark)
                .collect(),
        })
    }

    /// Cells it takes: (columns, rows), quiet zone included.
    pub fn size(&self) -> (u16, u16) {
        let side = self.width + 2 * QUIET;
        (side as u16, side.div_ceil(2) as u16)
    }

    fn is_dark(&self, x: usize, y: usize) -> bool {
        // Coordinates include the quiet zone, which is light.
        let (Some(x), Some(y)) = (x.checked_sub(QUIET), y.checked_sub(QUIET)) else {
            return false;
        };
        x < self.width && y < self.width && self.dark[y * self.width + x]
    }

    /// Text rows: light modules are drawn as blocks and dark ones left
    /// blank, so it reads correctly on the dark background it's drawn on.
    pub fn rows(&self) -> Vec<String> {
        let side = self.width + 2 * QUIET;
        (0..side)
            .step_by(2)
            .map(|y| {
                (0..side)
                    .map(|x| {
                        let top = !self.is_dark(x, y);
                        let bottom = y + 1 < side && !self.is_dark(x, y + 1);
                        match (top, bottom) {
                            (true, true) => '█',
                            (true, false) => '▀',
                            (false, true) => '▄',
                            (false, false) => ' ',
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// Draws it at `area`'s top left. With `color`, the colors are set
    /// explicitly (white on black) so it scans on light terminals too.
    pub fn render(&self, buf: &mut Buffer, area: Rect, color: bool) {
        let style = if color {
            Style::new().fg(Color::White).bg(Color::Black)
        } else {
            Style::new()
        };
        for (dy, row) in self.rows().iter().enumerate() {
            let y = area.y + dy as u16;
            if y >= area.bottom() {
                break;
            }
            buf.set_stringn(area.x, y, row, usize::from(area.width), style);
        }
    }
}

/// Makes the first occurrence of `url` inside `area` a clickable link, for
/// terminals that support OSC 8 (others show the text unchanged). The link
/// escape codes ride along with the cells' symbols, two characters per cell
/// with the next cell skipped, so they don't count toward the width.
pub fn link(buf: &mut Buffer, area: Rect, url: &str) {
    if !url.is_ascii() || url.contains('\x1b') {
        return;
    }
    for y in area.top()..area.bottom() {
        let row: String = (area.left()..area.right())
            .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect();
        let Some(start) = row.find(url) else {
            continue;
        };
        let x0 = area.x + start as u16;
        for (i, chunk) in url.as_bytes().chunks(2).enumerate() {
            let x = x0 + 2 * i as u16;
            let text = std::str::from_utf8(chunk).unwrap_or_default();
            buf[(x, y)].set_symbol(&format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\"));
            if chunk.len() == 2 {
                buf[(x + 1, y)].set_diff_option(CellDiffOption::Skip);
            }
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_page_code_is_small_enough_for_a_popup() {
        let qr = Qr::new("https://www.google.com/device").unwrap();
        let (w, h) = qr.size();
        assert!(w <= 33 && h <= 17, "{w}x{h}");
        let rows = qr.rows();
        assert_eq!(rows.len(), usize::from(h));
        assert!(rows.iter().all(|r| r.chars().count() == usize::from(w)));
        // The quiet zone is light all round: a full-block top row.
        assert!(rows[0].chars().all(|c| c == '█'));
    }

    /// What a phone sees: the rendered rows read back as pixels (block
    /// glyphs lit, blanks dark, the way they look on a dark terminal) and
    /// decoded by a real QR reader.
    #[test]
    fn rendered_rows_scan_back_to_the_url() {
        let url = "https://www.google.com/device";
        let rows = Qr::new(url).unwrap().rows();
        const SCALE: usize = 4;
        let width = rows[0].chars().count() * SCALE;
        let height = rows.len() * 2 * SCALE;
        let lit = |x: usize, y: usize| {
            let c = rows[y / SCALE / 2].chars().nth(x / SCALE).unwrap();
            let top = (y / SCALE).is_multiple_of(2);
            match c {
                '█' => true,
                '▀' => top,
                '▄' => !top,
                _ => false,
            }
        };
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
            if lit(x, y) { 255 } else { 0 }
        });
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1, "one code found");
        let (_, text) = grids[0].decode().unwrap();
        assert_eq!(text, url);
    }

    #[test]
    fn finder_patterns_are_where_scanners_look() {
        let qr = Qr::new("https://www.google.com/device").unwrap();
        // Each corner's finder pattern starts with a dark 7-module edge.
        for (x, y) in [(0, 0), (qr.width - 7, 0), (0, qr.width - 7)] {
            for i in 0..7 {
                assert!(qr.is_dark(QUIET + x + i, QUIET + y), "top edge at {x},{y}");
                assert!(qr.is_dark(QUIET + x, QUIET + y + i), "left edge at {x},{y}");
            }
        }
    }

    #[test]
    fn links_wrap_the_url_cells_in_osc_8() {
        let area = Rect::new(0, 0, 40, 2);
        let mut buf = Buffer::empty(area);
        buf.set_string(3, 1, "go to https://x.example now", Style::new());
        link(&mut buf, area, "https://x.example");
        let first = buf[(9, 1)].symbol();
        assert!(
            first.starts_with("\x1b]8;;https://x.example\x1b\\ht"),
            "{first:?}"
        );
        assert_eq!(buf[(10, 1)].diff_option, CellDiffOption::Skip);
        // The last chunk is a single character, with nothing skipped after it.
        assert!(buf[(25, 1)].symbol().contains("e\x1b]8;;"));
        assert_eq!(buf[(26, 1)].diff_option, CellDiffOption::None);
        assert_eq!(buf[(27, 1)].symbol(), "n");
    }
}
