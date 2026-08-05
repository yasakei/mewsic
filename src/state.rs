//! Shared runtime state between the poller, status sender and UI.

use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};

use crate::config::Settings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricsLine {
    pub time: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Playback {
    pub song_name: String,
    pub song_author: String,
    pub song_id: String,
    pub old_song_id: String,
    pub song_duration: u64,
    pub song_progress: u64,
    pub lyrics: Option<Vec<LyricsLine>>,
    pub current_line: Option<LyricsLine>,
    pub has_lyrics: bool,
    pub is_playing: bool,
}

impl Playback {
    pub fn ended(&self) -> bool {
        self.song_duration > 0 && self.song_duration <= self.song_progress
    }
}

/// Tracks which lyric lines have been pushed to Discord and latency history.
#[derive(Debug, Default)]
pub struct Tracker {
    /// Timestamps of lines already sent for the current song.
    pub sent_lines: Vec<u64>,
    /// Song id at the time of the last status send.
    pub last_seen_song: String,
    /// Rolling latency samples (ms) for autooffset.
    pub latencies: Vec<u64>,
    pub last_latency: u64,
    /// Last.fm only: estimated detection lag (ms) — how long ago the current
    /// song started before the poller first noticed it. `None` until measured
    /// or when the estimate isn't trustworthy.
    pub lastfm_lag: Option<u64>,
}

impl Tracker {
    pub fn add_latency(&mut self, ms: u64, limit: usize) {
        self.latencies.push(ms);
        if self.latencies.len() > limit.max(1) {
            self.latencies.remove(0);
        }
        self.last_latency = ms;
    }

    pub fn avg_latency(&self) -> u64 {
        if self.latencies.is_empty() {
            return 0;
        }
        let sum: u64 = self.latencies.iter().sum();
        sum / self.latencies.len() as u64
    }
}

/// Everything the UI and web panel read, wrapped in cheap locks.
pub struct Shared {
    pub playback: Mutex<Playback>,
    pub tracker: Mutex<Tracker>,
    /// Human-readable provenance of the current lyrics ("LrcLib", "cache", ...).
    pub lyric_source: Mutex<String>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            playback: Mutex::new(Playback::default()),
            tracker: Mutex::new(Tracker::default()),
            lyric_source: Mutex::new("not fetched".to_string()),
        })
    }
}

/// Handles both live reloads (wizard / web panel) and the settings snapshot.
pub struct AppContext {
    pub shared: Arc<Shared>,
    pub settings: Arc<RwLock<Settings>>,
    pub config_dir: std::path::PathBuf,
}

impl AppContext {
    pub fn new(
        shared: Arc<Shared>,
        settings: Arc<RwLock<Settings>>,
        config_dir: std::path::PathBuf,
    ) -> AppContext {
        AppContext {
            shared,
            settings,
            config_dir,
        }
    }
}
