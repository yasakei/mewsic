# Mewsic

Keep your Discord status in sync with the song you're playing — line by line.

Mewsic is a small, self-contained daemon that watches your Spotify playback
through your Discord connection and mirrors each lyric line into your Discord
status in real time. Written in safe Rust: zero `unsafe`, a tiny memory
footprint, and no runtime dependencies.

## Why Rust?

Mewsic runs as a single native binary:

- **Always safe** — `#![forbid(unsafe_code)]` at the crate root; no `unsafe`
  anywhere in the codebase.
- **Memory efficient** — no async runtime, minimal heap. A single polling
  thread, a sender thread, and a few guarded shared buffers. The release
  binary is a few MB and idles at single-digit-MB RSS.
- **Small dependency set** — `serde`, `serde_json`, `toml`, `ureq`, `crossterm`,
  `ratatui`, `base64`. Nothing else.

## Features

- Pulls your current track from the Discord → Spotify connection (no local
  player required, works on any OS with Discord).
- Fetches synced lyrics from LrcLib, NetEase Music, and QQ Music, in that
  order, with an on-disk cache so repeat plays are instant.
- Sends each line to Discord ahead of time using a fixed offset or an
  auto-offset that learns from measured Discord API latency.
- Clears the status the moment the song changes (optional).
- Fully configurable status: `[m:ss]` timestamp, emoji, labels, or a custom
  template.
-  Terminal dashboard with live progress bar, plus a web panel served on
  `http://127.0.0.1:8999`.
- `setup` wizard, interactive settings editor, autostart on login, and a
  `stop` command via PID file.

## Requirements

- A Spotify account connected to Discord (Settings → Connections → Spotify).
- A Discord user token. You can get one by enabling developer mode in Discord
  and copying your token (Account → Advanced).

## Building

```sh
cargo build --release
# binary at target/release/mewsic
```

## Usage

```sh
mewsic           # run the dashboard + engine
mewsic web       # run the engine with the web panel enabled
mewsic setup     # interactive first-time setup
mewsic settings  # edit settings interactively
mewsic stop      # stop the running instance
mewsic version   # print version
```

On first run without a token, Mewsic offers a choice: run the terminal setup
wizard, or open the web panel and finish setup in the browser. The engine picks
up the token automatically once it's saved.

## Configuration

Settings live in `settings.toml` under the config directory
(`$XDG_CONFIG_HOME/mewsic`, `$HOME/.config/mewsic`, or `%APPDATA%\mewsic` on
Windows — override with `$MEWSIC_CONFIG_DIR`). The setup wizard and settings
editor write this file for you, so you usually never touch it by hand.

## Architecture

| Module            | Responsibility                                        |
|-------------------|-------------------------------------------------------|
| `main.rs`         | CLI entry, PID file, instance guard, run loops        |
| `engine.rs`       | Poll loop, lyric sync, status sender thread           |
| `connector.rs`    | Discord token refresh, Spotify playback state         |
| `lyrics.rs`       | Lyric sources (LrcLib/NetEase/QQ), LRC parser, cache  |
| `sync.rs`         | Status text rendering (offset, template, crop)        |
| `state.rs`        | Shared playback/tracker state, UI snapshot            |
| `config.rs`       | TOML settings                                         |
| `tui.rs`          | Terminal dashboard, setup wizard, settings editor     |
| `web.rs`          | Tiny std-only HTTP server for the web panel           |
| `autostart.rs`    | Launch-on-login registration                          |
| `util.rs` / `log.rs` / `net.rs` | Formatting helpers, file logging, HTTP agent |

## License

MIT — see [LICENSE](LICENSE).
