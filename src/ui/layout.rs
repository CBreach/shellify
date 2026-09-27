//! Responsive screen layout, kept pure so every breakpoint is unit-testable.
//!
//! - 80+ cols:          Library | Tracks | Queue; the side panes take the
//!   user's `PaneSizes` percentages (resizable), Tracks gets the rest
//! - narrow (< 80):      only the focused pane, full width; the header shows
//!   which pane that is
//! - below MIN_WIDTH x MIN_HEIGHT: a "terminal too small" message

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::action::Pane;

pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 12;
/// Now Playing's height without the visualizer (border + two lines).
const NOW_PLAYING_HEIGHT: u16 = 4;
/// Rows the visualizer adds to Now Playing, and the window height it needs.
pub const VIZ_ROWS: u16 = 5;
pub const VIZ_MIN_HEIGHT: u16 = 26;
const NARROW_BELOW: u16 = 80;
/// Narrowest a side pane may get, in columns, however it's resized.
const MIN_SIDE: u16 = 14;
/// Tracks always keeps at least this many columns.
const MIN_TRACKS: u16 = 30;

/// Side pane widths as a percentage of the window; Tracks gets the rest.
/// Set by dragging pane borders (or `:resize`) and saved to config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneSizes {
    pub library: u16,
    pub queue: u16,
}

impl Default for PaneSizes {
    fn default() -> Self {
        Self {
            library: 22,
            queue: 28,
        }
    }
}

impl PaneSizes {
    pub const MIN_PCT: u16 = 10;
    pub const MAX_PCT: u16 = 45;
    /// Library + Queue may use at most this much, leaving Tracks >= 30%.
    const MAX_SIDES_PCT: u16 = 70;

    /// Keeps each side pane in range and leaves room for Tracks, taking any
    /// excess from `keep`'s opposite pane first.
    pub fn clamped(self, keep: Pane) -> Self {
        let mut library = self.library.clamp(Self::MIN_PCT, Self::MAX_PCT);
        let mut queue = self.queue.clamp(Self::MIN_PCT, Self::MAX_PCT);
        let excess = (library + queue).saturating_sub(Self::MAX_SIDES_PCT);
        if keep == Pane::Library {
            queue -= excess;
        } else {
            library -= excess;
        }
        Self { library, queue }
    }

    /// The width of a side pane (Tracks has no width of its own).
    pub fn get(self, pane: Pane) -> Option<u16> {
        match pane {
            Pane::Library => Some(self.library),
            Pane::Queue => Some(self.queue),
            Pane::Tracks => None,
        }
    }

    /// Sets a side pane's width and re-clamps, favoring that pane.
    pub fn with(mut self, pane: Pane, pct: u16) -> Self {
        match pane {
            Pane::Library => self.library = pct,
            Pane::Queue => self.queue = pct,
            Pane::Tracks => {}
        }
        self.clamped(pane)
    }

    /// The percentage of `total` columns that `cols` represents, rounded.
    pub fn pct_of(cols: u16, total: u16) -> u16 {
        if total == 0 {
            return 0;
        }
        ((u32::from(cols) * 100 + u32::from(total) / 2) / u32::from(total)) as u16
    }
}

/// Column widths for (Library, Queue) at `width`, honoring the minimums.
fn side_widths(width: u16, sizes: PaneSizes) -> (u16, u16) {
    let cols = |pct: u16| ((u32::from(width) * u32::from(pct) + 50) / 100) as u16;
    let mut library = cols(sizes.library).max(MIN_SIDE);
    let mut queue = cols(sizes.queue).max(MIN_SIDE);
    // Give columns back to Tracks from the wider side pane first.
    while width.saturating_sub(library + queue) < MIN_TRACKS {
        if library >= queue && library > MIN_SIDE {
            library -= 1;
        } else if queue > MIN_SIDE {
            queue -= 1;
        } else {
            break;
        }
    }
    (library, queue)
}

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

/// `visualizer` asks for room for it in Now Playing; it only gets it when the
/// window is tall enough (`VIZ_MIN_HEIGHT`).
pub fn compute(area: Rect, focus: Pane, sizes: PaneSizes, visualizer: bool) -> Screen {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Screen::TooSmall;
    }
    let viz_rows = if visualizer && area.height >= VIZ_MIN_HEIGHT {
        VIZ_ROWS
    } else {
        0
    };
    let [header, main, now_playing, cmdline] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(NOW_PLAYING_HEIGHT + viz_rows),
        Constraint::Length(1),
    ])
    .areas(area);

    let narrow = area.width < NARROW_BELOW;
    let panes = if narrow {
        vec![(focus, main)]
    } else {
        let (library, queue) = side_widths(main.width, sizes);
        let widths = [
            Constraint::Length(library),
            Constraint::Fill(1),
            Constraint::Length(queue),
        ];
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
        match compute(Rect::new(0, 0, w, h), focus, PaneSizes::default(), false) {
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
    fn side_panes_follow_their_percentages() {
        let a = areas(90, 24, Pane::Tracks);
        assert_eq!(a.panes[0].1.width, 20); // 22% of 90
        assert_eq!(a.panes[2].1.width, 25); // 28% of 90
        assert_eq!(a.panes[1].1.width, 45);

        let wide = PaneSizes {
            library: 40,
            queue: 30,
        };
        let Screen::Normal(a) = compute(Rect::new(0, 0, 100, 30), Pane::Tracks, wide, false) else {
            panic!("fits")
        };
        assert_eq!(
            a.panes.iter().map(|(_, r)| r.width).collect::<Vec<_>>(),
            [40, 30, 30]
        );
    }

    #[test]
    fn visualizer_gets_rows_only_in_tall_windows() {
        let np = |h: u16, viz: bool| match compute(
            Rect::new(0, 0, 100, h),
            Pane::Tracks,
            PaneSizes::default(),
            viz,
        ) {
            Screen::Normal(a) => a.now_playing.height,
            Screen::TooSmall => panic!("fits"),
        };
        assert_eq!(np(30, false), NOW_PLAYING_HEIGHT);
        assert_eq!(np(30, true), NOW_PLAYING_HEIGHT + VIZ_ROWS);
        assert_eq!(
            np(VIZ_MIN_HEIGHT - 1, true),
            NOW_PLAYING_HEIGHT,
            "too short: hidden"
        );
    }

    #[test]
    fn minimums_win_over_percentages() {
        // At 80 columns, 45% + 45% would squeeze Tracks below its minimum.
        let big = PaneSizes {
            library: 45,
            queue: 45,
        };
        let Screen::Normal(a) = compute(Rect::new(0, 0, 80, 24), Pane::Tracks, big, false) else {
            panic!("fits")
        };
        assert!(a.panes[1].1.width >= MIN_TRACKS);
        assert!(a.panes[0].1.width >= MIN_SIDE && a.panes[2].1.width >= MIN_SIDE);
    }

    #[test]
    fn clamping_keeps_room_for_tracks() {
        let s = PaneSizes {
            library: 60,
            queue: 5,
        }
        .clamped(Pane::Library);
        assert_eq!((s.library, s.queue), (45, 10));
        let s = PaneSizes {
            library: 40,
            queue: 40,
        }
        .clamped(Pane::Queue);
        assert_eq!((s.library, s.queue), (30, 40), "the other pane gives way");
        assert_eq!(PaneSizes::pct_of(25, 100), 25);
        assert_eq!(PaneSizes::pct_of(20, 90), 22);
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
            compute(
                Rect::new(0, 0, MIN_WIDTH - 1, 30),
                Pane::Tracks,
                PaneSizes::default(),
                false
            ),
            Screen::TooSmall
        );
        assert_eq!(
            compute(
                Rect::new(0, 0, 120, MIN_HEIGHT - 1),
                Pane::Tracks,
                PaneSizes::default(),
                false
            ),
            Screen::TooSmall
        );
    }
}
