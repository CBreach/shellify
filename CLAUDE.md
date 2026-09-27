# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Shellify is a Rust terminal music client: a full-screen TUI with a vim-style `:` command mode that plays music from streaming providers. The plan is at `~/.claude/plans/hey-claudio-lets-start-shimmying-deer.md`.

What exists now (roadmap step 1): the TUI skeleton, the command parser, the keymap, the config, the queue and the event loop, running on placeholder data from `src/app/demo.rs`. Playback is **simulated**: `App::on_tick` in `src/app/dispatch.rs` advances the position. The `Player` and `Provider` traits described below don't exist yet; they arrive in steps 2 and 3. Update this file as they land.

## Commands

```sh
cargo build
cargo run                              # launches the TUI
cargo test                             # all tests
cargo test <name_substring>            # single test, e.g. cargo test parse_seek
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Runtime dependencies (not Rust crates): `mpv` and `yt-dlp` must be on `PATH` (`brew install mpv yt-dlp`).

Smoke-testing the TUI headlessly: run it in tmux (`tmux new-session -d -s t -x 110 -y 22 ./target/debug/shellify`), drive it with `tmux send-keys`, and read the screen with `tmux capture-pane -p`. Send typed text with `send-keys -l`, because tmux otherwise treats words like `home` or `end` as key names. Also pause between Esc and the next key, or crossterm reads the pair as Alt+key. For logs, set `SHELLIFY_LOG=shellify=debug`; the log is written to `~/Library/Caches/shellify/shellify.log`, and every dispatched `Action` is logged at debug level.

## Architecture

- **Everything funnels through one `Action` enum** (`src/app/action.rs`). Normal-mode keybindings (`src/keymap.rs`) and `:commands` (`src/command/parser.rs`) both produce `Action`s, which `App::dispatch` (`src/app/dispatch.rs`) applies. Every key binding, default or user-defined, is stored as a *command string* and parsed by the same parser. To add a feature, add an `Action` variant, a command in `parse` (plus `COMMANDS` for Tab completion), and optionally a default key in `DEFAULT_BINDINGS`. `:` (open the prompt) and Ctrl-C (quit) are hard-wired in `src/app/mod.rs` and can't be rebound.
- **Modes:** Normal, Command (`:`) and Search (`/`). Both prompts use `command::LineEditor` (history, char-aware cursor). While a prompt is open, the status message is shown right-aligned as a hint and cleared on the next key.
- **UI** (`src/ui/`) only draws: it takes `&mut AppState` because ratatui's `ListState`/`TableState` (scroll offsets) live in the state.
- **Single process, single tokio runtime, one `mpsc` event channel.** Terminal input, player events (mpv property changes such as `time-pos`, `pause`, EOF) and async provider results all arrive as `AppEvent`s on the same channel; the app updates state and re-renders at about 30fps. Never block the event loop; long provider calls run as spawned tasks that report back through the channel.
- **`Provider` trait (`src/provider/`)**: auth status, search, library playlists, playlist tracks, liked tracks, and `resolve_playback(track) -> PlaybackSource`. `PlaybackSource::Url{url, headers}` is played locally by mpv; `PlaybackSource::Remote` is for providers that play elsewhere (Spotify Connect, the macOS Music.app for Apple Music). Keep provider-specific types out of the app core.
- **`Player` trait (`src/player/`)**: `MpvPlayer` spawns `mpv --idle --no-video --input-ipc-server=<sock>` and talks JSON IPC over a `UnixStream`. mpv's yt-dlp hook resolves YouTube URLs; auth cookies go through `--ytdl-raw-options=cookies=<file>`. On quit, the mpv child and the socket file must be cleaned up.
- **The queue is owned by the app core** (`src/app/queue.rs`), not by mpv's playlist, so it stays provider-agnostic.
- **Logging** uses `tracing` and goes to a file, never to stdout or stderr, because the TUI owns the terminal.

## Providers

- **YouTube Music (MVP)** uses `ytmapi-rs` (unofficial API). Auth is browser headers/cookies pasted in via `:login ytmusic`. Secrets live in the OS keychain (`keyring`), and a Netscape cookie file with mode 0600 in the app cache dir is shared with yt-dlp. These endpoints are unofficial and break periodically, so keep that code isolated behind the `Provider` trait.
- **Spotify (next)**: Web API (OAuth PKCE) for metadata, plus librespot as a second `Player` backend (needs Premium).
- **Apple Music (deferred)**: planned as a macOS-only `Remote` provider that controls Music.app through JXA.

## Config

User config lives at `~/.config/shellify/config.toml` (keymap overrides, theme, default provider). Paths come from the `directories` crate.
