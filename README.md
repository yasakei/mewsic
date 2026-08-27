# Mewsic
<img width="800" height="380" alt="image" src="https://cdn.yasakei.dev/image/upload/v1779784805/6878ec6702c50_cgr597.png" />

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

- Two playback sources, chosen in setup / settings:
  - **Spotify** — pulled from the Discord → Spotify connection (no local player
    required, works on any OS with Discord).
  - **Last.fm** — follows your scrobbles, which also covers **YouTube Music**
    (via the WebScrobbler extension or the YT Music desktop app's built-in
    Last.fm scrobbling) and any other scrobbled player.
- Fetches synced lyrics from LrcLib, NetEase Music, and QQ Music, in that
  order, with an on-disk cache so repeat plays are instant.
- Sends each line to Discord ahead of time using a fixed offset or an
  auto-offset that learns from measured Discord API latency.
- Clears the status the moment the song changes (optional).
- Fully configurable status: `[m:ss]` timestamp, emoji, labels, or a custom
  template.
-  Terminal dashboard with live progress bar, plus a web panel served on
  `http://127.0.0.1:8999`.
- Detached background mode: `mewsic background` keeps playing after the
  terminal closes; running `mewsic` again attaches the TUI to that daemon.
- Manual updates: checks the Mewsic update API on interactive launch or with
  `mewsic update`,
  verifies the download against the release checksums, and swaps the new
  binary in place (restart mewsic to use it). Windows installs in elevated
  directories fall back to a silent NSIS installer; `mewsic update` forces a
  check, `mewsic update check` only reports.
- `setup` wizard, interactive settings editor, autostart on login (launches a
  background daemon), and `stop` / `kill` commands via PID files. Autostart
  can be turned off with `mewsic kill autostart` or in the settings editor.

## Requirements

- A Discord user token. You can get one by enabling developer mode in Discord
  and copying your token (Account → Advanced).
- **Spotify source:** a Spotify account connected to Discord (Settings →
  Connections → Spotify).
- **Last.fm source:** a free API key (https://www.last.fm/api/account/create)
  and your Last.fm username. You also need something actively scrobbling to
  Last.fm — for **YouTube Music** you must install the
  [WebScrobbler browser extension](https://webscrobbler.com/) (Firefox /
  Chrome / Edge), or use the YT Music desktop app's built-in Last.fm
  scrobbling. Any other scrobbling player works too (Spotify, iTunes, …).

Note: the Last.fm API reports the current track but no playback position, so
progress is estimated with a local clock. Lyric sync stays accurate while the
song plays straight through; pauses and seeks are not detected.

## Building

```sh
cargo build --release
# binary at target/release/mewsic
```

## Usage

```sh
mewsic                 # run the dashboard + engine
mewsic web             # run the engine with the web panel enabled
mewsic background      # run detached — keeps playing after the terminal closes
mewsic setup           # interactive first-time setup
mewsic settings        # edit settings interactively
mewsic stop            # stop the running foreground instance
mewsic kill background # stop the background instance
mewsic kill autostart  # disable autostart (start-on-login)
mewsic update          # check for and install the latest release
mewsic update check    # check for a newer release without installing
mewsic uninstall       # disable autostart and remove the installed binary
mewsic version         # print version
```

Autostart is enabled from the setup wizard, the settings editor (Ctrl+S from
the dashboard) or the web panel; it launches a background daemon that survives
the terminal session. Run `mewsic kill autostart` (or toggle it off in
settings) to stop mewsic from starting on login.

On first run with nothing configured (no Discord token and no Last.fm
credentials), Mewsic offers a choice: run the terminal setup wizard, or open
the web panel and finish setup in the browser. The engine picks up the
configuration automatically once it's saved.

## Configuration

Settings live in `settings.toml` under the config directory
(`$XDG_CONFIG_HOME/mewsic`, `$HOME/.config/mewsic`, or `%APPDATA%\mewsic` on
Windows — override with `$MEWSIC_CONFIG_DIR`). The setup wizard and settings
editor write this file for you, so you usually never touch it by hand.

Your Discord token is **not** stored in that file. It's kept in the OS
credential manager (macOS Keychain, Windows Credential Manager, or Linux
Secret Service/KWallet) via the `keyring` crate, and `settings.toml` is
chmodded `0600` so the Last.fm API key stays private too. On systems without a
keyring backend (e.g. a headless Linux box), the token falls back to a
`0600`-permission `token` file in the same directory instead of plaintext in
`settings.toml`. A token left in `settings.toml` by an older version is moved
into the credential store automatically on startup.

## Architecture

| Module            | Responsibility                                        |
|-------------------|-------------------------------------------------------|
| `main.rs`         | CLI entry, PID file, instance guard, run loops        |
| `engine.rs`       | Poll loop, lyric sync, status sender thread           |
| `connector.rs`    | Discord token refresh, Spotify playback state         |
| `lastfm.rs`       | Last.fm playback source (scrobble polling, duration)  |
| `lyrics.rs`       | Lyric sources (LrcLib/NetEase/QQ), LRC parser, cache  |
| `sync.rs`         | Status text rendering (offset, template, crop)        |
| `state.rs`        | Shared playback/tracker state, UI snapshot            |
| `config.rs`       | TOML settings                                         |
| `credential.rs`   | OS keyring storage for the Discord token              |
| `update.rs`       | Auto-updater (release check, checksum verify, install)|
| `tui.rs`          | Terminal dashboard, setup wizard, settings editor     |
| `web.rs`          | Tiny std-only HTTP server for the web panel           |
| `autostart.rs`    | Launch-on-login registration                          |
| `util.rs` / `log.rs` / `net.rs` | Formatting helpers, file logging, HTTP agent |

## License

MIT — see [LICENSE](LICENSE).
