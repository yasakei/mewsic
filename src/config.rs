use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Spotify,
    Lastfm,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Spotify => "Spotify",
            Source::Lastfm => "Last.fm",
        }
    }

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
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token: String,
    pub source: Source,
    pub lastfm: LastFmSettings,
    pub view: ViewSettings,
    pub timing: TimingSettings,
    pub update: UpdateSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LastFmSettings {
    pub api_key: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSettings {
    pub timestamp: bool,
    pub label: bool,
    pub emoji: String,
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
    pub send_time_offset: u64,
    pub enable_autooffset: bool,
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
    pub auto_start: bool,
    pub auto_check: bool,
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        let path = dir.join("settings.toml");
        let mut settings = if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(settings) = toml::from_str::<Settings>(&raw) {
                settings
            } else {
                default_with_migration(dir)
            }
        } else {
            default_with_migration(dir)
        };

        if let Some(stored) = crate::credential::load_token(dir) {
            settings.token = stored;
        } else if !settings.token.is_empty()
            && crate::credential::store_token(dir, &settings.token).is_ok()
        {
            let mut stripped = settings.clone();
            stripped.token = String::new();
            let _ = Self::write_toml(dir, &stripped);
        }
        settings
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let _ = fs::create_dir_all(dir);
        let mut file_settings = self.clone();
        if self.token.is_empty() {
            crate::credential::clear_token(dir);
        } else {
            match crate::credential::store_token(dir, &self.token) {
                Ok(()) => file_settings.token = String::new(),
                Err(e) => crate::log::write(&format!(
                    "credential store unavailable; token stays in settings.toml ({e})"
                )),
            }
        }
        Self::write_toml(dir, &file_settings)
    }

    fn write_toml(dir: &Path, settings: &Settings) -> Result<(), String> {
        let path = dir.join("settings.toml");
        let raw = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
        fs::write(&path, raw).map_err(|e| e.to_string())?;
        restrict_permissions(&path);
        Ok(())
    }
}

fn default_with_migration(dir: &Path) -> Settings {
    let mut settings = Settings::default();
    if let Some(migrated) = migrate_legacy(dir) {
        settings = migrated;
    }
    settings
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

fn migrate_legacy(dir: &Path) -> Option<Settings> {
    for path in [dir.join("settings.json"), PathBuf::from("settings.json")] {
        if let Some(settings) = read_legacy_json(&path) {
            if path.is_file() {
                let _ = fs::remove_file(&path);
            }
            return Some(settings);
        }
    }
    None
}

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

    static KEYRING_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _guard = KEYRING_ENV_LOCK.lock().unwrap();
        std::env::set_var("MEWSIC_KEYRING_SERVICE", "mewsic-legacy-test");
        std::env::set_var("MEWSIC_KEYRING_USER", "legacy-user");
        let dir = std::env::temp_dir().join(format!("mewsic-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("settings.json"), r#"{"credentials":{"token":"t"}}"#).unwrap();
        let s = Settings::load(&dir);
        assert_eq!(s.source, Source::Spotify);
        assert_eq!(s.token, "t");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn token_is_never_written_to_settings_toml() {
        let _guard = KEYRING_ENV_LOCK.lock().unwrap();
        std::env::set_var("MEWSIC_KEYRING_SERVICE", "mewsic-token-test");
        std::env::set_var("MEWSIC_KEYRING_USER", "token-test");
        let dir = std::env::temp_dir().join(format!("mewsic-token-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);

        let s = Settings {
            token: "super-secret".into(),
            ..Settings::default()
        };
        s.save(&dir).unwrap();

        let raw = fs::read_to_string(dir.join("settings.toml")).unwrap();
        assert!(
            !raw.contains("super-secret"),
            "token must not be written to settings.toml:\n{raw}"
        );

        let loaded = Settings::load(&dir);
        assert_eq!(loaded.token, "super-secret");

        let cleared = Settings::default();
        cleared.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir).token, "", "clearing must drop the credential");

        let _ = fs::remove_dir_all(&dir);
    }
}
