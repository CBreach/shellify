# Shellify

Listen to your music from the terminal. Shellify is a keyboard-driven, full-screen TUI with vim-style navigation and a `:` command mode. It connects to your own streaming accounts.

> **Status: early development.** The interface, keys and commands work, but playback is simulated on demo playlists. The roadmap below lists what's coming next.

## Roadmap

- [x] TUI skeleton: panes, vim keys, `:` command mode, `/` search, queue, repeat/shuffle, configurable keys
- [ ] Real audio playback through mpv
- [ ] YouTube Music: sign in, search, your library, playlists and liked songs
- [ ] Spotify (Premium required for in-terminal playback)
- [ ] Apple Music (macOS)

## Requirements

- Rust (stable, edition 2024): <https://rustup.rs>
- [mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp) for playback

```sh
# macOS
brew install mpv yt-dlp
```

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
| `:queue`, `:focus <pane>` | Jump to a pane |
| `:q` | Quit |

## Configuration

Shellify reads `~/.config/shellify/config.toml` (or `$XDG_CONFIG_HOME/shellify/config.toml`). Any key can be rebound to a command:

```toml
[keys]
"ctrl-n" = "next"
"ctrl-p" = "prev"
"s" = ""          # empty string unbinds the key
```

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

## Your accounts & privacy

Shellify signs in to *your* accounts and ships with no credentials. Anything you provide (cookies, tokens, API client IDs) stays on your machine: in your OS keychain or your user config/cache directories, never in the project folder.

Logs are written to your cache directory (`~/Library/Caches/shellify/` on macOS). Set `SHELLIFY_LOG=shellify=debug` for more detail.

## License

[MIT](LICENSE)
