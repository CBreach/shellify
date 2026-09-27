//! Mouse support (opt-in via Settings). Every mouse action maps to something
//! the keyboard can already do, so mouse only ever accelerates.
//!
//! - click: focus a pane / select a row / switch tab / seek in the progress bar
//! - double-click: activate (same as Enter)
//! - scroll: move the selection (or scroll help)
//! - drag a pane border: resize the side panes (saved on release; `:resize`
//!   does the same from the keyboard)

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::App;
use super::action::{Action, Pane, Seek, Select, View};
use super::state::Mode;
use crate::ui::layout::PaneSizes;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const HELP_SCROLL_STEP: u16 = 3;

impl App {
    pub(super) fn on_mouse(&mut self, ev: MouseEvent) {
        if !self.state.appearance.mouse {
            return;
        }
        let pos = Position::new(ev.column, ev.row);

        if self.state.help_open {
            let scroll = &mut self.state.help_scroll;
            match ev.kind {
                MouseEventKind::ScrollDown => *scroll = scroll.saturating_add(HELP_SCROLL_STEP),
                MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(HELP_SCROLL_STEP),
                MouseEventKind::Down(MouseButton::Left)
                    if !self.state.hits.help.is_some_and(|r| r.contains(pos)) =>
                {
                    self.state.help_open = false;
                }
                _ => {}
            }
            return;
        }
        // Don't yank focus around while the user is typing in a prompt.
        if self.state.mode != Mode::Normal {
            return;
        }
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let divider = self
                    .state
                    .hits
                    .dividers
                    .iter()
                    .find(|(r, _)| r.contains(pos));
                if let Some(&(_, pane)) = divider {
                    self.drag = Some(pane);
                    self.last_click = None;
                } else {
                    self.click(pos);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(pane) = self.drag {
                    self.drag_border(pane, pos.x);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // Save once, when the drag ends, not on every motion event.
                if self.drag.take().is_some() {
                    self.save_pane_sizes();
                }
            }
            MouseEventKind::ScrollDown => self.scroll(pos, 1),
            MouseEventKind::ScrollUp => self.scroll(pos, -1),
            _ => {}
        }
    }

    fn click(&mut self, pos: Position) {
        let double = self
            .last_click
            .is_some_and(|(p, at)| p == pos && at.elapsed() < DOUBLE_CLICK);
        // A double-click consumes the pair, so a third click starts over.
        self.last_click = if double {
            None
        } else {
            Some((pos, Instant::now()))
        };

        let hits = &self.state.hits;
        if let Some(&(_, view)) = hits.tabs.iter().find(|(r, _)| r.contains(pos)) {
            self.dispatch(Action::View(view));
            return;
        }
        if let Some(bar) = hits.progress.filter(|r| r.contains(pos)) {
            if let Some(track) = self.state.queue.current() {
                // Same duration the progress bar shows: mpv's exact one if known.
                let duration = self.state.playback.duration.unwrap_or(track.duration);
                let ratio = seek_ratio(bar.x, bar.width, pos.x);
                let target = duration.mul_f64(ratio);
                self.dispatch(Action::Seek(Seek::To(target)));
            }
            return;
        }
        if self.state.view == View::Settings {
            if let Some(&(_, i)) = hits.settings_rows.iter().find(|(r, _)| r.contains(pos)) {
                self.state.settings_cursor = i;
                if double {
                    self.dispatch(Action::PlaySelected);
                }
            }
            return;
        }
        if let Some(&list) = hits.lists.iter().find(|l| l.area.contains(pos)) {
            self.state.focus = list.pane;
            let (_, len) = self.state.selection(list.pane);
            let offset = self.state.list_offset(list.pane);
            if let Some(i) = list.item_at(pos.y, offset).filter(|&i| i < len) {
                self.state.set_selection(list.pane, Some(i));
                if double {
                    self.dispatch(Action::PlaySelected);
                }
            }
        }
    }

    /// Resizes `pane` so its inner border follows column `x`.
    fn drag_border(&mut self, pane: Pane, x: u16) {
        let Some(area) = self.state.hits.panes_area else {
            return;
        };
        // Library's border is its last column; Queue's is its first.
        let cols = match pane {
            Pane::Library => x.saturating_sub(area.x) + 1,
            Pane::Queue => area.right().saturating_sub(x),
            Pane::Tracks => return,
        };
        let pct = PaneSizes::pct_of(cols, area.width);
        let panes = self.state.appearance.panes.with(pane, pct);
        self.state.appearance.panes = panes;
        self.state.info(format!(
            "Library {}%, Queue {}% (release to save)",
            panes.library, panes.queue
        ));
    }

    /// The wheel moves the selection in whatever is under the pointer.
    fn scroll(&mut self, pos: Position, delta: i32) {
        if self.state.view == View::Music {
            let Some(list) = self.state.hits.lists.iter().find(|l| l.area.contains(pos)) else {
                return;
            };
            self.state.focus = list.pane;
        }
        self.dispatch(Action::Select(Select::By(delta)));
    }
}

/// Where along a bar starting at `x` (`width` cells) column `col` falls, 0..=1.
fn seek_ratio(x: u16, width: u16, col: u16) -> f64 {
    if width <= 1 {
        return 0.0;
    }
    (f64::from(col.saturating_sub(x)) / f64::from(width - 1)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::config::Config;
    use crate::ui;

    fn app_with_mouse(on: bool) -> App {
        app_saving_to(on, PathBuf::from("/nonexistent/shellify-test.toml"))
    }

    fn app_saving_to(mouse: bool, path: PathBuf) -> App {
        let mut config = Config::default();
        config.ui.mouse = mouse;
        App::new(&config, path).unwrap()
    }

    fn draw(app: &mut App, w: u16, h: u16) {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| ui::draw(f, &mut app.state, &app.theme))
            .unwrap();
    }

    fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
        app.on_mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn click(app: &mut App, x: u16, y: u16) {
        mouse(app, MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn tracks_row(app: &App, index: u16) -> (u16, u16) {
        let list = app
            .state
            .hits
            .lists
            .iter()
            .find(|l| l.pane == Pane::Tracks)
            .unwrap();
        (list.rows.x + 2, list.rows.y + index)
    }

    #[test]
    fn click_selects_and_double_click_plays() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (x, y) = tracks_row(&app, 2);
        click(&mut app, x, y);
        assert_eq!(app.state.focus, Pane::Tracks);
        assert_eq!(app.state.tracks_state.selected(), Some(2));
        assert!(
            app.state.queue.current().is_none(),
            "single click only selects"
        );

        click(&mut app, x, y);
        assert_eq!(
            app.state.queue.current_index(),
            Some(2),
            "double-click plays"
        );
    }

    #[test]
    fn clicks_are_ignored_when_mouse_is_off() {
        let mut app = app_with_mouse(false);
        draw(&mut app, 100, 30);
        let (x, y) = tracks_row(&app, 2);
        click(&mut app, x, y);
        assert_eq!(app.state.focus, Pane::Library);
        assert_eq!(app.state.tracks_state.selected(), Some(0));
    }

    #[test]
    fn scroll_moves_selection_in_the_pane_under_the_pointer() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (x, y) = tracks_row(&app, 0);
        mouse(&mut app, MouseEventKind::ScrollDown, x, y);
        mouse(&mut app, MouseEventKind::ScrollDown, x, y);
        assert_eq!(app.state.focus, Pane::Tracks);
        assert_eq!(app.state.tracks_state.selected(), Some(2));
    }

    #[test]
    fn clicking_a_tab_switches_view() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (rect, _) = *app
            .state
            .hits
            .tabs
            .iter()
            .find(|(_, v)| *v == View::Settings)
            .unwrap();
        click(&mut app, rect.x + 1, rect.y);
        assert_eq!(app.state.view, View::Settings);

        draw(&mut app, 100, 30);
        let (row, i) = app.state.hits.settings_rows[3];
        click(&mut app, row.x + 1, row.y);
        assert_eq!(app.state.settings_cursor, i);
    }

    #[test]
    fn clicking_the_progress_bar_seeks() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (x, y) = tracks_row(&app, 0);
        click(&mut app, x, y);
        click(&mut app, x, y); // double-click: play the first track
        draw(&mut app, 100, 30);
        let bar = app.state.hits.progress.expect("progress bar drawn");
        click(&mut app, bar.x + bar.width - 1, bar.y);
        let duration = app.state.queue.current().unwrap().duration;
        assert_eq!(app.state.playback.position, duration);
    }

    #[test]
    fn help_scrolls_and_closes_on_outside_click() {
        let mut app = app_with_mouse(true);
        app.state.help_open = true;
        draw(&mut app, 100, 30);
        mouse(&mut app, MouseEventKind::ScrollDown, 50, 15);
        assert_eq!(app.state.help_scroll, HELP_SCROLL_STEP);
        click(&mut app, 0, 0);
        assert!(!app.state.help_open);
    }

    fn pane_width(app: &App, pane: Pane) -> u16 {
        app.state
            .hits
            .lists
            .iter()
            .find(|l| l.pane == pane)
            .unwrap()
            .area
            .width
    }

    #[test]
    fn dragging_a_border_resizes_and_saves_on_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut app = app_saving_to(true, path.clone());
        draw(&mut app, 100, 30);
        assert_eq!(pane_width(&app, Pane::Library), 22);

        let (border, pane) = app.state.hits.dividers[0];
        assert_eq!(pane, Pane::Library);
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            border.x,
            border.y + 3,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            34,
            border.y + 3,
        );
        assert!(!path.exists(), "nothing saved mid-drag");
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            34,
            border.y + 3,
        );

        draw(&mut app, 100, 30);
        assert_eq!(pane_width(&app, Pane::Library), 35);
        assert_eq!(
            app.state.tracks_state.selected(),
            Some(0),
            "drag didn't click"
        );
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("library_width = 35"), "{saved}");
    }

    #[test]
    fn dragging_the_queue_border_left_widens_the_queue() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (border, pane) = app.state.hits.dividers[1];
        assert_eq!(pane, Pane::Queue);
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            border.x + 1,
            border.y,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            60,
            border.y,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            60,
            border.y,
        );
        draw(&mut app, 100, 30);
        assert_eq!(pane_width(&app, Pane::Queue), 40);
        assert!(pane_width(&app, Pane::Tracks) >= 30);
    }

    #[test]
    fn dragging_is_clamped_and_narrow_layouts_have_no_borders() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (border, _) = app.state.hits.dividers[0];
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            border.x,
            border.y,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            99,
            border.y,
        );
        assert_eq!(app.state.appearance.panes.library, PaneSizes::MAX_PCT);
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            0,
            border.y,
        );
        assert_eq!(app.state.appearance.panes.library, PaneSizes::MIN_PCT);

        draw(&mut app, 60, 24);
        assert!(app.state.hits.dividers.is_empty());
    }

    #[test]
    fn seek_ratio_spans_the_bar() {
        assert_eq!(seek_ratio(10, 11, 10), 0.0);
        assert_eq!(seek_ratio(10, 11, 20), 1.0);
        assert_eq!(seek_ratio(10, 11, 15), 0.5);
        assert_eq!(seek_ratio(10, 1, 10), 0.0);
    }
}
