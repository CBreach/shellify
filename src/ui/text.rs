use std::borrow::Cow;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Shortens `s` to at most `max` terminal cells, ending with `…` when cut.
pub fn truncate(s: &str, max: usize) -> Cow<'_, str> {
    if s.width() <= max {
        return Cow::Borrowed(s);
    }
    if max == 0 {
        return Cow::Borrowed("");
    }
    let mut out = String::new();
    let mut width = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if width + w > max - 1 {
            break;
        }
        out.push(c);
        width += w;
    }
    out.push('…');
    Cow::Owned(out)
}

/// Splits a `width`-cell progress bar at `ratio` into (filled, knob, empty)
/// cell counts. The knob always takes one cell when there is room.
pub fn progress_split(ratio: f64, width: usize) -> (usize, usize, usize) {
    if width == 0 {
        return (0, 0, 0);
    }
    let filled = (ratio.clamp(0.0, 1.0) * (width - 1) as f64).round() as usize;
    (filled, 1, width - 1 - filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_leaves_short_text_alone() {
        assert_eq!(truncate("Nightcall", 9), "Nightcall");
    }

    #[test]
    fn truncate_adds_ellipsis_within_width() {
        assert_eq!(truncate("Harder, Better", 8), "Harder,…");
        assert_eq!(truncate("abc", 1), "…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn truncate_counts_wide_chars_as_two_cells() {
        // Each CJK char is 2 cells: 2 chars + ellipsis = 5 cells.
        let t = truncate("夜に駆ける", 6);
        assert_eq!(t, "夜に…");
        assert!(t.width() <= 6);
    }

    #[test]
    fn progress_split_bounds() {
        assert_eq!(progress_split(0.0, 10), (0, 1, 9));
        assert_eq!(progress_split(1.0, 10), (9, 1, 0));
        assert_eq!(progress_split(0.5, 11), (5, 1, 5));
        assert_eq!(progress_split(0.5, 0), (0, 0, 0));
    }
}
