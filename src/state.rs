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

#[derive(Debug, Default)]
pub struct Tracker {
    pub sent_lines: Vec<u64>,
    pub last_seen_song: String,
    pub latencies: Vec<u64>,
    pub last_latency: u64,
    pub lastfm_lag: Option<u64>,
    /// Whether the player was playing on the previous tick, used to detect the
    /// playing -> paused edge so the status reverts when the song is paused.
    pub prev_playing: bool,
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

#[derive(Debug, Clone, Default)]
pub struct UpdateState {
    pub latest: Option<String>,
    pub message: String,
}

pub struct Shared {
    pub playback: Mutex<Playback>,
    pub tracker: Mutex<Tracker>,
    pub lyric_source: Mutex<String>,
    pub update: Mutex<UpdateState>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            playback: Mutex::new(Playback::default()),
            tracker: Mutex::new(Tracker::default()),
            lyric_source: Mutex::new("not fetched".to_string()),
            update: Mutex::new(UpdateState::default()),
        })
    }
}

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
