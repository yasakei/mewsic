use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::connector::{self, FetchError};
use crate::lyrics::LyricsFetcher;
use crate::state::AppContext;
use crate::sync::build_status;

const DEFAULT_LASTFM_LAG_MS: u64 = 3_000;
/// Refresh the status shortly before its `expires_at` lapses so a long
/// instrumental gap between lyric lines doesn't blank the status out.
const STATUS_KEEPALIVE_INTERVAL_MS: u64 =
    crate::connector::STATUS_EXPIRY_SECS.saturating_sub(15) as u64 * 1000;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

enum StatusMsg {
    Update { text: String, emoji: String },
    KeepAlive { text: String, emoji: String },
    Clear,
    Restore,
    Shutdown(mpsc::Sender<()>),
}

pub struct Engine {
    ctx: Arc<AppContext>,
    sender: mpsc::Sender<StatusMsg>,
    quit: Arc<AtomicBool>,
    _sender_thread: Option<thread::JoinHandle<()>>,
}

impl Engine {
    pub fn new(ctx: Arc<AppContext>) -> Arc<Engine> {
        let (tx, rx) = mpsc::channel::<StatusMsg>();

        let sender_ctx = ctx.clone();
        let sender_thread = thread::spawn(move || {
            let mut original_status = None;
            while let Ok(msg) = rx.recv() {
                let token = sender_ctx.settings.read().unwrap().token.clone();
                if token.is_empty() {
                    if let StatusMsg::Shutdown(done) = msg {
                        let _ = done.send(());
                        break;
                    }
                    continue;
                }

                if matches!(
                    msg,
                    StatusMsg::Update { .. } | StatusMsg::KeepAlive { .. } | StatusMsg::Clear
                ) && original_status.is_none()
                {
                    match connector::fetch_status(&token) {
                        Ok(status) => original_status = Some(status),
                        Err(error) => {
                            crate::log::write(&format!(
                                "could not capture Discord status: {error:?}"
                            ));
                            continue;
                        }
                    }
                }

                let sent_at = Instant::now();
                match msg {
                    StatusMsg::Update { text, emoji } => {
                        if connector::patch_status(&token, &text, &emoji).is_ok() {
                            let ms = sent_at.elapsed().as_millis() as u64;
                            let limit =
                                sender_ctx.settings.read().unwrap().timing.autooffset.max(1);
                            sender_ctx
                                .shared
                                .tracker
                                .lock()
                                .unwrap()
                                .add_latency(ms, limit);
                        }
                    }
                    StatusMsg::KeepAlive { text, emoji } => {
                        let _ = connector::patch_status(&token, &text, &emoji);
                    }
                    StatusMsg::Clear => {
                        let _ = connector::patch_status(&token, "", "");
                    }
                    StatusMsg::Restore => {
                        if let Some(status) = original_status.as_ref() {
                            if let Err(error) = connector::restore_status(&token, status) {
                                crate::log::write(&format!(
                                    "could not restore Discord status: {error:?}"
                                ));
                            }
                        }
                    }
                    StatusMsg::Shutdown(done) => {
                        if let Some(status) = original_status.as_ref() {
                            if let Err(error) = connector::restore_status(&token, status) {
                                crate::log::write(&format!(
                                    "could not restore Discord status: {error:?}"
                                ));
                            }
                        }
                        let _ = done.send(());
                        break;
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

    pub fn quit(&self) -> Arc<AtomicBool> {
        self.quit.clone()
    }

    pub fn tick(&self, delta_ms: u64) {
        {
            let mut playback = self.ctx.shared.playback.lock().unwrap();
            if playback.is_playing {
                playback.song_progress = playback.song_progress.saturating_add(delta_ms);
            }
            if playback.ended() {
                self.ctx.shared.tracker.lock().unwrap().sent_lines.clear();
            }
        }

        // Revert to the pre-lyrics status when the song is paused.
        if playing_edge_to_pause(&self.ctx) {
            let _ = self.sender.send(StatusMsg::Restore);
        }

        self.maybe_send_line();
        self.maybe_keep_alive();
    }

    /// Re-sends the current line when the status's `expires_at` is close to
    /// lapsing but no new lyric has arrived, so the status doesn't blank out
    /// during instrumental gaps longer than the expiry window.
    fn maybe_keep_alive(&self) {
        let last_send = self.ctx.shared.tracker.lock().unwrap().last_send;
        let Some(last) = last_send else { return };
        if (last.elapsed().as_millis() as u64) < STATUS_KEEPALIVE_INTERVAL_MS {
            return;
        }

        let (text, emoji) = {
            let playback = self.ctx.shared.playback.lock().unwrap();
            if !playback.is_playing || !playback.has_lyrics || playback.ended() {
                return;
            }
            let Some(line) = playback.current_line.as_ref() else {
                return;
            };
            let settings = self.ctx.settings.read().unwrap();
            build_status(&settings, &playback, line)
        };
        self.ctx.shared.tracker.lock().unwrap().last_send = Some(Instant::now());
        let _ = self.sender.send(StatusMsg::KeepAlive { text, emoji });
    }

    fn maybe_send_line(&self) {
        let settings = self.ctx.settings.read().unwrap();
        let mut playback = self.ctx.shared.playback.lock().unwrap();
        let mut tracker = self.ctx.shared.tracker.lock().unwrap();

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
        let offset = if settings.source == crate::config::Source::Lastfm {
            tracker
                .lastfm_lag
                .unwrap_or(DEFAULT_LASTFM_LAG_MS)
                .saturating_add(allowance)
        } else {
            allowance
        };
        let threshold = playback.song_progress.saturating_add(offset);

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
            tracker.last_send = Some(Instant::now());

            let (text, emoji) = build_status(&settings, &playback, &line);
            let _ = self.sender.send(StatusMsg::Update { text, emoji });
        }
    }

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
        let (done_tx, done_rx) = mpsc::channel();
        if self.sender.send(StatusMsg::Shutdown(done_tx)).is_ok() {
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
        }
    }
}

#[derive(Default)]
struct PollerState {
    spotify_token: Option<String>,
    last_logged_source: Option<crate::config::Source>,
}

fn poll_once(ctx: &AppContext, fetcher: &mut LyricsFetcher, poller: &mut PollerState) {
    let settings = ctx.settings.read().unwrap();
    let source = settings.source;
    let token = settings.token.clone();
    drop(settings);

    match source {
        crate::config::Source::Spotify => {
            if token.is_empty() {
                return;
            }
            poll_spotify(ctx, fetcher, poller, &token);
        }
        crate::config::Source::Lastfm => poll_lastfm(ctx, fetcher, poller),
    }
}

fn poll_spotify(
    ctx: &AppContext,
    fetcher: &mut LyricsFetcher,
    poller: &mut PollerState,
    token: &str,
) {
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
            poller.spotify_token = None;
            crate::log::write("spotify player returned 401, refreshing token");
            return;
        }
        Err(FetchError::Other(e)) => {
            crate::log::write(&format!("spotify player error: {e}"));
            return;
        }
    };
    let rtt = request_start.elapsed().as_millis() as u64;

    let song_changed = apply_state(ctx, &state, Some(state.progress_ms.saturating_add(rtt)));
    sync_lyrics(ctx, fetcher, song_changed);
}

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
            playback.song_progress = 0;
        }
    }
    song_changed
}

fn sync_lyrics(ctx: &AppContext, fetcher: &mut LyricsFetcher, song_changed: bool) {
    let lyrics_settings = ctx.settings.read().unwrap().lyrics.clone();

    let romanize_changed = fetcher.romanize_changed(&lyrics_settings);
    if !song_changed && !romanize_changed && ctx.shared.playback.lock().unwrap().has_lyrics {
        return;
    }

    let (name, artist) = {
        let pb = ctx.shared.playback.lock().unwrap();
        (pb.song_name.clone(), pb.song_author.clone())
    };
    if name.is_empty() {
        return;
    }

    if let Some(cached) = fetcher.read_cache(&name, &artist, lyrics_settings.romanize) {
        let mut pb = ctx.shared.playback.lock().unwrap();
        pb.lyrics = Some(cached);
        pb.has_lyrics = true;
        pb.current_line = None;
        *ctx.shared.lyric_source.lock().unwrap() = "cache".to_string();
        return;
    }

    let key = format!("{name}{artist}");
    if fetcher.last_fetched_for() == key && !fetcher.providers_changed(&lyrics_settings) {
        return;
    }

    let result = fetcher.fetch(&name, &artist, &lyrics_settings);

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
    let romanize = ctx.settings.read().unwrap().lyrics.romanize;
    let pb = ctx.shared.playback.lock().unwrap();
    UiSnapshot {
        song_name: pb.song_name.clone(),
        song_author: pb.song_author.clone(),
        song_progress: pb.song_progress,
        song_duration: pb.song_duration,
        is_playing: pb.is_playing,
        has_lyrics: pb.has_lyrics,
        current_line: pb.current_line.as_ref().map(|l| {
            if romanize {
                crate::romanize::romanize(&l.text)
            } else {
                l.text.clone()
            }
        }),
    }
}

pub fn last_source(ctx: &AppContext) -> String {
    ctx.shared.lyric_source.lock().unwrap().clone()
}

pub fn last_latency(ctx: &AppContext) -> u64 {
    ctx.shared.tracker.lock().unwrap().last_latency
}

/// Detects the playing -> paused transition, updating the tracked previous
/// state. Returns true exactly once per pause so the status is reverted
/// rather than spamming Discord.
fn playing_edge_to_pause(ctx: &AppContext) -> bool {
    let mut tracker = ctx.shared.tracker.lock().unwrap();
    let playing = ctx.shared.playback.lock().unwrap().is_playing;
    let edge = tracker.prev_playing && !playing;
    tracker.prev_playing = playing;
    edge
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::state::{AppContext, LyricsLine, Shared};
    use std::sync::{Arc, RwLock};

    fn test_ctx() -> Arc<AppContext> {
        Arc::new(AppContext::new(
            Shared::new(),
            Arc::new(RwLock::new(Settings::default())),
            std::path::PathBuf::from("/nonexistent"),
        ))
    }

    fn set_playing(ctx: &AppContext, playing: bool) {
        ctx.shared.playback.lock().unwrap().is_playing = playing;
    }

    #[test]
    fn pause_edge_fires_only_on_the_transition() {
        let ctx = test_ctx();
        // Starts paused: no edge.
        set_playing(&ctx, false);
        assert!(!playing_edge_to_pause(&ctx));

        // Start playing: no edge.
        set_playing(&ctx, true);
        assert!(!playing_edge_to_pause(&ctx));

        // Pause: fires exactly once.
        set_playing(&ctx, false);
        assert!(playing_edge_to_pause(&ctx));
        // Second tick while still paused: no repeat.
        assert!(!playing_edge_to_pause(&ctx));

        // Resume, then pause again: fires again.
        set_playing(&ctx, true);
        assert!(!playing_edge_to_pause(&ctx));
        set_playing(&ctx, false);
        assert!(playing_edge_to_pause(&ctx));
    }

    #[test]
    fn never_playing_never_fires() {
        let ctx = test_ctx();
        assert!(!playing_edge_to_pause(&ctx));
        assert!(!playing_edge_to_pause(&ctx));
    }

    #[test]
    fn sending_a_line_records_last_send() {
        let ctx = test_ctx();
        let engine = Engine::new(ctx.clone());
        {
            let mut pb = ctx.shared.playback.lock().unwrap();
            pb.song_id = "id".to_string();
            pb.song_name = "Song".to_string();
            pb.song_author = "Artist".to_string();
            pb.has_lyrics = true;
            pb.is_playing = true;
            pb.song_progress = 2_000;
            pb.song_duration = 300_000;
            pb.lyrics = Some(vec![LyricsLine {
                time: 1_500,
                text: "hello".to_string(),
            }]);
        }

        assert!(ctx.shared.tracker.lock().unwrap().last_send.is_none());
        engine.maybe_send_line();
        let last_send = ctx.shared.tracker.lock().unwrap().last_send;
        assert!(
            last_send.is_some(),
            "a real line send must record when the status was last pushed"
        );
        assert!(last_send.unwrap().elapsed().as_millis() < 1_000);
    }

    #[test]
    fn keep_alive_refreshes_last_send_on_an_expiring_status() {
        let ctx = test_ctx();
        let engine = Engine::new(ctx.clone());
        {
            let mut pb = ctx.shared.playback.lock().unwrap();
            pb.song_id = "id".to_string();
            pb.song_name = "Song".to_string();
            pb.song_author = "Artist".to_string();
            pb.has_lyrics = true;
            pb.is_playing = true;
            pb.song_duration = 300_000;
            pb.current_line = Some(LyricsLine {
                time: 9_000,
                text: "still playing".to_string(),
            });
        }
        // Simulate the last push being long enough ago that the status is
        // about to expire.
        ctx.shared.tracker.lock().unwrap().last_send =
            Some(Instant::now() - Duration::from_millis(STATUS_KEEPALIVE_INTERVAL_MS + 10_000));

        engine.maybe_keep_alive();
        let last_send = ctx.shared.tracker.lock().unwrap().last_send;
        assert!(
            last_send.unwrap().elapsed().as_millis() < 1_000,
            "an expiring status should be refreshed"
        );
    }

    #[test]
    fn keep_alive_skips_within_the_refresh_window() {
        let ctx = test_ctx();
        let engine = Engine::new(ctx.clone());
        let before = Instant::now() - Duration::from_millis(5_000);
        ctx.shared.tracker.lock().unwrap().last_send = Some(before);

        engine.maybe_keep_alive();
        assert_eq!(
            ctx.shared.tracker.lock().unwrap().last_send,
            Some(before),
            "a recent push should not be refreshed yet"
        );
    }
}
