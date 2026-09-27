//! Mouse support (opt-in via Settings). Every mouse action maps to something
//! the keyboard can already do, so mouse only ever accelerates.
//!
//! - click: focus a pane / select a row / switch tab / seek in the progress bar
//! - double-click: activate (same as Enter)
//! - scroll: move the selection (or scroll help)
//! - drag a pane border: resize the side panes (saved on release; `:resize`
//!   does the same from the keyboard); double-click a border to reset it.
//!   Borders show a grip while mouse support is on and light up on hover.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::action::{Action, Pane, Resize, Seek, Select, View};
use super::state::{BorderDrag, Mode};
use super::{App, pointer};
use crate::ui::layout::PaneSizes;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const HELP_SCROLL_STEP: u16 = 3;

impl App {
    pub(super) fn on_mouse(&mut self, ev: MouseEvent) {
        if !self.state.appearance.mouse {
            return;
        }
        let pos = Position::new(ev.column, ev.row);

        // A drag ends on release wherever that happens (over help, during a
        // prompt...), and a new press ends any drag whose release we missed.
        if matches!(
            ev.kind,
            MouseEventKind::Up(MouseButton::Left) | MouseEventKind::Down(_)
        ) {
            self.end_drag();
        }

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
        if self.state.provider_setup.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = ev.kind
                && !self.state.hits.setup.is_some_and(|r| r.contains(pos))
            {
                self.state.provider_setup = None;
            }
            return;
        }
        // Don't yank focus around while the user is typing in a prompt.
        if self.state.mode != Mode::Normal {
            return;
        }
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.divider_at(pos) {
                Some((zone, pane)) => self.press_border(zone, pane, pos),
                None => self.click(pos),
            },
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(drag) = self.state.divider_drag {
                    self.drag_border(drag, pos.x);
                }
            }
            MouseEventKind::Moved => self.hover(pos),
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
        if self.state.view == View::Providers {
            // One click opens setup (no double-click needed: a card is a button).
            if let Some(&(_, i)) = hits.providers.iter().find(|(r, _)| r.contains(pos)) {
                self.open_provider_setup(i);
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

    /// Shows the resize pointer while a border is hovered or dragged, if the
    /// user opted in (see `pointer`).
    pub(super) fn sync_pointer(&mut self) {
        let a = &self.state.appearance;
        let over_border = self.state.divider_hover.is_some() || self.state.divider_drag.is_some();
        let want = a.mouse && a.resize_cursor && over_border;
        if want != self.pointer_resize {
            self.pointer_resize = want;
            pointer::set_resize(want);
        }
    }

    fn divider_at(&self, pos: Position) -> Option<(Rect, Pane)> {
        let hits = &self.state.hits.dividers;
        hits.iter().find(|(r, _)| r.contains(pos)).copied()
    }

    /// Finishes a border drag, saving only if the sizes actually changed.
    fn end_drag(&mut self) {
        let Some(drag) = self.state.divider_drag.take() else {
            return;
        };
        if self.state.appearance.panes != drag.start {
            self.save_pane_sizes();
        }
        self.sync_pointer();
    }

    /// Highlights a border while the pointer is over it, and says what it does.
    fn hover(&mut self, pos: Position) {
        let over = self.divider_at(pos).map(|(_, pane)| pane);
        if over != self.state.divider_hover {
            self.state.divider_hover = over;
            if over.is_some() && self.state.divider_drag.is_none() {
                self.state.info("Drag to resize, double-click to reset");
            }
            self.sync_pointer();
        }
    }

    /// Press on a border: start dragging it, or reset it on a double-click.
    fn press_border(&mut self, zone: Rect, pane: Pane, pos: Position) {
        let double = self
            .last_click
            .is_some_and(|(p, at)| p == pos && at.elapsed() < DOUBLE_CLICK);
        if double {
            self.last_click = None;
            let default = PaneSizes::default().get(pane).unwrap_or_default();
            self.dispatch(Action::Resize(Resize::Set(pane, default)));
        } else {
            self.last_click = Some((pos, Instant::now()));
            // The zone covers both border cells; the pane's own border is
            // Library's last column (left cell) or Queue's first (right cell).
            let border_x = match pane {
                Pane::Queue => zone.x + 1,
                _ => zone.x,
            };
            self.state.divider_drag = Some(BorderDrag {
                pane,
                start: self.state.appearance.panes,
                grab_offset: i32::from(pos.x) - i32::from(border_x),
            });
            self.sync_pointer();
        }
    }

    /// Resizes the dragged pane so its inner border follows the pointer.
    fn drag_border(&mut self, drag: BorderDrag, pointer_x: u16) {
        let Some(area) = self.state.hits.panes_area else {
            return;
        };
        let x = (i32::from(pointer_x) - drag.grab_offset).clamp(0, i32::from(u16::MAX)) as u16;
        let pane = drag.pane;
        // Library's border is its last column; Queue's is its first.
        let cols = match pane {
            Pane::Library => x.saturating_sub(area.x) + 1,
            Pane::Queue => area.right().saturating_sub(x),
            Pane::Tracks => return,
        };
        let pct = PaneSizes::pct_of(cols, area.width);
        // Clamp against the starting sizes, not the previous motion's result.
        let panes = drag.start.with(pane, pct);
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
    fn clicking_a_provider_opens_its_setup_and_outside_closes_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_saving_to(true, dir.path().join("config.toml"));
        app.dispatch(Action::View(View::Providers));
        draw(&mut app, 100, 30);
        let (card, i) = app.state.hits.providers[2];
        click(&mut app, card.x + 3, card.y + 3);
        assert_eq!(i, 2);
        assert_eq!(
            app.state.provider_setup,
            Some(crate::provider::ProviderKind::AppleMusic)
        );

        draw(&mut app, 100, 30);
        let popup = app.state.hits.setup.unwrap();
        click(&mut app, popup.x + 2, popup.y + 2);
        assert!(
            app.state.provider_setup.is_some(),
            "a click inside keeps it"
        );
        click(&mut app, 0, 0);
        assert!(app.state.provider_setup.is_none(), "outside closes it");
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
    fn hovering_a_border_highlights_it_and_explains() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (border, pane) = app.state.hits.dividers[1];
        mouse(&mut app, MouseEventKind::Moved, border.x, border.y + 2);
        assert_eq!(app.state.divider_hover, Some(pane));
        assert!(
            app.state
                .status
                .as_ref()
                .unwrap()
                .text
                .contains("Drag to resize")
        );
        mouse(&mut app, MouseEventKind::Moved, 50, border.y + 2);
        assert_eq!(app.state.divider_hover, None);
    }

    #[test]
    fn double_clicking_a_border_resets_that_pane() {
        let mut app = app_with_mouse(true);
        app.state.appearance.panes = PaneSizes {
            library: 40,
            queue: 20,
        };
        draw(&mut app, 100, 30);
        let (border, _) = app.state.hits.dividers[0];
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Left),
        ] {
            mouse(&mut app, kind, border.x, border.y + 1);
        }
        assert_eq!(
            app.state.appearance.panes.library,
            PaneSizes::default().library
        );
        assert_eq!(
            app.state.appearance.panes.queue, 20,
            "only that border resets"
        );
    }

    #[test]
    fn resize_pointer_follows_hover_only_when_opted_in() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (border, _) = app.state.hits.dividers[0];
        let over = (border.x, border.y + 2);

        mouse(&mut app, MouseEventKind::Moved, over.0, over.1);
        assert!(!app.pointer_resize, "off by default");
        mouse(&mut app, MouseEventKind::Moved, 50, over.1);

        app.state.appearance.resize_cursor = true;
        mouse(&mut app, MouseEventKind::Moved, over.0, over.1);
        assert!(app.pointer_resize);
        mouse(&mut app, MouseEventKind::Moved, 50, over.1);
        assert!(!app.pointer_resize, "restored when the pointer leaves");

        // Turning the option off while hovering restores it right away.
        mouse(&mut app, MouseEventKind::Moved, over.0, over.1);
        app.state.appearance.resize_cursor = false;
        app.sync_pointer();
        assert!(!app.pointer_resize);
    }

    fn press(app: &mut App, x: u16, y: u16) {
        mouse(app, MouseEventKind::Down(MouseButton::Left), x, y);
    }
    fn drag_to(app: &mut App, x: u16, y: u16) {
        mouse(app, MouseEventKind::Drag(MouseButton::Left), x, y);
    }
    fn release(app: &mut App, x: u16, y: u16) {
        mouse(app, MouseEventKind::Up(MouseButton::Left), x, y);
    }

    #[test]
    fn a_release_over_help_or_a_prompt_still_ends_the_drag() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let (border, _) = app.state.hits.dividers[0];
        press(&mut app, border.x, border.y + 2);
        drag_to(&mut app, 30, border.y + 2);
        app.state.help_open = true; // e.g. `?` pressed mid-drag
        release(&mut app, 30, border.y + 2);
        assert!(app.state.divider_drag.is_none());

        app.state.help_open = false;
        press(&mut app, border.x, border.y + 2);
        app.state.mode = Mode::Command; // `:` pressed mid-drag
        release(&mut app, border.x, border.y + 2);
        assert!(app.state.divider_drag.is_none());
    }

    #[test]
    fn dragging_out_and_back_doesnt_squeeze_the_other_pane() {
        let mut app = app_with_mouse(true);
        draw(&mut app, 100, 30);
        let start = app.state.appearance.panes;
        let (border, _) = app.state.hits.dividers[0];
        press(&mut app, border.x, border.y + 2);
        drag_to(&mut app, 60, border.y + 2); // past the limit: Queue gives way
        assert!(app.state.appearance.panes.queue < start.queue);
        drag_to(&mut app, border.x, border.y + 2); // and back
        assert_eq!(app.state.appearance.panes, start);
    }

    #[test]
    fn clicking_a_border_without_moving_saves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut app = app_saving_to(true, path.clone());
        draw(&mut app, 100, 30);
        let (border, _) = app.state.hits.dividers[0];
        press(&mut app, border.x, border.y + 2);
        release(&mut app, border.x, border.y + 2);
        assert!(!path.exists(), "a plain click must not rewrite the config");
    }

    #[test]
    fn grabbing_either_border_cell_doesnt_jump() {
        for cell in 0..2 {
            let mut app = app_with_mouse(true);
            draw(&mut app, 100, 30);
            let start = app.state.appearance.panes;
            for &(zone, _) in &app.state.hits.dividers.clone() {
                let x = zone.x + cell;
                press(&mut app, x, zone.y + 2);
                drag_to(&mut app, x, zone.y + 2); // motion without moving columns
                assert_eq!(app.state.appearance.panes, start, "cell {cell} of {zone:?}");
                release(&mut app, x, zone.y + 2);
            }
        }
    }

    #[test]
    fn seek_ratio_spans_the_bar() {
        assert_eq!(seek_ratio(10, 11, 10), 0.0);
        assert_eq!(seek_ratio(10, 11, 20), 1.0);
        assert_eq!(seek_ratio(10, 11, 15), 0.5);
        assert_eq!(seek_ratio(10, 1, 10), 0.0);
    }
}
