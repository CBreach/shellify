//! Responsive screen layout, kept pure so every breakpoint is unit-testable.
//!
//! - wide (>= 100 cols): Library | Tracks | Queue, proportional
//! - standard (80-99):   Library | Tracks | Queue, side panes at fixed widths
//! - narrow (< 80):      only the focused pane, full width; the header shows
//!   which pane that is
//! - below MIN_WIDTH x MIN_HEIGHT: a "terminal too small" message

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::action::Pane;

pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 12;
const NARROW_BELOW: u16 = 80;
const WIDE_FROM: u16 = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum Screen {
    TooSmall,
    Normal(Areas),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Areas {
    pub header: Rect,
    /// Everything between the header and Now Playing (the Settings tab uses it whole).
    pub main: Rect,
    /// The panes on screen, left to right.
    pub panes: Vec<(Pane, Rect)>,
    pub now_playing: Rect,
    pub cmdline: Rect,
    /// Only one pane fits; the header shows the pane switcher.
    pub narrow: bool,
}

pub fn compute(area: Rect, focus: Pane) -> Screen {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Screen::TooSmall;
    }
    let [header, main, now_playing, cmdline] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .areas(area);

    let narrow = area.width < NARROW_BELOW;
    let panes = if narrow {
        vec![(focus, main)]
    } else {
        let widths = if area.width >= WIDE_FROM {
            [
                Constraint::Percentage(22),
                Constraint::Percentage(50),
                Constraint::Percentage(28),
            ]
        } else {
            [
                Constraint::Length(20),
                Constraint::Fill(1),
                Constraint::Length(26),
            ]
        };
        let rects: [Rect; 3] = Layout::horizontal(widths).areas(main);
        Pane::ALL.into_iter().zip(rects).collect()
    };

    Screen::Normal(Areas {
        header,
        main,
        panes,
        now_playing,
        cmdline,
        narrow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn areas(w: u16, h: u16, focus: Pane) -> Areas {
        match compute(Rect::new(0, 0, w, h), focus) {
            Screen::Normal(a) => a,
            Screen::TooSmall => panic!("{w}x{h} should fit"),
        }
    }

    fn panes(a: &Areas) -> Vec<Pane> {
        a.panes.iter().map(|(p, _)| *p).collect()
    }

    #[test]
    fn wide_and_standard_show_all_three_panes() {
        for (w, h) in [(160, 40), (100, 30), (80, 24)] {
            let a = areas(w, h, Pane::Library);
            assert_eq!(panes(&a), Pane::ALL, "{w}x{h}");
            assert!(!a.narrow);
            let total: u16 = a.panes.iter().map(|(_, r)| r.width).sum();
            assert_eq!(total, w, "{w}x{h} panes should fill the width");
        }
    }

    #[test]
    fn standard_keeps_side_panes_fixed() {
        let a = areas(90, 24, Pane::Tracks);
        assert_eq!(a.panes[0].1.width, 20);
        assert_eq!(a.panes[2].1.width, 26);
        assert_eq!(a.panes[1].1.width, 44);
    }

    #[test]
    fn narrow_shows_only_the_focused_pane() {
        for focus in Pane::ALL {
            let a = areas(60, 24, focus);
            assert!(a.narrow);
            assert_eq!(panes(&a), [focus]);
            assert_eq!(a.panes[0].1.width, 60);
        }
    }

    #[test]
    fn minimum_size_fits_and_below_it_does_not() {
        let a = areas(MIN_WIDTH, MIN_HEIGHT, Pane::Tracks);
        assert!(a.panes[0].1.height >= 4);
        assert_eq!(
            compute(Rect::new(0, 0, MIN_WIDTH - 1, 30), Pane::Tracks),
            Screen::TooSmall
        );
        assert_eq!(
            compute(Rect::new(0, 0, 120, MIN_HEIGHT - 1), Pane::Tracks),
            Screen::TooSmall
        );
    }
}
