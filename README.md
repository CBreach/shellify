# Shellify

[![CI](https://github.com/CBreach/shellify/actions/workflows/ci.yml/badge.svg)](https://github.com/CBreach/shellify/actions/workflows/ci.yml)

Listen to your music from the terminal. Shellify is a keyboard-driven, full-screen TUI with vim-style navigation and a `:` command mode. It connects to your own streaming accounts.

> **Status: early development.** The interface, keys and commands work, and audio plays through mpv. **YouTube Music works signed out**: search it and play songs, no account needed. Sign-in, for your own playlists and liked songs, is next. Until you turn a provider on, Shellify shows a demo library, labelled as such. `:open` plays any URL or local file. The roadmap below lists what's coming.

## Roadmap

- [x] TUI skeleton: panes, vim keys, `:` command mode, `/` search, queue, repeat/shuffle, configurable keys
- [x] Real audio playback through mpv
- [x] YouTube Music, signed out: search and play
- [ ] YouTube Music sign-in: your library, playlists and liked songs
- [ ] Spotify (Premium required for in-terminal playback)
- [ ] Apple Music (macOS)

## Requirements

- macOS or Linux. Playback controls mpv over a Unix socket, so Windows isn't supported yet.
- Rust (stable, edition 2024): <https://rustup.rs>
- [mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp), both on your `PATH`. Shellify runs mpv in the background; yt-dlp is what lets mpv play YouTube. yt-dlp also wants a JavaScript runtime such as [deno](https://deno.com) for YouTube.

```sh
# macOS
brew install mpv yt-dlp deno
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
| `v` / `V` | Toggle the visualizer / next style |
| `1` / `2` / `3` | Music / Settings / Providers tab |
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
| `:settings`, `:providers`, `:view <music\|settings\|providers>` | Switch tab |
| `:provider youtube-music`, `:provider off` | Turn YouTube Music on, or go back to the demo tracks |
| `:resize library 30`, `:resize queue +5`, `:resize reset` | Set a side pane's width (% of the window) |
| `:theme <name>`, `:theme import <file>`, `:theme reload` | Switch, import or reload color themes |
| `:visualizer [on\|off\|next\|bars\|mirror\|wave\|dots]` | Toggle the visualizer or pick its style |
| `:visualizer fade [on\|off]` | Make the visualizer fade in and out |
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

### Providers tab

Press `3` (or `:providers`) to see the streaming services Shellify supports: YouTube Music, Spotify and Apple Music, each with a pixel-art logo. The highlighted one bounces, and each card says whether it's on. Move with `h`/`l` (or the arrows, or `j`/`k`), then press `Enter` (or click a card, with the mouse on):

- **YouTube Music** switches on, signed out: press `/` to search it and `Enter` to play. Press `Enter` on it again (or `:provider off`) to go back to the demo tracks. Shellify remembers your choice (`[providers] active` in `config.toml`).
- **Spotify** and **Apple Music** aren't built yet; their setup screen says what they'll need.

Signed out, a few songs won't play, because YouTube only serves them to signed-in accounts. Shellify skips them and says why. Sign-in comes next.

Choosing a provider also switches Shellify to that provider's colors: red for YouTube Music, green for Spotify, pink for Apple Music. You can pick another theme in Settings at any time.

### Themes

Pick a preset (`default`, `nord`, `gruvbox`, `catppuccin`, or a provider theme: `youtube-music`, `spotify`, `apple-music`) and optionally override individual colors. A color can be a name (`cyan`, `light-blue`), `#rrggbb` or an ANSI index (`0`–`255`).

```toml
[theme]
preset = "catppuccin"
accent = "#f5c2e7"      # focused borders, selection, progress bar
# text = "reset"        # main text (reset = your terminal's default)
# muted = "dark-gray"   # unfocused borders, secondary info
# error = "red"
# selection_fg = "black"  # text drawn on the accent color
```

### Visualizer

Press `v` (or `:visualizer`) to show an animated visualizer in the Now Playing panel, and `V` to cycle its style:

- **bars**: vertical bars with falling peak caps
- **mirror**: bars growing up and down from a center line
- **wave**: an oscilloscope-style line
- **dots**: bouncing dots with peak markers

Turn on **fade** (`:visualizer fade`) and the bars fade in as they rise and fade out as they fall, leaving a short trail. The wave leaves fading echoes instead, and the dots leave tails. True-color themes fade smoothly. Other themes step through dimmer styles and lighter shade glyphs, which also works without color.

You can also set it in **Settings → Visualizer**, with `:visualizer [on|off|next|bars|mirror|wave|dots]`, or in the config:

```toml
[ui]
visualizer = true
visualizer_style = "wave"
visualizer_fade = true
```

It reacts to the music you're playing: mpv measures the loudness (with FFmpeg's `astats`, which leaves the audio untouched), so the bars rise and punch with the track. mpv doesn't expose a frequency spectrum, so how that energy spreads across the bars is a smooth animated pattern, not a true spectrum analyzer. Without mpv, it just drifts gently.

It needs a window at least 26 rows tall and hides itself in smaller ones. It animates at about 30fps only while music plays or it's still settling; when paused or off, it costs nothing. It works with `NO_COLOR` and the ascii icon pack.

### Custom themes

You can add your own themes alongside the built-in ones. Each theme is a small TOML file in `~/.config/shellify/themes/`, named after the theme:

```toml
# ~/.config/shellify/themes/tokyo-night.toml
name = "Tokyo Night"      # optional display name
base = "nord"             # optional: start from a built-in theme
accent = "#7aa2f7"        # any color you leave out comes from `base`
text = "#c0caf5"
muted = "#565f89"
error = "#f7768e"
selection_fg = "#1a1b26"
```

Use `:theme tokyo-night`, or pick it in **Settings → Theme**, where custom themes appear after the built-in ones.

**Importing** is the easiest way in:

```
:theme import ~/Downloads/tokyo-night.yaml
```

`:theme import` accepts a Shellify `.toml` theme or a **base16 scheme** (`.yaml`), the format most popular terminal themes are published in (see [tinted-theming/schemes](https://github.com/tinted-theming/schemes)). Shellify checks the file, converts it, saves it to your themes folder and switches to it. It never overwrites an existing theme. The base16 roles map as follows:

- `base0D` (blue) → accent
- `base05` → text
- `base03` → muted
- `base08` (red) → error
- `base00` → selection text

After editing or adding theme files by hand, run `:theme reload`. A file with a mistake in it is skipped with a message rather than stopping Shellify from starting. If your config names a theme that no longer exists, Shellify falls back to the default one.

### Color

Shellify honors [`NO_COLOR`](https://no-color.org) and `TERM=dumb`. Without color, it marks focus with a heavier border, selection with reverse video, and status with symbols.

```toml
[ui]
color = "auto"     # auto (default) | always | never
icons = "unicode"  # unicode (default) | ascii | nerd
mouse = false      # click, double-click and scroll; also in the Settings tab
resize_cursor = false  # resize pointer over pane borders (OSC 22 terminals)
```

`ascii` draws everything with plain ASCII, including the borders, for legacy terminals and serial consoles. `nerd` uses [Nerd Font](https://www.nerdfonts.com) glyphs and requires a Nerd Font in your terminal. It's opt-in because terminals can't report which font they use.

### Mouse

Mouse support is off by default. Turn it on under **Settings → Mouse** (or set `mouse = true` in `[ui]`). While it's on, your terminal's own click-and-drag text selection needs **Shift** held in most terminals. With it on, you can click to focus a pane or select a row, double-click to play or open, scroll to move the selection, click the header tabs, click the progress bar to seek, and **drag the border between two panes to resize them**. Draggable borders show a small `◂▸` grip and light up when you hover over them; double-click a border to reset it to its default width.

Optionally, **Settings → Resize cursor** (`[ui] resize_cursor = true`) also changes the mouse pointer to a left-right resize arrow over pane borders. It uses the OSC 22 escape sequence, which terminals such as kitty, foot and Ghostty support. Others ignore it, and inside tmux it usually has no effect. It's off by default, and Shellify only sends the sequence when it's turned on. Every mouse action also has a key.

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
