//! Runtime engine: a Spotify poller thread, a Discord status-sender thread and
//! a ~60 fps tick that advances playback progress and fires status updates.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::connector::{self, FetchError};
use crate::lyrics::LyricsFetcher;
use crate::state::AppContext;
use crate::sync::build_status;

/// Last.fm line-offset fallback (ms) when the detection lag couldn't be
/// measured (no scrobble history, a long pause, or the app started mid-song).
const DEFAULT_LASTFM_LAG_MS: u64 = 3_000;

/// Current wall clock as Unix seconds (0 on clock errors).
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Messages sent from the tick loop to the Discord sender thread.
enum StatusMsg {
    Update { text: String, emoji: String },
    Clear,
}

pub struct Engine {
    ctx: Arc<AppContext>,
    sender: mpsc::Sender<StatusMsg>,
    quit: Arc<AtomicBool>,
    /// Detaches the sender thread when dropped.
    _sender_thread: Option<thread::JoinHandle<()>>,
}

impl Engine {
    pub fn new(ctx: Arc<AppContext>) -> Arc<Engine> {
        let (tx, rx) = mpsc::channel::<StatusMsg>();

        // ── Discord status sender thread ────────────────────────────────────
        let sender_ctx = ctx.clone();
        let sender_thread = thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                let token = sender_ctx.settings.read().unwrap().token.clone();
                if token.is_empty() {
                    continue;
                }
                let sent_at = Instant::now();
                match msg {
                    StatusMsg::Update { text, emoji } => {
                        if connector::patch_status(&token, &text, &emoji).is_ok() {
                            let ms = sent_at.elapsed().as_millis() as u64;
                            let limit = sender_ctx
                                .settings
                                .read()
                                .unwrap()
                                .timing
                                .autooffset
                                .max(1);
                            sender_ctx
                                .shared
                                .tracker
                                .lock()
                                .unwrap()
                                .add_latency(ms, limit);
                        }
                    }
                    StatusMsg::Clear => {
                        let _ = connector::patch_status(&token, "", "");
                    }
                }
            }
        });

        Arc::new(Engine {
            ctx,
            sender: tx,
            quit: Arc::new(AtomicBool::new(false)),
            _sender_thread: Some(sender_thread),
        })
    }

    pub fn quit(&self) -> &AtomicBool {
        &self.quit
    }

    /// Advance progress and decide whether a new lyric line must be pushed.
    pub fn tick(&self, delta_ms: u64) {
        // Lock order is always `playback` → `tracker` to avoid deadlocks.
        {
            let mut playback = self.ctx.shared.playback.lock().unwrap();
            if playback.is_playing {
                playback.song_progress = playback.song_progress.saturating_add(delta_ms);
            }
            if playback.ended() {
                self.ctx.shared.tracker.lock().unwrap().sent_lines.clear();
            }
        }
        self.maybe_send_line();
    }

    /// Auto-clear on song switch, then send the newest line whose timestamp has
    /// passed the (auto)offset threshold.
    fn maybe_send_line(&self) {
        let settings = self.ctx.settings.read().unwrap();
        let mut playback = self.ctx.shared.playback.lock().unwrap();
        let mut tracker = self.ctx.shared.tracker.lock().unwrap();

        // Reset per-song tracking on every song change; only send the actual
        // clear request when auto_clear is enabled.
        if !playback.song_id.is_empty() && playback.song_id != tracker.last_seen_song {
            tracker.last_seen_song = playback.song_id.clone();
            tracker.sent_lines.clear();
            if settings.view.auto_clear {
                let _ = self.sender.send(StatusMsg::Clear);
            }
        }

        if !playback.is_playing || !playback.has_lyrics || playback.ended() {
            return;
        }

        let allowance = if settings.timing.enable_autooffset {
            tracker.avg_latency().saturating_add(100)
        } else {
            settings.timing.send_time_offset
        };
        // Last.fm has no playback position: the local clock only starts when
        // the nowplaying track is *first seen*, which is a few seconds after
        // it began playing. Compensate with the measured detection lag (or a
        // sane default when it couldn't be estimated). Spotify reports an
        // exact position, so it keeps the plain allowance.
        let offset = if settings.source == crate::config::Source::Lastfm {
            tracker
                .lastfm_lag
                .unwrap_or(DEFAULT_LASTFM_LAG_MS)
                .saturating_add(allowance)
        } else {
            allowance
        };
        let threshold = playback.song_progress.saturating_add(offset);

        // Find the target index while borrowing, then clone only that line —
        // avoids copying the whole vector on every 60 fps tick.
        let target = {
            let lyrics = match &playback.lyrics {
                Some(l) => l,
                None => return,
            };
            let mut found = None;
            for (i, line) in lyrics.iter().enumerate() {
                if line.time >= threshold {
                    continue;
                }
                if line.text.trim().is_empty() {
                    continue;
                }
                // Wait until this is the last line that has passed the threshold.
                if let Some(next) = lyrics.get(i + 1) {
                    if next.time < threshold {
                        continue;
                    }
                }
                if tracker.sent_lines.contains(&line.time) {
                    continue;
                }
                if playback.current_line.as_ref() == Some(line) {
                    continue;
                }
                found = Some(i);
                break;
            }
            found
        };

        if let Some(i) = target {
            let line = playback.lyrics.as_ref().unwrap()[i].clone();
            playback.current_line = Some(line.clone());
            tracker.sent_lines.push(line.time);

            let (text, emoji) = build_status(&settings, &playback, &line);
            let _ = self.sender.send(StatusMsg::Update { text, emoji });
        }
    }

    /// Run the poller loop in a background thread (every ~2 s). The poller owns
    /// a single plain `LyricsFetcher` and the cached Spotify token, so no lock
    /// is ever held across network I/O in here.
    pub fn spawn_poller(&self) {
        let ctx = self.ctx.clone();
        let quit = self.quit.clone();
        thread::spawn(move || {
            let mut fetcher = LyricsFetcher::new(&ctx.config_dir);
            let mut state = PollerState::default();
            let mut last = Instant::now();
            while !quit.load(Ordering::SeqCst) {
                let now = Instant::now();
                if now.duration_since(last) >= Duration::from_millis(2000) {
                    last = now;
                    poll_once(&ctx, &mut fetcher, &mut state);
                }
                thread::sleep(Duration::from_millis(200));
            }
        });
    }

    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::SeqCst);
    }
}

/// Per-thread state kept between poll cycles (cached tokens, last source).
#[derive(Default)]
struct PollerState {
    /// Cached Spotify token from the Discord connections endpoint.
    spotify_token: Option<String>,
    /// The source used on the previous cycle, to avoid spamming the log when
    /// a source's prerequisites are missing.
    last_logged_source: Option<crate::config::Source>,
}

/// One poll cycle: refresh the Spotify token when needed, fetch player state,
/// detect song changes and refresh lyrics when needed.
fn poll_once(ctx: &AppContext, fetcher: &mut LyricsFetcher, poller: &mut PollerState) {
    let settings = ctx.settings.read().unwrap();
    let source = settings.source;
    let token = settings.token.clone();
    drop(settings);

    match source {
        crate::config::Source::Spotify => {
            // The Discord token is only *required* for Spotify (it's what
            // unlocks the Spotify OAuth token). Last.fm works without it —
            // the status just won't be updated.
            if token.is_empty() {
                return;
            }
            poll_spotify(ctx, fetcher, poller, &token);
        }
        crate::config::Source::Lastfm => poll_lastfm(ctx, fetcher, poller),
    }
}

/// Spotify cycle: token refresh via Discord, then the public player endpoint.
fn poll_spotify(ctx: &AppContext, fetcher: &mut LyricsFetcher, poller: &mut PollerState, token: &str) {
    // Fetch the Spotify token lazily and cache it; only refresh on 401 so we
    // don't hammer Discord's connections endpoint every 2 s.
    if poller.spotify_token.is_none() {
        match connector::fetch_spotify_token(token) {
            Ok(t) => poller.spotify_token = Some(t),
            Err(e) => {
                crate::log::write(&format!("spotify token refresh failed: {e:?}"));
                return;
            }
        }
    }
    let spotify = poller.spotify_token.as_ref().unwrap();

    let request_start = Instant::now();
    let state = match connector::fetch_player(spotify) {
        Ok(Some(s)) => s,
        Ok(None) => {
            ctx.shared.playback.lock().unwrap().is_playing = false;
            return;
        }
        Err(FetchError::Unauthorized) => {
            // Token expired — drop it and refetch next cycle.
            poller.spotify_token = None;
            crate::log::write("spotify player returned 401, refreshing token");
            return;
        }
        Err(FetchError::Other(e)) => {
            crate::log::write(&format!("spotify player error: {e}"));
            return;
        }
    };
    // Compensate for the network round-trip so progress stays accurate.
    let rtt = request_start.elapsed().as_millis() as u64;

    let song_changed = apply_state(ctx, &state, Some(state.progress_ms.saturating_add(rtt)));
    sync_lyrics(ctx, fetcher, song_changed);
}

/// Last.fm cycle: poll the user's scrobbles. No playback position is exposed,
/// so progress is left to the local tick clock (see [`apply_state`]).
fn poll_lastfm(ctx: &AppContext, fetcher: &mut LyricsFetcher, poller: &mut PollerState) {
    let settings = ctx.settings.read().unwrap();
    let api_key = settings.lastfm.api_key.clone();
    let username = settings.lastfm.username.clone();
    drop(settings);

    if api_key.is_empty() || username.is_empty() {
        if poller.last_logged_source != Some(crate::config::Source::Lastfm) {
            crate::log::write("last.fm source selected but no api key/username set");
            poller.last_logged_source = Some(crate::config::Source::Lastfm);
        }
        ctx.shared.playback.lock().unwrap().is_playing = false;
        return;
    }
    poller.last_logged_source = None;

    let (state, prev_uts) = match crate::lastfm::fetch_player(&api_key, &username) {
        Ok(Some((s, p))) => (s, p),
        Ok(None) => {
            ctx.shared.tracker.lock().unwrap().lastfm_lag = None;
            ctx.shared.playback.lock().unwrap().is_playing = false;
            return;
        }
        Err(e) => {
            crate::log::write(&format!("last.fm player error: {e:?}"));
            return;
        }
    };

    // The previous completed scrobble's end time is only meaningful *at the
    // moment the song changes*: it's an upper bound for when the current song
    // started, i.e. how far into it the poller was when it first noticed it.
    // Measured later in the song it drifts (it's just the song's age), so take
    // it exactly once per track. Untrusted estimates (pause, gap, late start)
    // fall back to a default when the offset is read.
    let song_changed = {
        let pb = ctx.shared.playback.lock().unwrap();
        pb.song_id != state.track_id
    };
    if song_changed {
        let lag = prev_uts.and_then(|p| crate::lastfm::measure_lag(now_unix_secs(), p));
        ctx.shared.tracker.lock().unwrap().lastfm_lag = lag;
    }

    let song_changed = apply_state(ctx, &state, None);
    sync_lyrics(ctx, fetcher, song_changed);
}

/// Push a fetched player state into shared playback. When `progress_ms` is
/// `Some`, it wins over the local clock (Spotify/MPRIS report real position);
/// when `None` (Last.fm), progress keeps being advanced by the tick loop and
/// is reset to zero on song change. Returns `true` when the song changed.
fn apply_state(ctx: &AppContext, state: &connector::PlayerState, progress_ms: Option<u64>) -> bool {
    let mut playback = ctx.shared.playback.lock().unwrap();
    let song_changed = playback.song_id != state.track_id;

    playback.is_playing = state.is_playing;
    playback.song_duration = state.duration_ms;
    if let Some(p) = progress_ms {
        playback.song_progress = p;
    }

    if song_changed {
        playback.old_song_id = playback.song_id.clone();
        playback.song_id = state.track_id.clone();
        playback.song_name = connector::cleanup_title(&state.name);
        playback.song_author = state.artist.clone();
        playback.lyrics = None;
        playback.current_line = None;
        playback.has_lyrics = false;
        if progress_ms.is_none() {
            // Last.fm: no position data, so (re)start the local clock.
            playback.song_progress = 0;
        }
    }
    song_changed
}

/// Fetch lyrics when the song changed (or none are loaded yet).
fn sync_lyrics(ctx: &AppContext, fetcher: &mut LyricsFetcher, song_changed: bool) {
    // Lyrics already present and the song hasn't changed — nothing to do.
    if !song_changed && ctx.shared.playback.lock().unwrap().has_lyrics {
        return;
    }

    let (name, artist) = {
        let pb = ctx.shared.playback.lock().unwrap();
        (pb.song_name.clone(), pb.song_author.clone())
    };
    if name.is_empty() {
        return;
    }

    // Load from the disk cache on song change — this covers replays of a
    // previously-cached song (the fetch guard below would otherwise skip it).
    if song_changed {
        if let Some(cached) = fetcher.read_cache(&name, &artist) {
            let mut pb = ctx.shared.playback.lock().unwrap();
            pb.lyrics = Some(cached);
            pb.has_lyrics = true;
            pb.current_line = None;
            *ctx.shared.lyric_source.lock().unwrap() = "cache".to_string();
            return;
        }
    }

    // Never re-query the same song while it keeps playing (a failed attempt is
    // recorded too, so lyric-less tracks are only tried once per song).
    let key = format!("{name}{artist}");
    if fetcher.last_fetched_for() == key {
        return;
    }

    let result = fetcher.fetch(&name, &artist);

    let mut pb = ctx.shared.playback.lock().unwrap();
    match result {
        Some((lines, source)) => {
            pb.lyrics = Some(lines);
            pb.has_lyrics = true;
            pb.current_line = None;
            *ctx.shared.lyric_source.lock().unwrap() = source.clone();
            crate::log::write(&format!("lyrics for \"{}\" from {source}", pb.song_name));
        }
        None => {
            pb.has_lyrics = false;
            *ctx.shared.lyric_source.lock().unwrap() = "none".to_string();
            crate::log::write(&format!("no lyrics found for \"{}\"", pb.song_name));
        }
    }
}

/// Lightweight, allocation-light snapshot for the UI / web panel. Deliberately
/// omits the full lyrics vector so rendering never clones it.
#[derive(Debug, Clone, Default)]
pub struct UiSnapshot {
    pub song_name: String,
    pub song_author: String,
    pub song_progress: u64,
    pub song_duration: u64,
    pub is_playing: bool,
    pub has_lyrics: bool,
    pub current_line: Option<String>,
}

pub fn snapshot(ctx: &AppContext) -> UiSnapshot {
    let pb = ctx.shared.playback.lock().unwrap();
    UiSnapshot {
        song_name: pb.song_name.clone(),
        song_author: pb.song_author.clone(),
        song_progress: pb.song_progress,
        song_duration: pb.song_duration,
        is_playing: pb.is_playing,
        has_lyrics: pb.has_lyrics,
        current_line: pb.current_line.as_ref().map(|l| l.text.clone()),
    }
}

/// Last lyric source name from shared state.
pub fn last_source(ctx: &AppContext) -> String {
    ctx.shared.lyric_source.lock().unwrap().clone()
}

/// Last measured Discord API latency in ms.
pub fn last_latency(ctx: &AppContext) -> u64 {
    ctx.shared.tracker.lock().unwrap().last_latency
}
