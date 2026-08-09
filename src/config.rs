//! Configuration loading, saving and migration.
//!
//! Mewsic stores settings as TOML at `~/.config/mewsic/settings.toml`. On first
//! launch it can migrate a legacy `settings.json` (from the old Node app) so
//! existing tokens keep working.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Platform-aware config directory (`$MEWSIC_CONFIG_DIR` overrides everything).
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEWSIC_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("mewsic");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("mewsic");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config").join("mewsic");
        }
    }
    PathBuf::from(".")
}

/// Which playback source the engine polls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Spotify, via the OAuth token Discord holds (the original backend).
    #[default]
    Spotify,
    /// Last.fm, via its public scrobble API (api key + username). Covers
    /// YouTube Music and any other web player once scrobbled — e.g. with the
    /// WebScrobbler extension or the YT Music desktop app's built-in
    /// Last.fm integration.
    Lastfm,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Spotify => "Spotify",
            Source::Lastfm => "Last.fm",
        }
    }

    /// Parse a user-typed choice (case-insensitive) back into a source.
    pub fn parse(input: &str) -> Option<Source> {
        match input.trim().to_ascii_lowercase().as_str() {
            "spotify" | "sp" | "discord" | "dc" => Some(Source::Spotify),
            "lastfm" | "last.fm" | "lf" | "ytmusic" | "ytm" => Some(Source::Lastfm),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Discord user token — the only credential.
    pub token: String,
    /// Playback backend: Spotify, Last.fm or local MPRIS players.
    pub source: Source,
    pub lastfm: LastFmSettings,
    pub view: ViewSettings,
    pub timing: TimingSettings,
    pub update: UpdateSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LastFmSettings {
    /// Last.fm API key (https://www.last.fm/api/account/create).
    pub api_key: String,
    /// Last.fm username whose scrobbles to follow.
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSettings {
    /// Prepend `[m:ss]` to the status text.
    pub timestamp: bool,
    /// Prepend `Song lyrics -` to the status text.
    pub label: bool,
    /// Optional emoji shown next to the status. Empty = none.
    pub emoji: String,
    /// Clear the status immediately when the song changes.
    pub auto_clear: bool,
    pub advanced: AdvancedSettings,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings {
            timestamp: true,
            label: true,
            emoji: String::new(),
            auto_clear: true,
            advanced: AdvancedSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// When true, `template` replaces the simple format.
    pub enabled: bool,
    pub emoji: String,
    pub template: String,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        AdvancedSettings {
            enabled: false,
            emoji: String::new(),
            template: "[{timestamp}] [{lyrics}]".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TimingSettings {
    /// Fixed ms to send the status ahead of the lyric timestamp.
    pub send_time_offset: u64,
    /// When true the offset is derived from Discord API latency.
    pub enable_autooffset: bool,
    /// Number of latency samples used for the autooffset average.
    pub autooffset: usize,
}

impl Default for TimingSettings {
    fn default() -> Self {
        TimingSettings {
            send_time_offset: 500,
            enable_autooffset: true,
            autooffset: 3,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Launch on login.
    pub auto_start: bool,
}

impl Settings {
    /// Load settings from `dir/settings.toml`, falling back to defaults and
    /// attempting a migration from a legacy `settings.json` in the same dir.
    pub fn load(dir: &Path) -> Settings {
        let path = dir.join("settings.toml");
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(settings) = toml::from_str::<Settings>(&raw) {
                return settings;
            }
        }

        let mut settings = Settings::default();
        if let Some(migrated) = migrate_legacy(dir) {
            settings = migrated;
        }
        settings
    }

    /// Persist to `dir/settings.toml`, creating the directory if needed.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let _ = fs::create_dir_all(dir);
        let path = dir.join("settings.toml");
        let raw = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, raw).map_err(|e| e.to_string())
    }
}

/// Import a legacy `settings.json` (the old Node app format) if present.
/// Checks the config dir first, then the current working directory.
fn migrate_legacy(dir: &Path) -> Option<Settings> {
    for path in [dir.join("settings.json"), PathBuf::from("settings.json")] {
        if let Some(settings) = read_legacy_json(&path) {
            // Only keep a migrated settings.json once we've consumed it.
            if path.is_file() {
                let _ = fs::remove_file(&path);
            }
            return Some(settings);
        }
    }
    None
}

/// Parse one legacy `settings.json` file, or `None` if it's absent/invalid.
fn read_legacy_json(path: &Path) -> Option<Settings> {
    let raw = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;

    let mut settings = Settings::default();

    if let Some(token) = json
        .pointer("/credentials/token")
        .and_then(|v| v.as_str())
    {
        settings.token = token.to_string();
    }
    if let Some(view) = json.get("view") {
        settings.view.timestamp = view
            .get("timestamp")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        settings.view.label = view.get("label").and_then(|v| v.as_bool()).unwrap_or(true);
        settings.view.auto_clear = view
            .get("autoClear")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if let Some(emoji) = view.get("emoji").and_then(|v| v.as_str()) {
            settings.view.emoji = emoji.to_string();
        }
        if let Some(adv) = view.get("advanced") {
            settings.view.advanced.enabled = adv
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(e) = adv.get("customEmoji").and_then(|v| v.as_str()) {
                settings.view.advanced.emoji = e.to_string();
            }
            if let Some(t) = adv.get("customStatus").and_then(|v| v.as_str()) {
                settings.view.advanced.template = t.to_string();
            }
        }
    }
    if let Some(tim) = json.get("timings") {
        settings.timing.send_time_offset = tim
            .get("sendTimeOffset")
            .and_then(|v| v.as_u64())
            .unwrap_or(500);
        settings.timing.enable_autooffset = tim
            .get("enableAutooffset")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        settings.timing.autooffset = tim
            .get("autooffset")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(3);
    }
    if let Some(upd) = json.get("update") {
        settings.update.auto_start = upd
            .get("autoStart")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }

    Some(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_defaults_to_spotify() {
        let s = Settings::default();
        assert_eq!(s.source, Source::Spotify);
    }

    #[test]
    fn source_parse_aliases() {
        assert_eq!(Source::parse("spotify"), Some(Source::Spotify));
        assert_eq!(Source::parse("SP"), Some(Source::Spotify));
        assert_eq!(Source::parse("lastfm"), Some(Source::Lastfm));
        assert_eq!(Source::parse("last.fm"), Some(Source::Lastfm));
        assert_eq!(Source::parse("ytmusic"), Some(Source::Lastfm));
        assert_eq!(Source::parse("ytm"), Some(Source::Lastfm));
        assert_eq!(Source::parse("slack"), None);
    }

    #[test]
    fn serde_roundtrip_keeps_source() {
        let mut s = Settings {
            source: Source::Lastfm,
            ..Settings::default()
        };
        s.lastfm.api_key = "abc".into();
        s.lastfm.username = "someone".into();
        let raw = toml::to_string(&s).unwrap();
        assert!(raw.contains("source = \"lastfm\""));
        let back: Settings = toml::from_str(&raw).unwrap();
        assert_eq!(back.source, Source::Lastfm);
        assert_eq!(back.lastfm.username, "someone");
    }

    #[test]
    fn legacy_settings_have_no_source() {
        // A settings.json without a `source` must migrate to the default.
        let dir = std::env::temp_dir().join(format!("mewsic-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("settings.json"), r#"{"credentials":{"token":"t"}}"#).unwrap();
        let s = Settings::load(&dir);
        assert_eq!(s.source, Source::Spotify);
        assert_eq!(s.token, "t");
        let _ = fs::remove_dir_all(&dir);
    }
}
