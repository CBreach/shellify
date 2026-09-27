# Shellify

[![CI](https://github.com/CBreach/shellify/actions/workflows/ci.yml/badge.svg)](https://github.com/CBreach/shellify/actions/workflows/ci.yml)

Listen to your music from the terminal. Shellify is a keyboard-driven, full-screen TUI with vim-style navigation and a `:` command mode. It connects to your own streaming accounts.

> **Status: early development.** The interface, keys and commands work, and audio plays through mpv. There's no account sign-in yet: the built-in demo playlists play the top YouTube search result for each song, and `:open` plays any URL or local file. The roadmap below lists what's coming next.

## Roadmap

- [x] TUI skeleton: panes, vim keys, `:` command mode, `/` search, queue, repeat/shuffle, configurable keys
- [x] Real audio playback through mpv
- [ ] YouTube Music: sign in, search, your library, playlists and liked songs
- [ ] Spotify (Premium required for in-terminal playback)
- [ ] Apple Music (macOS)

## Requirements

- macOS or Linux. Playback controls mpv over a Unix socket, so Windows isn't supported yet.
- Rust (stable, edition 2024): <https://rustup.rs>
- [mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp), both on your `PATH`. Shellify runs mpv in the background; yt-dlp is what lets mpv play YouTube.

```sh
# macOS
brew install mpv yt-dlp
# Debian/Ubuntu (distro yt-dlp packages go stale fast; pipx keeps it current)
sudo apt install mpv pipx && pipx install yt-dlp
```

If either is missing, Shellify still starts and tells you what to install.

Shellify adapts to the window size. It shows three panes from 80 columns up and one pane at a time below that (switch with `h`/`l`), and needs at least 40×12.

## Build & run

```sh
git clone https://github.com/CBreach/shellify.git
cd shellify
cargo run --release
```

## Usage

### Keys (normal mode)

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Move down / up |
| `gg` / `G` | Jump to top / bottom |
| `h` / `l`, `Tab` | Switch pane |
| `Enter` | Open playlist / play track |
| `Space` | Play / pause |
| `n` / `p` | Next / previous track |
| `←` / `→` | Seek −5s / +5s |
| `+` / `-` | Volume up / down |
| `a` | Add selection to queue |
| `s` | Shuffle queue |
| `r` | Cycle repeat (off → all → one) |
| `/` | Search |
| `?` | Help: every key and command |
| `1` / `2` | Music / Settings tab |
| `:` | Command mode |
| `q` | Quit |

### Commands

Press `:` then type a command. Tab completes command names, and ↑/↓ browse history.

| Command | Description |
|---|---|
| `:play [query]` | Play the selection, or search and play |
| `:search <query>` | Search |
| `:pause` | Toggle pause |
| `:next`, `:prev` | Skip |
| `:seek 1:30` / `+10` / `-10` | Seek to a time or by an offset |
| `:vol 60` / `+5` / `-5` | Set or change volume |
| `:repeat [off\|all\|one]` | Set or cycle repeat |
| `:shuffle`, `:clear` | Shuffle / clear the queue |
| `:open <url-or-file>` | Play a URL (anything yt-dlp supports) or a local audio file |
| `:queue`, `:focus <pane>` | Jump to a pane |
| `:help` | Show the help overlay |
| `:settings`, `:view <music\|settings>` | Switch tab |
| `:q` | Quit |

## Configuration

Shellify reads `~/.config/shellify/config.toml` (or `$XDG_CONFIG_HOME/shellify/config.toml`). Any key can be rebound to a command:

```toml
[keys]
"ctrl-n" = "next"
"ctrl-p" = "prev"
"s" = ""          # empty string unbinds the key
```

### Settings tab

Press `2` (or `:settings`) to open **Settings**. There you can pick a theme preset, change any color, switch icon pack and set the color mode, and you see the result as you go. Use `j`/`k` to move between rows and `h`/`l` (or ←/→) to change a value. Press `Enter` on a color to type a name or `#hex`. Press `1` or `Esc` to go back to the music. Every change is saved to your `config.toml`, and your key bindings and comments there are left untouched.

### Themes

Pick a preset (`default`, `nord`, `gruvbox`, `catppuccin`) and optionally override individual colors. A color can be a name (`cyan`, `light-blue`), `#rrggbb` or an ANSI index (`0`–`255`).

```toml
[theme]
preset = "catppuccin"
accent = "#f5c2e7"      # focused borders, selection, progress bar
# text = "reset"        # main text (reset = your terminal's default)
# muted = "dark-gray"   # unfocused borders, secondary info
# error = "red"
# selection_fg = "black"  # text drawn on the accent color
```

### Color

Shellify honors [`NO_COLOR`](https://no-color.org) and `TERM=dumb`. Without color, it marks focus with a heavier border, selection with reverse video, and status with symbols.

```toml
[ui]
color = "auto"     # auto (default) | always | never
icons = "unicode"  # unicode (default) | ascii | nerd
mouse = false      # click, double-click and scroll; also in the Settings tab
```

`ascii` draws everything with plain ASCII, including the borders, for legacy terminals and serial consoles. `nerd` uses [Nerd Font](https://www.nerdfonts.com) glyphs and requires a Nerd Font in your terminal. It's opt-in because terminals can't report which font they use.

### Mouse

Mouse support is off by default. Turn it on under **Settings → Mouse** (or set `mouse = true` in `[ui]`). While it's on, your terminal's own click-and-drag text selection needs **Shift** held in most terminals. With it on, you can click to focus a pane or select a row, double-click to play or open, scroll to move the selection, click the header tabs, and click the progress bar to seek. Every mouse action also has a key.

## Your accounts & privacy

Shellify signs in to *your* accounts and ships with no credentials. Anything you provide (cookies, tokens, API client IDs) stays on your machine: in your OS keychain or your user config/cache directories, never in the project folder.

Logs are written to your cache directory (`~/Library/Caches/shellify/` on macOS). Set `SHELLIFY_LOG=shellify=debug` for more detail.

## Contributing

Every change goes through a pull request into `main`. CI runs `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` on Linux and macOS, and a PR can only merge once the **All checks** job passes. Run the same checks locally before pushing:

```sh
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test
```

## License

[MIT](LICENSE)
