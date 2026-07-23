# LyricsStatus V4 (Fork)

## What is it?

LyricsStatus is a tool that changes your Discord custom status to the synced lyrics of the song you're currently listening to on Spotify.

It is written in TypeScript and runs on Node.js. No Spotify developer account, no OAuth setup, no cookies — just your Discord token.

## How it works

LyricsStatus fetches your current Spotify playback state using the Spotify access token that Discord already holds internally (from your connected Spotify account). It then fetches synced lyrics from free sources and updates your Discord custom status line by line in real time.

## Precautions

This tool is provided "AS IS" without any warranty that it will work on your machine.

The creator of LyricsStatus is not responsible for any consequences that may arise from its use.

By using it, you agree with the statements above.

## Requirements

- [Node.js](https://nodejs.org/en) v17 or higher
- A Discord account with **Spotify connected** (Settings → Connections)
- Spotify playing on any device

## Setup

### 1. Download

Clone the repo or download the source archive from [Releases](https://github.com/OvalQuilter/lyrics-status/releases):

```
git clone https://github.com/OvalQuilter/lyrics-status
```

### 2. Install dependencies

You can use either npm or pnpm. pnpm is recommended if you want stricter dependency isolation.

```
npm install
```

or

```
pnpm install
```

### 3. Build

```
npm run build
```

or

```
pnpm run build
```

### 4. Run

```
npm start
```

or

```
pnpm start
```

If this is your first launch, `pnpm start` will ask whether you want the web panel or the terminal setup.
The terminal path also includes a dedicated update-settings screen.

While the live terminal dashboard is running you can use two handy shortcuts:

 - Press <kbd>Ctrl</kbd>+<kbd>S</kbd> to open the terminal settings editor (edit Discord token, view, timings, update options, etc.). Changes are saved to `settings.json` and are applied immediately to the running process.
 - Press <kbd>Ctrl</kbd>+<kbd>B</kbd> to start the web panel (same as `pnpm run web`).

There is also a direct script to open the terminal settings editor without starting the dashboard:

```
pnpm run settings
```

### 5. Configure

The terminal setup flow covers the full configuration flow, including update settings.

If you still want the legacy panel, start it explicitly with:

```
pnpm run web
```

Then open `http://localhost:8999` in your browser.

**Discord token** — the only credential you need. Here's [a video](https://www.youtube.com/watch?v=LnBnm_tZlyU) showing how to get it. In the terminal wizard, paste it when prompted and it will be verified automatically.

If you want to rerun setup later, use:

```
pnpm run setup
```

That's it. Play a song on Spotify and your Discord status will start updating with synced lyrics within a few seconds.

## Settings

| Setting | Description |
|---|---|
| Discord token | Your Discord user token. Used to read your Spotify connection and update your status. |
| Show playback timestamp | Prepends `[m:ss]` to the status text. |
| Show label | Prepends `Song lyrics -` to the status text. |
| Custom status template | Advanced mode — build your own status string using placeholders (see below). |
| Send time offset | How many ms ahead of the lyric timestamp to send the status update. Default 500. |
| Autooffset | Automatically calculates the offset based on Discord API response times. |

### Custom status placeholders

`{lyrics}`, `{lyrics_upper}`, `{lyrics_lower}`, `{lyrics_letters_only}`  
`{song_name}`, `{song_name_cropped}`, `{song_name_upper}`, `{song_name_lower}`  
`{song_author}`, `{song_author_upper}`, `{song_author_lower}`  
`{timestamp}`

Status text is automatically cropped to 128 characters (Discord's limit).

## Lyrics sources

Lyrics are fetched in this order, falling back to the next if one fails:

1. **LrcLib** — best coverage for synced LRC lyrics
2. **NetEase Music** — large catalogue, good for non-English tracks
3. **QQ Music** — last resort fallback

Fetched lyrics are cached locally in `./cache/` to avoid redundant requests.

## Troubleshooting

**Status not updating** — make sure Spotify is connected to your Discord account under Settings → Connections, and that you have a song actively playing (not paused).

**Token invalid** — re-run `pnpm run setup` and verify the Discord token when prompted.

**No lyrics found** — the song may not be in any of the lyrics databases, or only has unsynced lyrics. LyricsStatus requires time-synced lyrics to work.

**Want the old panel back** — run `pnpm run web`.

**Windows** — try running the command prompt with administrator privileges or temporarily disabling your firewall.

**Linux** — try running the terminal as root if you hit permission issues.

## Start, Stop and Autostart

The repository includes helper scripts and an autostart option to make running the app easier.

- **Start:** Run `pnpm start` to launch the background process. The start script writes a PID file at `~/.config/lyrics-status/lyrics-status.pid` and will refuse to spawn a duplicate if it detects a live instance.
- **Stop:** Run `pnpm stop` to stop the running instance. The stop script reads the PID file, sends `SIGTERM` to the process, and removes the PID file.
- **Autostart on login:** There's an `Auto-start on login` toggle in the terminal settings editor (also available in the web panel). When enabled the app will attempt to install a platform-specific autostart entry:
	- Linux: a `.desktop` entry in your user autostart directory.
	- macOS: a LaunchAgent plist in `~/Library/LaunchAgents`.
	- Windows: a `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` entry.
	Changes take effect immediately; disabling the toggle will attempt to remove the installed autostart entry.

Notes:
- `pnpm start` does not attach an interactive TTY to an already-running background process; it only avoids spawning duplicates.
- PID file location: `~/.config/lyrics-status/lyrics-status.pid`.
- Open the web panel manually with `pnpm run web`.
- Open the terminal settings editor directly with `pnpm run settings`.
