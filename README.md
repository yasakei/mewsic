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

- **Two playback sources** — Spotify via your Discord connection (no local
  player needed), or Last.fm scrobbles (covers YouTube Music via
  [WebScrobbler](https://webscrobbler.com/), and any scrobbling player).
- **Synced lyrics** from LrcLib, NetEase Music and QQ Music — pick any
  combination, or plug in a custom provider (URL template + optional JSON
  path). Cached on disk, so repeat plays are instant.
- **Romanization** — Japanese, Korean, Hindi, Bangla, Punjabi, Cyrillic,
  Greek and Arabic lyrics transliterated locally into Latin letters; Chinese
  passes through. Fully data-driven and user-overridable (see
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
