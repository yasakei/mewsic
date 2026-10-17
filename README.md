# Mewsic

<img width="700" height="390" alt="mewsic" src="https://cdn.yasakei.dev/image/upload/v1779784805/6878ec6702c50_cgr597.png" />

**Keep your Discord status singing along — lyric line by lyric line.**

Mewsic watches your Spotify playback through your Discord connection (or your
Last.fm scrobbles) and mirrors each lyric line into your Discord status in
real time. One tiny native binary, written in safe Rust.

![Release](https://img.shields.io/github/v/release/yasakei/mewsic?style=flat-square&label=release&color=f5a97f)
![CI](https://img.shields.io/github/actions/workflow/status/yasakei/mewsic/check.yml?branch=main&style=flat-square&label=CI&color=a6da95)
![Rust](https://img.shields.io/badge/rust-safe%20only-a6da95?style=flat-square)
![Platforms](https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-8aadf4?style=flat-square)
![PRs](https://img.shields.io/badge/PRs-welcome-f5bde6?style=flat-square)

---

## Features

- **Three playback sources** — Spotify via your Discord connection (no local
  player needed), Last.fm scrobbles (covers YouTube Music via
  [WebScrobbler](https://webscrobbler.com/), and any scrobbling player),
  or the local player on your machine (MPRIS on Linux, Spotify/Music on
  macOS, media controls on Windows — exact position, no scrobble lag).
- **Synced lyrics** from LrcLib, NetEase Music, QQ Music, Last.fm transcripts
  and YouTube captions (no key needed) — pick any combination, or plug in a
  custom provider (URL template + optional JSON path). Cached on disk, so
  repeat plays are instant.
- **Romanization** — Japanese, Korean, Hindi, Bangla, Punjabi, Cyrillic,
  Greek, Arabic, Armenian, Georgian, Thai, Lao and Khmer lyrics transliterated
  locally into Latin letters; Chinese passes through. Fully data-driven and user-overridable (see
  [Romanization](#romanization)).
- **Ahead-of-time line sync** — fixed offset, or an auto-offset that learns
  your Discord API latency.
- **Your style** — timestamp, emoji, labels, or a fully custom template.
  Status clears on song change (optional).
- **Terminal dashboard** with live progress bar, plus a web panel at
  `http://127.0.0.1:8999`.
- **Detached background mode** — `mewsic background` outlives your terminal;
  run `mewsic` again to re-attach.
- **Self-updating** — checks the release API, verifies checksums, swaps the
  binary in place.

## Getting started

```sh
mewsic setup    # one-time wizard (Discord token, source, providers)
mewsic          # dashboard + engine
```

**Requirements:** a Discord user token, plus either Spotify connected to
Discord (*Settings → Connections → Spotify*) or a
[Last.fm API key](https://www.last.fm/api/account/create) with something
actively scrobbling.

> [!NOTE]
> Last.fm doesn't report playback position, so progress is estimated with a
> local clock — sync stays accurate while a song plays straight through.

## Commands

| Command                  | What it does                              |
|--------------------------|-------------------------------------------|
| `mewsic`                 | dashboard + engine                        |
| `mewsic web`             | engine with the web panel enabled         |
| `mewsic background`      | run detached, survive terminal close      |
| `mewsic setup`           | interactive first-time setup              |
| `mewsic settings`        | edit settings in the terminal             |
| `mewsic stop`            | stop the foreground instance              |
| `mewsic kill background` | stop the background instance              |
| `mewsic kill autostart`  | disable start-on-login                    |
| `mewsic update`          | check for and install the latest release  |
| `mewsic update check`    | report only, don't install                |
| `mewsic uninstall`       | remove autostart and the installed binary |

## Configuration

Settings live in `settings.toml` under your config directory
(`~/.config/mewsic`, or `%APPDATA%\mewsic` on Windows; override with
`$MEWSIC_CONFIG_DIR`). The wizard and settings editor write it for you.

Your Discord token never touches that file — it's stored in the OS credential
manager (Keychain / Credential Manager / Secret Service), with a `0600`
fallback file on systems without a keyring.

### Lyrics relay (experimental)

The panel's **Experiments** card can expose your live song + lyric line as a
push-only WebSocket at `/song/ws` so other pages can follow along. Only that
one route is ever reachable from outside — the token, settings and saves stay
behind the local-only panel check.

**WS payload** (one JSON message per state/line change, ~1/s while playing):

```json
{
  "song": "Love Me Not", "artist": "Ravyn Lenae",
  "playing": true, "progress": 175000, "duration": 213000,
  "line": "All this time I'm thinking, I'm strong enough to sink it",
  "hasLyrics": true, "source": "Spotify", "ts": 1760689200000
}
```

Quick consumer from any page:

```js
const ws = new WebSocket("wss://your.host/song/ws");
ws.onmessage = e => { const s = JSON.parse(e.data); renderNowPlaying(s); };
```

#### Options (panel: Experiments card, or `[relay]` in settings.toml)

| Option | Default | Effect |
| --- | --- | --- |
| `enabled` | `false` | Serve the WS feed at `/song/ws` (through a tunnel) |
| `max_clients` | `8` | Concurrent listener cap; extra connections get `429` |
| `token` | *(empty)* | When set, the handshake requires `?token=` (`401` otherwise) |
| `tunnel` | `false` | Start cloudflared automatically (a quick tunnel if `tunnel_name` is empty) |
| `tunnel_name` | *(empty)* | Name of your **own** cloudflared tunnel → custom domain mode |
| `tunnel_host` | *(empty)* | Public hostname routed to the tunnel; builds the display link |

Also enforced regardless of config: inbound messages are capped at 8 per
10s per client (over the cap = disconnected) and frames larger than 4 KiB
are rejected (close code 1009).

TOML example:

```toml
[relay]
enabled = true
max_clients = 8
token = "pick-a-long-secret"
tunnel = true

# custom domain (optional — see one-time setup below)
tunnel_name = "mewsic-relay"
tunnel_host = "lyrics.yasakei.dev"
```

#### Auto-start with mewsic

The relay (and its tunnel) come up whenever mewsic runs — including the
desktop autostart path (`Auto-start on login` in the panel / `System` card).
If mewsic would normally run without the web panel (plain TUI, headless),
an enabled relay starts the panel listener anyway, since that's what serves
`/song/ws`. Disable the relay in the Experiments card (or `enabled = false`
in settings.toml) to turn everything off; the tunnel process is stopped
when mewsic exits.

#### Quick tunnel (no setup)

Leave `tunnel_name` empty, turn the tunnel toggle on → `cloudflared tunnel
--url http://127.0.0.1:8999` runs and Mewsic parses the temporary
`*.trycloudflare.com` link from its output and shows it in the panel. The
link rotates every restart; reconnect consumers to whatever is current.

#### Custom domain (your own named tunnel)

One-time, in a terminal:

```sh
cloudflared tunnel login                          # browser auth
cloudflared tunnel create mewsic-relay            # tunnel + credentials json
cloudflared tunnel route dns mewsic-relay lyrics.yasakei.dev
```

Then set `tunnel_name = "mewsic-relay"` and `tunnel_host =
"lyrics.yasakei.dev"` in the panel (or `[relay]` in settings.toml). Mewsic
runs `cloudflared tunnel run mewsic-relay`, restarts it if it dies, stops it
when mewsic quits, and displays `wss://lyrics.yasakei.dev/song/ws` (+ your
`?token=`) as the relay link. Works with the quick-tunnel toggle off.

The relay link and listener count update live in the Experiments card at
`GET /api/relay` — local only, like the rest of the panel.

### Telemetry

By default mewsic sends one short, anonymous request per run so we can publish
"how many people use mewsic on which OS" graphs. The payload is only:
a random install id, your OS, your CPU architecture and the app version —
**never IPs, usernames, tokens, or song data**. Aggregate stats live at
[`mewsic.yasakei.dev/usage`](https://mewsic.yasakei.dev/usage).

Counts are keyed by the install id, not by how often you launch mewsic — run
it a hundred times a day and you still count as one user. Your id is a random
string generated once per installation (persisted in `~/.config/mewsic/install_id`)
and the server keeps it for 90 days.

Opt out any time with `mewsic settings` → section 8, the web panel's *system*
section, `usage.enabled = false` in `settings.toml`, or the environment
variable `MEWSIC_NO_TELEMETRY=1`.

## Romanization

Every letter-to-Latin table lives in [`romanize/`](romanize/) — **one TOML
file per script**. A build script embeds the folder automatically, so adding
a script is just dropping in a file. Copy
[`romanize/template.toml`](romanize/template.toml) to start.

You can override or extend scripts without rebuilding: put a TOML with the
same name in `~/.config/mewsic/romanize/` and your entries win (new names add
new scripts). Test any change instantly with:

```sh
mewsic romanize "日本語の歌詞"    # nihongo no kashi
mewsic romanize < lyrics.txt
```

Two table kinds:

- **`chart`** — one-letter maps (Cyrillic, Greek, Arabic…)
- **`abugida`** — Indic-style scripts (Devanagari, Bengali, Gurmukhi) with an
  inherent vowel, matras, halant conjuncts and per-dialect phonetic rules

Kana and Hangul are handled algorithmically; Chinese hanzi pass through
(pinyin needs a dictionary).

## Building

```sh
cargo build --release
# binary at target/release/mewsic
```

## License

MIT — see [LICENSE](LICENSE).
