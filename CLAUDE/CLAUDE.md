# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Shellify is a Rust terminal music client: a full-screen TUI with a vim-style `:` command mode that plays music from streaming providers. The original architecture plan is in `CLAUDE/PLAN.md` (local-only, gitignored).

What exists now: the TUI (panes, vim keys, `:` command mode, `/` search, help overlay, footer hints, responsive layout, themes, icon packs, `NO_COLOR`, Settings tab) and **real playback through mpv** (`src/player/`). The library is still placeholder data from `src/app/demo.rs`. Demo tracks have no real source, so `demo::playback_source` turns each into `ytdl://ytsearch1:<artist> <title>` (the top YouTube search result). Step 3 (YouTube Music) replaces that function with the `Provider` trait described below, which doesn't exist yet. Update this file as it lands.

## Handoff (read first)

`CLAUDE/HANDOFF.md` is a **local-only** session log: current state, the last feature worked on and whether it finished, open problems with a plan for each, and next steps.
- Read it at the start of every session.
- Update it after every major change (each feature commit, merge or significant decision) and before ending a session, so it survives a crash.
- Never commit it. It's gitignored, along with `CLAUDE/PLAN.md`.

## Public repo: credentials never live in the repo

Shellify is a public, open-source project meant for anyone to clone and use with their own accounts. Consequences:
- **No credentials in the source tree, ever.** That covers API keys, OAuth client secrets, tokens, cookies, pasted browser headers and session files. Each user supplies their own at runtime. Secrets are stored in the OS keychain (`keyring` crate); derived files that tools need (such as the yt-dlp cookie file) go in the user's cache dir with mode 0600, never under the repo.
- **Never embed a developer's personal credentials as defaults.** When a provider needs an app registration (for example a Spotify client ID for OAuth PKCE), the user brings their own through `~/.config/shellify/config.toml`, and the README says how to get one.
- Don't log secrets: redact tokens and cookies in `tracing` output.
- Test fixtures must use obviously fake values.
- Before committing, check `git diff --cached` for anything secret-looking. `.gitignore` also blocks common credential filenames as a backstop.

## Commits

**Every feature goes through a pull request.** Branch from `main` (`feature/<name>`), commit after each small working step, push the branch, and open a PR for the user to review. Never push features straight to `main`, and don't merge PRs unless the user asks. **Do not add `Co-Authored-By` or any other AI/tool attribution to commit messages or PR descriptions.**

CI (`.github/workflows/ci.yml`) runs fmt, clippy (`-D warnings`) and tests on ubuntu and macOS for every PR and every push to main. Branch protection on `main` requires the **All checks** job to pass and the branch to be up to date before merging.

## Commands

```sh
cargo build
cargo run                              # launches the TUI
cargo test                             # all tests
cargo test <name_substring>            # single test, e.g. cargo test parse_seek
cargo test -- --ignored                # tests that need a real mpv (plays a generated tone, no network or audio device)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Runtime dependencies (not Rust crates): `mpv` and `yt-dlp` must be on `PATH` (`brew install mpv yt-dlp`).

Smoke-testing the TUI headlessly: run it in tmux (`tmux new-session -d -s t -x 110 -y 22 ./target/debug/shellify`), drive it with `tmux send-keys`, and read the screen with `tmux capture-pane -p`. Send typed text with `send-keys -l`, because tmux otherwise treats words like `home` or `end` as key names. Also pause between Esc and the next key, or crossterm reads the pair as Alt+key. For logs, set `SHELLIFY_LOG=shellify=debug`; the log is written to `~/Library/Caches/shellify/shellify.log`, and every dispatched `Action` is logged at debug level.

## Architecture

- **Everything funnels through one `Action` enum** (`src/app/action.rs`). Normal-mode keybindings (`src/keymap.rs`) and `:commands` (`src/command/parser.rs`) both produce `Action`s, which `App::dispatch` (`src/app/dispatch.rs`) applies. Every key binding, default or user-defined, is stored as a *command string* and parsed by the same parser. To add a feature, add an `Action` variant, a command in `parse` (plus `COMMANDS` for Tab completion), and optionally a default key in `DEFAULT_BINDINGS`. `:` (open the prompt) and Ctrl-C (quit) are hard-wired in `src/app/mod.rs` and can't be rebound.
- **Discoverability is derived from the keymap:** the `?` help overlay (`Keymap::help_entries`, plus `COMMANDS` metadata in `src/command/parser.rs`) and the per-pane footer hints (`pane_hints` in `src/app/mod.rs`) read the live bindings, so user overrides show up automatically. Label new default bindings in `describe()` in `src/keymap.rs`. Status messages fade after a few seconds (`Status::expired`) so the hints return.
- **Tabs and the Settings tab:**
  - The header shows `1 Music` / `2 Settings`. They're switched with `Action::View`, `:view <music|settings>` or `:settings`, and `Esc` returns to Music.
  - `src/app/settings.rs` holds the model: `Appearance` (theme config, color mode, icon pack), the `SettingRow` list, and how each value cycles.
  - `src/ui/settings.rs` renders it, with previews.
  - On the Settings tab, `App::settings_action` reinterprets the navigation actions, so user remaps keep working:
    - select moves between rows;
    - focus/seek next/prev change the value;
    - play activates the row.
  - `Mode::EditSetting(row)` is the prompt for typing a color by name or `#hex`.
  - Every change goes through `App::apply_appearance`, which rebuilds the theme and calls `config::save_appearance`. That uses `toml_edit` to keep the user's comments and `[keys]`, writes atomically (temp file, then rename), and leaves default values out.
- **Modes:** Normal, Command (`:`), Search (`/`) and EditSetting. Both prompts use `command::LineEditor` (history, char-aware cursor). While a prompt is open, the status message is shown right-aligned as a hint and cleared on the next key.
- **Responsive layout** lives in `src/ui/layout.rs` as a pure `compute(area, focus, sizes)`. From 80 cols up it shows three panes: the side panes take the user's `PaneSizes` percentages (set by dragging a border or with `:resize`, saved as `[ui] library_width`/`queue_width`, and clamped so each pane keeps at least 14 cols and Tracks at least 30); narrow (<80) shows only the focused pane, with a pane switcher in the header; below 40x12 you get a "terminal too small" screen. `src/ui/mod.rs` has TestBackend render tests at each of these sizes. Keep them passing when you change the UI.
- **Visualizer:**
  - `src/app/visualizer.rs` is a pure, deterministic model (its own xorshift, time passed in): overall loudness plus bands, with peaks and attack/decay.
  - `src/ui/visualizer.rs` draws the bars, mirror, wave and dots styles. Glyphs come from `icons.viz_*`, and `icons.braille` switches between canvas braille and cells.
  - Loudness comes from mpv:
    - `Player::set_metering` adds or removes an `af` `astats` filter labelled `@shellify-meter`;
    - the player observes `af-metadata/shellify-meter`, which arrives as `PlayerEvent::Levels`;
    - metering runs only while the visualizer is on.
  - The layout gives Now Playing `VIZ_ROWS` extra rows only when the window has at least `VIZ_MIN_HEIGHT` rows.
  - The animation clock (`spawn_animator`, driven by a `watch<bool>`) sends `AppEvent::Frame` at 30fps only while the visualizer is on and either playing or not yet at rest (`App::sync_animation` after each event batch).
- **Custom themes** (`src/themes.rs`): `*.toml` files in `themes/` next to the config file (`themes_dir(config_path)`), loaded at startup into `state.user_themes` and resolved by `Theme::named` (a custom theme overrides a built-in with the same id). `:theme import` validates a Shellify `.toml` or converts a base16 `.yaml`, then writes a normalized `<id>.toml` and never overwrites. A broken file is skipped with a warning; a config that names a missing theme falls back to default. `theme_ids()` lists what the Settings Theme row cycles through.
- **Theme = colors + icon pack + mono flag.** Never hard-code colors or glyphs in `src/ui/`. Use `theme.<color>`, `theme.selection()` and `theme.icons.*`, so `NO_COLOR` and the ascii pack keep working.
- **UI** (`src/ui/`) only draws: it takes `&mut AppState` because ratatui's `ListState`/`TableState` (scroll offsets) live in the state.
- **Single process, single tokio runtime, one `mpsc` event channel.** Terminal input, player events (mpv property changes such as `time-pos`, `pause`, EOF) and async provider results all arrive as `AppEvent`s on the same channel. The app redraws after each batch of events, plus a 250ms tick that animates the loading spinner. Never block the event loop; long provider calls run as spawned tasks that report back through the channel.
- **`Provider` trait (`src/provider/`)**: auth status, search, library playlists, playlist tracks, liked tracks, and `resolve_playback(track) -> PlaybackSource`. `PlaybackSource::Url{url, headers}` is played locally by mpv; `PlaybackSource::Remote` is for providers that play elsewhere (Spotify Connect, the macOS Music.app for Apple Music). Keep provider-specific types out of the app core.
- **`Player` trait (`src/player/mod.rs`)**: `load`, `set_pause`, `seek`, `set_volume`, `stop` and `shutdown`. The app starts the player in `App::run` and shuts it down after the loop ends. Player events arrive as `AppEvent::Player` and are handled by `App::on_player_event`.
  - **`MpvPlayer` (`src/player/mpv.rs`)** runs `mpv --idle --no-video ...` with an IPC socket in the temp dir. A writer task sends commands; a reader task parses mpv's messages (codec in `src/player/ipc.rs`).
  - **Loads and stale events.** Each `load` gets a `LoadId`, sent as the IPC `request_id`. The reader's `Tracker` maps mpv playlist entries back to that load, following `redirect` entries, which is how `ytsearch` resolves. The app only acts on `Loaded`/`Ended` for its `current_load`, so a late EOF or "stop" from a replaced track can't skip twice. Keep that invariant.
  - **Position and duration** events are throttled in the tracker and ignored by the app while `playback.loading` is set.
  - **Failures:** tracks that fail are skipped, and playback stops after `MAX_FAILURES` failures in a row.
  - **Missing binaries:** if mpv is missing or dies, `app.player` is `None` and the UI keeps working.
  - **Cleanup:** mpv leaves its socket file behind, so `shutdown` (and `Drop`) remove it; the child is `kill_on_drop`.
  - **Future:** auth cookies for YouTube Music will go through `--ytdl-raw-options=cookies=<file>`.
- **Testing app logic without mpv:** `dispatch.rs` tests use a `FakePlayer` that records calls, and `mpv.rs` tests drive `MpvPlayer::attach` against a fake mpv on a `UnixStream::pair`.
- **The queue is owned by the app core** (`src/app/queue.rs`), not by mpv's playlist, so it stays provider-agnostic.
- **Logging** uses `tracing` and goes to a file, never to stdout or stderr, because the TUI owns the terminal.

## Providers

- **YouTube Music (MVP)** uses `ytmapi-rs` (unofficial API). Auth is browser headers/cookies pasted in via `:login ytmusic`. Secrets live in the OS keychain (`keyring`), and a Netscape cookie file with mode 0600 in the app cache dir is shared with yt-dlp. These endpoints are unofficial and break periodically, so keep that code isolated behind the `Provider` trait.
- **Spotify (next)**: Web API (OAuth PKCE) for metadata, plus librespot as a second `Player` backend (needs Premium).
- **Apple Music (deferred)**: planned as a macOS-only `Remote` provider that controls Music.app through JXA.

## Config

User config lives at `~/.config/shellify/config.toml` (keymap overrides, theme, default provider). Paths come from the `directories` crate.
