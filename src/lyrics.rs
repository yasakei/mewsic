use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::net;
use crate::state::LyricsLine;
use crate::util::{decode_html_entities, sanitize_filename, urlencode};

pub trait Source {
    fn app_name(&self) -> String;
    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String>;
}

pub struct LrcLibSource;

impl Source for LrcLibSource {
    fn app_name(&self) -> String {
        "LrcLib".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        let url = format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}",
            urlencode(title),
            urlencode(artist)
        );
        let resp = net::agent().get(&url).call().map_err(|e| e.to_string())?;
        if resp.status() != 200 {
            return Err(format!("LrcLib responded {}", resp.status()));
        }
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
        let synced = json
            .get("syncedLyrics")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if synced.trim().is_empty() {
            return Err("no synced lyrics".into());
        }
        Ok(parse_lrc(synced))
    }
}

pub struct NetEaseSource;

impl Source for NetEaseSource {
    fn app_name(&self) -> String {
        "NetEase Music".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        let song_id = self.song_id(title, artist)?;

        let url = format!(
            "https://music.163.com/api/song/lyric?tv=-1&kv=-1&lv=-1&os=pc&id={song_id}"
        );
        let resp = self
            .post(&url)
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;

        let lyric = json
            .pointer("/lrc/lyric")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if lyric.trim().is_empty() {
            return Err("no lyric in response".into());
        }
        Ok(parse_lrc(lyric))
    }
}

impl NetEaseSource {
    fn post(&self, url: &str) -> Result<ureq::Response, Box<ureq::Error>> {
        net::agent()
            .post(url)
            .set("Referer", "https://music.163.com")
            .set("Cookie", "appver=2.0.2")
            .set("X-Real-IP", "202.96.0.0")
            .call()
            .map_err(Box::new)
    }

    fn song_id(&self, title: &str, artist: &str) -> Result<i64, String> {
        let url = format!(
            "https://music.163.com/api/search/get?s={}&type=1&offset=0&sub=false&limit=5",
            urlencode(&format!("{title}-{artist}"))
        );
        let resp = self.post(&url).map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;

        let count = json
            .pointer("/result/songCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if count == 0 {
            return Err("song not found".into());
        }
        json.pointer("/result/songs/0/id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "no song id in search".into())
    }
}

pub struct QqMusicSource;

impl Source for QqMusicSource {
    fn app_name(&self) -> String {
        "QQ Music".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        let mid = self.song_mid(title, artist)?;

        let url = format!(
            "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?g_tk=5381&format=json&inCharset=utf-8&outCharset=utf-8&songmid={mid}"
        );
        let resp = net::agent()
            .get(&url)
            .set("Referer", "http://y.qq.com/portal/player.html")
            .call()
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;

        let b64 = json
            .get("lyric")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if b64.trim().is_empty() {
            return Err("no lyric in response".into());
        }

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("bad base64: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(parse_lrc(&decode_html_entities(&text)))
    }
}

impl QqMusicSource {
    fn song_mid(&self, title: &str, artist: &str) -> Result<String, String> {
        let url = format!(
            "https://c.y.qq.com/splcloud/fcgi-bin/smartbox_new.fcg?inCharset=utf-8&outCharset=utf-8&format=json&key={}",
            urlencode(&format!("{title}-{artist}"))
        );
        let resp = net::agent()
            .get(&url)
            .set("Referer", "http://y.qq.com/portal/player.html")
            .call()
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;

        let count = json.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        if count == 0 {
            return Err("song not found".into());
        }
        json.pointer("/data/song/itemlist/0/mid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| "no mid in search".into())
    }
}

/// A user-defined lyrics provider backed by a URL template and an optional
/// JSON pointer used to locate the raw LRC text inside a JSON response.
pub struct CustomSource {
    config: crate::config::CustomProvider,
}

impl CustomSource {
    pub fn new(config: crate::config::CustomProvider) -> CustomSource {
        CustomSource { config }
    }

    fn api_key(&self) -> &str {
        self.config.api_key.as_deref().unwrap_or("").trim()
    }

    fn request_url(&self, title: &str, artist: &str) -> String {
        self.config
            .url
            .replace("{title}", &urlencode(title))
            .replace("{artist}", &urlencode(artist))
            .replace("{api_key}", &urlencode(self.api_key()))
    }
}

impl Source for CustomSource {
    fn app_name(&self) -> String {
        if self.config.name.trim().is_empty() {
            "Custom".to_string()
        } else {
            self.config.name.clone()
        }
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        if self.config.url.trim().is_empty() {
            return Err("custom provider has no url".into());
        }
        let api_key = self.api_key();
        let url = self.request_url(title, artist);
        let req = net::agent().get(&url);
        let req = if !api_key.is_empty() {
            req.set("Authorization", &format!("Bearer {api_key}"))
        } else {
            req
        };
        let resp = req.call().map_err(|e| e.to_string())?;
        if resp.status() != 200 {
            return Err(format!("custom provider responded {}", resp.status()));
        }
        let body = resp
            .into_string()
            .map_err(|e| format!("custom provider read error: {e}"))?;

        let lrc = match &self.config.json_path {
            Some(path) if !path.is_empty() => {
                let json: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| format!("bad json: {e}"))?;
                json.pointer(path)
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("json path {path:?} not found"))?
                    .to_string()
            }
            _ => body,
        };
        if lrc.trim().is_empty() {
            return Err("custom provider returned no lyrics".into());
        }
        Ok(parse_lrc(&lrc))
    }
}

pub fn parse_lrc(text: &str) -> Vec<LyricsLine> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (times, body) = split_timestamps(line);
        if times.is_empty() || body.trim().is_empty() {
            continue;
        }
        for t in times {
            out.push(LyricsLine {
                time: t,
                text: body.trim().to_string(),
            });
        }
    }
    out.sort_by_key(|l| l.time);
    out
}

fn split_timestamps(line: &str) -> (Vec<u64>, String) {
    let mut times = Vec::new();
    let mut body = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find('[') {
        body.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if let Some(end) = after.find(']') {
            if let Some(ms) = parse_time(&after[..end]) {
                times.push(ms);
                rest = &after[end + 1..];
                continue;
            }
        }
        body.push('[');
        rest = &rest[pos + 1..];
    }
    body.push_str(rest);
    (times, body)
}

fn parse_time(tag: &str) -> Option<u64> {
    let (min_s, sec_s) = tag.split_once(':')?;
    let min: u64 = min_s.trim().parse().ok()?;
    let sec: f64 = sec_s.trim().parse().ok()?;
    Some(min * 60_000 + (sec * 1000.0).round() as u64)
}

#[derive(Serialize, Deserialize)]
struct CachedLyrics {
    source: String,
    lines: Vec<LyricsLine>,
    /// The same lines with `lyrics.romanize` applied, filled in the first
    /// time the song is loaded with romanization enabled. Older cache files
    /// lack the field and deserialize to an empty vec.
    #[serde(default)]
    romanized: Vec<LyricsLine>,
}

pub struct LyricsFetcher {
    cache_dir: PathBuf,
    last_fetched_for: String,
    last_provider_sig: String,
    /// Romanization flag the current cache read/fetch was made with, so a
    /// setting change triggers a reload from cache.
    last_romanize: Option<bool>,
}

impl LyricsFetcher {
    pub fn new(config_dir: &Path) -> LyricsFetcher {
        LyricsFetcher {
            cache_dir: config_dir.join("cache"),
            last_fetched_for: String::new(),
            last_provider_sig: String::new(),
            last_romanize: None,
        }
    }

    pub fn last_fetched_for(&self) -> &str {
        &self.last_fetched_for
    }

    /// True when the configured provider set differs from the last fetch
    /// attempt, so a same-song retry must not be skipped by the anti-hammer
    /// guard (e.g. the user just toggled a provider back on).
    pub fn providers_changed(&self, lyrics: &crate::config::LyricsSettings) -> bool {
        self.last_fetched_for.is_empty() || self.last_provider_sig != provider_sig(lyrics)
    }

    /// True when the romanization setting changed since the lyrics currently
    /// in playback were loaded, so they must be re-read from cache.
    pub fn romanize_changed(&self, lyrics: &crate::config::LyricsSettings) -> bool {
        self.last_romanize.is_some_and(|prev| prev != lyrics.romanize)
    }

    fn cache_path(&self, title: &str, artist: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}-{}.json", sanitize_filename(title), sanitize_filename(artist)))
    }

    /// Cached lyrics for a song, honoring the romanization setting: with it
    /// enabled the stored romanized copy is served (romanized once and
    /// persisted on first use); with it disabled the original lines are.
    pub fn read_cache(&mut self, title: &str, artist: &str, romanize: bool) -> Option<Vec<LyricsLine>> {
        self.last_romanize = Some(romanize);
        let raw = fs::read_to_string(self.cache_path(title, artist)).ok()?;
        let mut parsed: CachedLyrics = serde_json::from_str(&raw).ok()?;
        if parsed.lines.is_empty() {
            return None;
        }
        if romanize {
            if parsed.romanized.is_empty() {
                parsed.romanized = parsed
                    .lines
                    .iter()
                    .map(|l| LyricsLine {
                        time: l.time,
                        text: crate::romanize::romanize(&l.text),
                    })
                    .collect();
                self.store_romanized(title, artist, &parsed);
            }
            return Some(parsed.romanized);
        }
        Some(parsed.lines)
    }

    /// Persist the romanized copy of a song's lyrics into its cache file,
    /// keeping the original lines intact.
    fn store_romanized(&self, title: &str, artist: &str, updated: &CachedLyrics) {
        let _ = fs::write(
            self.cache_path(title, artist),
            serde_json::to_string(updated).unwrap_or_default(),
        );
    }

    fn write_cache(&self, title: &str, artist: &str, source: &str, lines: &[LyricsLine]) {
        let _ = fs::create_dir_all(&self.cache_dir);
        let cached = CachedLyrics {
            source: source.to_string(),
            lines: lines.to_vec(),
            romanized: Vec::new(),
        };
        let _ = fs::write(
            self.cache_path(title, artist),
            serde_json::to_string(&cached).unwrap_or_default(),
        );
    }

    /// Build the ordered list of sources requested by `lyrics`, excluding any
    /// provider that is toggled off (and a custom provider with no URL).
    fn enabled_sources(&self, lyrics: &crate::config::LyricsSettings) -> Vec<Box<dyn Source>> {
        let mut out: Vec<Box<dyn Source>> = Vec::new();
        for id in &lyrics.providers {
            match id.as_str() {
                "lrclib" => out.push(Box::new(LrcLibSource)),
                "netease" => out.push(Box::new(NetEaseSource)),
                "qqmusic" => out.push(Box::new(QqMusicSource)),
                "custom" => {
                    if let Some(custom) = &lyrics.custom {
                        if !custom.url.trim().is_empty() {
                            out.push(Box::new(CustomSource::new(custom.clone())));
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn fetch(
        &mut self,
        title: &str,
        artist: &str,
        lyrics: &crate::config::LyricsSettings,
    ) -> Option<(Vec<LyricsLine>, String)> {
        self.last_fetched_for = format!("{title}{artist}");
        self.last_provider_sig = provider_sig(lyrics);

        if let Some(cached) = self.read_cache(title, artist, lyrics.romanize) {
            return Some((cached, "cache".to_string()));
        }

        let sources = self.enabled_sources(lyrics);
        for source in sources {
            match source.fetch(title, artist) {
                Ok(lines) if !lines.is_empty() => {
                    self.write_cache(title, artist, &source.app_name(), &lines);
                    self.last_romanize = Some(lyrics.romanize);
                    if lyrics.romanize {
                        let romanized: Vec<LyricsLine> = lines
                            .iter()
                            .map(|l| LyricsLine {
                                time: l.time,
                                text: crate::romanize::romanize(&l.text),
                            })
                            .collect();
                        self.store_romanized(
                            title,
                            artist,
                            &CachedLyrics {
                                source: source.app_name(),
                                lines: lines.clone(),
                                romanized: romanized.clone(),
                            },
                        );
                        return Some((romanized, source.app_name()));
                    }
                    return Some((lines, source.app_name()));
                }
                _ => continue,
            }
        }

        None
    }
}

/// A signature of what a fetch would produce: the provider ids plus the custom
/// provider's url/api key/json path. Used to detect provider changes so the
/// same song gets refetched instead of being skipped by the anti-hammer guard.
fn provider_sig(lyrics: &crate::config::LyricsSettings) -> String {
    let custom = lyrics
        .custom
        .as_ref()
        .map(|c| {
            format!(
                "{}|{}|{}",
                c.url,
                c.api_key.as_deref().unwrap_or(""),
                c.json_path.as_deref().unwrap_or("")
            )
        })
        .unwrap_or_default();
    format!("{}::{}", lyrics.providers.join(","), custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single() {
        let lines = parse_lrc("[01:02.50] hello world");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].time, 62_500);
        assert_eq!(lines[0].text, "hello world");
    }

    #[test]
    fn parses_multi_timestamp() {
        let lines = parse_lrc("[00:01.00][00:02.00] chorus");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time, 1_000);
        assert_eq!(lines[1].time, 2_000);
        assert_eq!(lines[0].text, "chorus");
    }

    #[test]
    fn drops_metadata_and_sorts() {
        let lines = parse_lrc("[ti:Some Song]\n[03:00.00] b\n[01:00.00] a");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "a");
        assert_eq!(lines[1].time, 180_000);
    }

    #[test]
    fn handles_seconds_only() {
        let lines = parse_lrc("[00:05] beep");
        assert_eq!(lines[0].time, 5_000);
    }

    #[test]
    fn handles_mid_line_timestamps() {
        let lines = parse_lrc("text [00:05.00] more");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].time, 5_000);
        assert!(lines[0].text.contains("text"));
    }

    #[test]
    fn keeps_metadata_as_text() {
        let (times, body) = split_timestamps("[ti:Some Song] title");
        assert!(times.is_empty());
        assert!(body.contains("[ti:Some Song]"));
    }

    #[test]
    fn enabled_sources_respect_provider_list() {
        use crate::config::LyricsSettings;
        let fetcher = LyricsFetcher::new(std::path::Path::new("/nonexistent"));
        let lyrics = LyricsSettings {
            providers: vec!["netease".into()],
            ..LyricsSettings::default()
        };
        let sources = fetcher.enabled_sources(&lyrics);
        assert_eq!(sources.len(), 1);

        let empty = LyricsSettings {
            providers: vec![],
            ..LyricsSettings::default()
        };
        assert!(fetcher.enabled_sources(&empty).is_empty());
    }

    #[test]
    fn enabled_sources_skips_custom_without_url() {
        use crate::config::{CustomProvider, LyricsSettings};
        let fetcher = LyricsFetcher::new(std::path::Path::new("/nonexistent"));
        let empty_custom = LyricsSettings {
            providers: vec!["custom".into()],
            romanize: false,
            custom: Some(CustomProvider {
                name: "My".into(),
                url: String::new(),
                api_key: None,
                json_path: None,
            }),
        };
        assert!(fetcher.enabled_sources(&empty_custom).is_empty());

        let filled_custom = LyricsSettings {
            custom: Some(CustomProvider {
                name: "My".into(),
                url: "https://example.com/{title}".into(),
                api_key: None,
                json_path: None,
            }),
            ..empty_custom
        };
        assert_eq!(fetcher.enabled_sources(&filled_custom).len(), 1);
    }

    #[test]
    fn providers_changed_detects_provider_edits() {
        use crate::config::LyricsSettings;
        let mut fetcher = LyricsFetcher::new(std::path::Path::new("/nonexistent"));

        // Nothing fetched yet -> differs, so a retry is allowed.
        assert!(fetcher.providers_changed(&LyricsSettings::default()));

        fetcher.last_fetched_for = "songartist".to_string();

        let mut lyrics = LyricsSettings {
            providers: vec!["lrclib".into(), "netease".into()],
            romanize: false,
            custom: None,
        };
        fetcher.last_provider_sig = provider_sig(&lyrics);
        // Same song + same providers -> the anti-hammer guard can skip.
        assert!(!fetcher.providers_changed(&lyrics));

        // Toggling a provider (or editing the custom one) invalidates the guard.
        lyrics.providers = vec!["netease".into()];
        assert!(fetcher.providers_changed(&lyrics));

        lyrics.providers = vec!["custom".into()];
        lyrics.custom = Some(crate::config::CustomProvider {
            name: "x".into(),
            url: "https://example.com/{title}".into(),
            api_key: Some("k".into()),
            json_path: None,
        });
        fetcher.last_provider_sig = provider_sig(&lyrics);
        assert!(!fetcher.providers_changed(&lyrics));

        lyrics.custom.as_mut().unwrap().api_key = Some("k2".into());
        assert!(fetcher.providers_changed(&lyrics));
    }

    #[test]
    fn custom_source_substitutes_all_placeholders() {
        let source = CustomSource::new(crate::config::CustomProvider {
            name: "My".into(),
            url: "https://example.com/q?t={title}&a={artist}&k={api_key}".into(),
            api_key: Some("top secret".into()),
            json_path: None,
        });
        let url = source.request_url("Hello World", "AC/DC");
        assert!(url.contains("t=Hello%20World"), "{url}");
        assert!(url.contains("a=AC%2FDC"), "{url}");
        assert!(url.contains("k=top%20secret"), "{url}");

        let no_key = CustomSource::new(crate::config::CustomProvider {
            name: "My".into(),
            url: "https://example.com/q?t={title}&k={api_key}".into(),
            api_key: None,
            json_path: None,
        });
        assert!(no_key.request_url("s", "a").ends_with("k="));
    }

    #[test]
    fn custom_source_fetches_from_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            let auth = req
                .lines()
                .find_map(|l| l.strip_prefix("Authorization: "))
                .unwrap_or("")
                .to_string();
            let body = "[00:01.00] hello\n[00:02.00] world";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).unwrap();
            auth
        });

        let source = CustomSource::new(crate::config::CustomProvider {
            name: "Local".into(),
            url: format!("http://127.0.0.1:{port}/lrc?t={{title}}&k={{api_key}}"),
            api_key: Some("sekrit".into()),
            json_path: None,
        });
        let lines = source.fetch("song", "artist").unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "hello");
        assert_eq!(lines[1].time, 2_000);
        assert_eq!(server.join().unwrap(), "Bearer sekrit");
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn temp_cache_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mewsic-lyrics-cache-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn write_raw_cache(dir: &std::path::Path, title: &str, artist: &str, text: &str) {
        let fetcher = LyricsFetcher::new(dir);
        let lines = vec![LyricsLine {
            time: 0,
            text: text.to_string(),
        }];
        fetcher.write_cache(title, artist, "test", &lines);
    }

    #[test]
    fn romanized_copy_is_persisted_and_served_per_setting() {
        let dir = temp_cache_dir("romanize");
        write_raw_cache(&dir, "Song", "Artist", "今日は");

        let mut fetcher = LyricsFetcher::new(&dir);

        // First read with romanization on: romanizes once, serves it and
        // persists the romanized copy alongside the original.
        let lines = fetcher.read_cache("Song", "Artist", true).unwrap();
        assert_eq!(lines[0].text, "kyouha");
        let raw = std::fs::read_to_string(dir.join("cache/Song-Artist.json")).unwrap();
        assert!(raw.contains("kyouha"), "romanized copy must be persisted");
        assert!(raw.contains("今日は"), "original must be kept");

        // Reads with the setting off keep serving the original.
        let lines = fetcher.read_cache("Song", "Artist", false).unwrap();
        assert_eq!(lines[0].text, "今日は");

        // And back on, the persisted romanized copy is reused as-is.
        let lines = fetcher.read_cache("Song", "Artist", true).unwrap();
        assert_eq!(lines[0].text, "kyouha");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn romanize_setting_change_forces_cache_reload() {
        let dir = temp_cache_dir("toggle");
        write_raw_cache(&dir, "Song2", "Artist", "さくら");
        let settings = crate::config::LyricsSettings::default();

        let mut fetcher = LyricsFetcher::new(&dir);
        let _ = fetcher.read_cache("Song2", "Artist", false).unwrap();
        assert!(!fetcher.romanize_changed(&settings));

        let mut on = settings.clone();
        on.romanize = true;
        assert!(fetcher.romanize_changed(&on), "toggle must trigger reload");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
