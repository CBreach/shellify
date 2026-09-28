mod cmdline;
mod header;
mod help;
pub mod icons;
pub mod layout;
mod library;
mod logos;
mod now_playing;
mod providers;
mod queue;
mod settings;
mod text;
pub mod theme;
mod tracks;
mod visualizer;

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::action::{Pane, View};
use crate::app::state::{AppState, HitMap, ListHit};
use layout::Screen;
use theme::Theme;

pub fn draw(frame: &mut Frame, state: &mut AppState, theme: &Theme) {
    // Base style so `text` applies everywhere widgets don't override it.
    frame.render_widget(
        Block::new().style(Style::new().fg(theme.text)),
        frame.area(),
    );

    state.hits = HitMap::default();
    let areas = match layout::compute(
        frame.area(),
        state.focus,
        state.appearance.panes,
        state.appearance.visualizer,
    ) {
        Screen::Normal(areas) => areas,
        Screen::TooSmall => {
            draw_too_small(frame, theme);
            return;
        }
    };

    state.hits.tabs = header::draw(frame, areas.header, state, theme, areas.narrow);
    match state.view {
        View::Settings => {
            state.hits.settings_rows = settings::draw(frame, areas.main, state, theme);
        }
        View::Providers => {
            state.hits.providers = providers::draw(frame, areas.main, state, theme);
        }
        View::Music => {
            for &(pane, area) in &areas.panes {
                match pane {
                    Pane::Library => library::draw(frame, area, state, theme),
                    Pane::Tracks => tracks::draw(frame, area, state, theme),
                    Pane::Queue => queue::draw(frame, area, state, theme),
                }
                state.hits.lists.push(list_hit(pane, area));
            }
            if !areas.narrow {
                state.hits.dividers = dividers(&areas.panes, areas.main);
                state.hits.panes_area = Some(areas.main);
                if state.appearance.mouse {
                    let active = state.divider_drag.map(|d| d.pane).or(state.divider_hover);
                    draw_grips(frame, &state.hits.dividers, active, theme);
                }
            }
        }
    }
    state.hits.progress = now_playing::draw(frame, areas.now_playing, state, theme);
    cmdline::draw(frame, areas.cmdline, state, theme);
    if let Some(kind) = state.provider_setup {
        state.hits.setup = Some(providers::draw_setup(frame, state, theme, kind));
    }
    if state.help_open {
        state.hits.help = Some(help::draw(frame, state, theme));
    }
}

/// Grab zones for resizing: the two border columns where Library meets
/// Tracks, and where Tracks meets Queue.
fn dividers(panes: &[(Pane, Rect)], main: Rect) -> Vec<(Rect, Pane)> {
    let [(_, library), (_, tracks), _] = panes else {
        return Vec::new();
    };
    let zone = |x: u16| Rect::new(x, main.y, 2, main.height);
    vec![
        (zone(library.right() - 1), Pane::Library),
        (zone(tracks.right() - 1), Pane::Queue),
    ]
}

/// Marks pane borders as draggable: a small grip in the middle of each, and
/// while one is hovered or dragged, the whole border heavy and accented.
/// Weight and glyphs carry the cue too, so it still reads without color.
fn draw_grips(frame: &mut Frame, dividers: &[(Rect, Pane)], active: Option<Pane>, theme: &Theme) {
    let icons = theme.icons;
    let lit = Style::new().fg(theme.accent).add_modifier(Modifier::BOLD);
    let buf = frame.buffer_mut();
    for &(zone, pane) in dividers {
        if zone.height < 3 {
            continue;
        }
        let on = active == Some(pane);
        if on {
            // Skip the top and bottom rows, which hold the border corners.
            for y in zone.y + 1..zone.bottom() - 1 {
                for x in zone.x..zone.right() {
                    buf[(x, y)]
                        .set_symbol(icons.border_focus.vertical_left)
                        .set_style(lit);
                }
            }
        }
        let grip_style = if on {
            lit
        } else {
            Style::new().fg(theme.muted)
        };
        let mid = zone.y + zone.height / 2;
        for (x, glyph) in (zone.x..).zip(icons.grip) {
            buf[(x, mid)].set_symbol(glyph).set_style(grip_style);
        }
    }
}

/// Where a pane's items sit on screen: inside the border, below the tracks
/// table's header row, two lines per queue entry.
fn list_hit(pane: Pane, area: Rect) -> ListHit {
    let inner = area.inner(Margin::new(1, 1));
    let (rows, item_height) = match pane {
        Pane::Library => (inner, 1),
        Pane::Tracks => (
            Rect {
                y: inner.y + 1,
                height: inner.height.saturating_sub(1),
                ..inner
            },
            1,
        ),
        Pane::Queue => (inner, 2),
    };
    ListHit {
        pane,
        area,
        rows,
        item_height,
    }
}

/// Shown instead of a garbled layout when the window can't fit the UI.
fn draw_too_small(frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    let msg = Paragraph::new(vec![
        Line::from("Terminal too small").style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(format!(
            "need {}x{}, have {}x{}",
            layout::MIN_WIDTH,
            layout::MIN_HEIGHT,
            area.width,
            area.height
        ))
        .style(Style::new().fg(theme.muted)),
    ])
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    let [middle] = Layout::vertical([Constraint::Length(2)])
        .flex(Flex::Center)
        .areas(area);
    frame.render_widget(msg, middle);
}

/// A bordered pane, highlighted when focused.
fn pane_block(title: String, pane: Pane, state: &AppState, theme: &Theme) -> Block<'static> {
    let focused = state.focus == pane;
    let (border, title_style) = if focused {
        let accent = Style::new().fg(theme.accent);
        (accent, accent.add_modifier(Modifier::BOLD))
    } else {
        (Style::new().fg(theme.muted), Style::new().fg(theme.text))
    };
    // Without color, a heavier border marks the focused pane.
    let border_set = if focused && theme.mono {
        theme.icons.border_focus
    } else {
        theme.icons.border
    };
    Block::bordered()
        .border_set(border_set)
        .border_style(border)
        .title(Line::from(format!(" {title} ")).style(title_style))
}

fn highlight_style(pane: Pane, state: &AppState, theme: &Theme) -> Style {
    if state.focus == pane {
        theme.selection()
    } else {
        Style::new().add_modifier(Modifier::REVERSED)
    }
}

/// `m:ss`, or `h:mm:ss` for long tracks.
fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::provider::{Playlist, Source, Track};

    fn render(w: u16, h: u16, theme: &Theme) -> String {
        let track = Track {
            id: "1".into(),
            title: "A Rather Long Song Title For Truncation".into(),
            artist: "Some Artist".into(),
            duration: Duration::from_secs(200),
            source: Source::Demo,
        };
        let mut state = AppState::new(vec![Playlist {
            id: "liked".into(),
            name: "Liked Songs".into(),
            tracks: Some(vec![track.clone()]),
        }]);
        state.queue.play_list(vec![track], 0);
        state.active_provider = Some(crate::provider::ProviderKind::YouTubeMusic);
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, &mut state, theme)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .chunks(w as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_at_every_contract_size() {
        let theme = Theme::default();
        for (w, h) in [(160, 48), (100, 30), (80, 24), (60, 24), (40, 12)] {
            let screen = render(w, h, &theme);
            assert!(screen.contains("Now Playing"), "{w}x{h}:\n{screen}");
            assert!(screen.contains("Music"), "{w}x{h}");
        }
    }

    #[test]
    fn narrow_header_lists_panes_and_tiny_shows_message() {
        let theme = Theme::default();
        assert!(render(60, 24, &theme).contains("Library  Tracks  Queue"));
        assert!(!render(100, 30, &theme).contains("Library  Tracks  Queue"));
        for (w, h) in [(39, 24), (80, 11), (10, 3)] {
            assert!(render(w, h, &theme).contains("too small"), "{w}x{h}");
        }
    }

    #[test]
    fn settings_tab_renders_and_keeps_selection_visible() {
        let theme = Theme::default();
        let mut state = AppState::new(Vec::new());
        state.view = View::Settings;
        for (w, h) in [(100, 30), (60, 24), (40, 12)] {
            state.settings_cursor = crate::app::settings::SettingRow::ALL.len() - 1;
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &mut state, &theme)).unwrap();
            let screen: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(screen.contains("Settings"), "{w}x{h}");
            assert!(
                screen.contains("Reset all settings"),
                "{w}x{h}: selected row scrolled off"
            );
        }
    }

    fn render_state(state: &mut AppState, w: u16, h: u16, theme: &Theme) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, state, theme)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn borders_show_a_grip_only_with_mouse_on_and_light_up_on_hover() {
        let theme = Theme::default();
        let mut state = AppState::new(Vec::new());
        assert!(!render_state(&mut state, 100, 30, &theme).contains("◂▸"));

        state.appearance.mouse = true;
        let screen = render_state(&mut state, 100, 30, &theme);
        assert_eq!(screen.matches("◂▸").count(), 2, "one grip per border");
        assert!(!screen.contains('┃'), "no highlight until hovered");

        state.divider_hover = Some(Pane::Queue);
        let screen = render_state(&mut state, 100, 30, &theme);
        assert!(screen.contains("┃┃"), "hovered border is drawn heavy");

        // No grips in the one-pane layout, where there's nothing to resize.
        assert!(!render_state(&mut state, 60, 24, &theme).contains("◂▸"));
    }

    #[test]
    fn ascii_pack_renders_only_ascii() {
        let theme = Theme {
            icons: icons::IconPack::Ascii.icons(),
            ..Theme::default()
        };
        let mut mouse_state = AppState::new(Vec::new());
        mouse_state.appearance.mouse = true;
        mouse_state.divider_hover = Some(Pane::Library);
        let screen = render_state(&mut mouse_state, 100, 30, &theme);
        assert!(screen.is_ascii(), "grips and hover highlight are ascii too");
        mouse_state.help_open = true;
        let screen = render_state(&mut mouse_state, 100, 40, &theme);
        assert!(
            screen.contains("drag a pane border (<>)"),
            "help names the pack's grip"
        );
        assert!(screen.is_ascii(), "help overlay is ascii too");
        for (w, h) in [(100, 30), (60, 24)] {
            let screen = render(w, h, &theme);
            let bad: String = screen.chars().filter(|c| !c.is_ascii()).collect();
            assert!(bad.is_empty(), "{w}x{h} non-ascii: {bad:?}");
        }
    }

    #[test]
    fn providers_tab_renders_at_every_contract_size() {
        let mut state = AppState::new(Vec::new());
        state.view = View::Providers;
        state.provider_cursor = 2;
        for theme in [Theme::default(), Theme::default().monochrome()] {
            for (w, h) in [(160, 48), (100, 30), (80, 24), (60, 20), (60, 24), (40, 12)] {
                let screen = render_state(&mut state, w, h, &theme);
                assert!(
                    screen.contains("Apple Music"),
                    "{w}x{h}: highlighted provider shown"
                );
                assert!(screen.contains("Providers"), "{w}x{h}");
                assert_eq!(state.hits.providers.len(), 3, "{w}x{h}: all clickable");
            }
        }
    }

    #[test]
    fn providers_tab_and_setup_are_ascii_with_the_ascii_pack() {
        let theme = Theme {
            icons: icons::IconPack::Ascii.icons(),
            ..Theme::default()
        };
        let mut state = AppState::new(Vec::new());
        state.view = View::Providers;
        for mono in [false, true] {
            let theme = if mono { theme.monochrome() } else { theme };
            for (w, h) in [(160, 48), (100, 30), (60, 20)] {
                state.provider_setup = None;
                let screen = render_state(&mut state, w, h, &theme);
                assert!(screen.is_ascii(), "{w}x{h} mono={mono}");
                state.provider_setup = Some(crate::provider::ProviderKind::YouTubeMusic);
                let screen = render_state(&mut state, w, h, &theme);
                assert!(screen.is_ascii(), "{w}x{h} mono={mono} setup");
            }
        }
    }

    #[test]
    fn provider_setup_screen_renders_over_any_tab() {
        let mut state = AppState::new(Vec::new());
        state.provider_setup = Some(crate::provider::ProviderKind::Spotify);
        for (w, h) in [(160, 48), (80, 24), (40, 12)] {
            let screen = render_state(&mut state, w, h, &Theme::default());
            assert!(screen.contains("Set up Spotify"), "{w}x{h}:\n{screen}");
            assert!(state.hits.setup.is_some());
        }
    }

    #[test]
    fn demo_library_says_so_and_how_to_add_a_provider() {
        let mut state = AppState::new(crate::app::demo_library());
        state.providers_key = Some("3".into());
        for theme in [Theme::default(), Theme::default().monochrome()] {
            let screen = render_state(&mut state, 120, 30, &theme);
            assert!(screen.contains("Demo tracks"), "Library pane notice");
            assert!(screen.contains("provider: press 3."), "{screen}");
            assert!(screen.contains("demo tracks · press 3 to add a provider"));
            assert!(
                state.hits.tabs.iter().any(|(_, v)| *v == View::Providers),
                "the badge is clickable"
            );
            // One pane at a time: the header still says so.
            let narrow = render_state(&mut state, 60, 24, &theme);
            assert!(narrow.contains("demo tracks"), "{narrow}");
        }
        let ascii = Theme {
            icons: icons::IconPack::Ascii.icons(),
            ..Theme::default()
        };
        for (w, h) in [(120, 30), (80, 24), (60, 24)] {
            assert!(render_state(&mut state, w, h, &ascii).is_ascii(), "{w}x{h}");
        }

        state.active_provider = Some(crate::provider::ProviderKind::YouTubeMusic);
        let screen = render_state(&mut state, 120, 30, &Theme::default());
        assert!(!screen.to_lowercase().contains("demo tracks"));
    }

    #[test]
    fn loading_library_and_tracks_say_so() {
        let mut state = AppState::new(Vec::new());
        state.active_provider = Some(crate::provider::ProviderKind::YouTubeMusic);
        state.library_loading = true;
        state.tracks_title = "Liked".into();
        state.tracks_loading = true;
        let screen = render_state(&mut state, 120, 30, &Theme::default());
        assert!(screen.contains("Loading your library…"), "{screen}");
        assert!(screen.contains("Loading…"));
        assert!(!screen.contains("0 tracks"), "no count until loaded");
        let ascii = Theme {
            icons: icons::IconPack::Ascii.icons(),
            ..Theme::default()
        };
        assert!(render_state(&mut state, 120, 30, &ascii).is_ascii());
    }

    #[test]
    fn a_provider_in_use_says_so_on_its_card_and_setup_screen() {
        let mut state = AppState::new(Vec::new());
        state.view = View::Providers;
        state.active_provider = Some(crate::provider::ProviderKind::YouTubeMusic);
        let screen = render_state(&mut state, 160, 48, &Theme::default());
        assert!(screen.contains(" on "), "card shows on");
        assert!(
            screen.contains("planned"),
            "Spotify keeps its roadmap status"
        );
        state.provider_setup = state.active_provider;
        let screen = render_state(&mut state, 100, 30, &Theme::default());
        assert!(screen.contains("YouTube Music is on"), "{screen}");
        assert!(screen.contains(":provider off"));
    }

    #[test]
    fn formats_durations() {
        assert_eq!(fmt_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(fmt_duration(Duration::from_secs(224)), "3:44");
        assert_eq!(fmt_duration(Duration::from_secs(3723)), "1:02:03");
    }
}
