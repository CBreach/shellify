use std::borrow::Cow;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Shortens `s` to at most `max` terminal cells, ending with `ellipsis` when cut.
pub fn truncate<'a>(s: &'a str, max: usize, ellipsis: &str) -> Cow<'a, str> {
    if s.width() <= max {
        return Cow::Borrowed(s);
    }
    let ell = ellipsis.width();
    if max < ell {
        return Cow::Owned(".".repeat(max));
    }
    let mut out = String::new();
    let mut width = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if width + w > max - ell {
            break;
        }
        out.push(c);
        width += w;
    }
    out.push_str(ellipsis);
    Cow::Owned(out)
}

/// Word-wraps `s` into lines of at most `width` cells. Words longer than a
/// line are truncated rather than split.
pub fn wrap(s: &str, width: usize, ellipsis: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let word = truncate(word, width, ellipsis);
        let needed = if line.is_empty() { 0 } else { line.width() + 1 };
        if !line.is_empty() && needed + word.width() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The loading spinner's current frame, driven by the clock (the app redraws
/// on every tick).
pub fn spinner(frames: &[&'static str]) -> &'static str {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    frames[(millis / 250 % frames.len() as u128) as usize]
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
        assert_eq!(truncate("Nightcall", 9, "…"), "Nightcall");
    }

    #[test]
    fn truncate_adds_ellipsis_within_width() {
        assert_eq!(truncate("Harder, Better", 8, "…"), "Harder,…");
        assert_eq!(truncate("Harder, Better", 8, "..."), "Harde...");
        assert_eq!(truncate("abc", 1, "…"), "…");
        assert_eq!(truncate("abcd", 2, "..."), "..");
        assert_eq!(truncate("abc", 0, "…"), "");
    }

    #[test]
    fn truncate_counts_wide_chars_as_two_cells() {
        // Each CJK char is 2 cells: 2 chars + ellipsis = 5 cells.
        let t = truncate("夜に駆ける", 6, "…");
        assert_eq!(t, "夜に…");
        assert!(t.width() <= 6);
    }

    #[test]
    fn wrap_breaks_between_words_within_width() {
        assert_eq!(
            wrap("These are demo tracks for trying things", 12, "…"),
            ["These are", "demo tracks", "for trying", "things"]
        );
        assert_eq!(wrap("a verylongword b", 5, "…"), ["a", "very…", "b"]);
        assert!(wrap("", 10, "…").is_empty());
        for line in wrap("夜に駆ける 夜に駆ける", 6, "…") {
            assert!(line.width() <= 6, "{line}");
        }
    }

    #[test]
    fn progress_split_bounds() {
        assert_eq!(progress_split(0.0, 10), (0, 1, 9));
        assert_eq!(progress_split(1.0, 10), (9, 1, 0));
        assert_eq!(progress_split(0.5, 11), (5, 1, 5));
        assert_eq!(progress_split(0.5, 0), (0, 0, 0));
    }
}
