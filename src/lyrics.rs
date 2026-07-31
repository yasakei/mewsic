//! Synced-lyrics fetching with a layered source chain and a disk cache.
//!
//! Sources are tried in order (LrcLib → NetEase → QQ Music) and the first one
//! that returns timed lines wins. Results are cached under
//! `<config>/cache/<title>-<artist>.json` so repeat plays are instant.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::net;
use crate::state::LyricsLine;
use crate::util::{decode_html_entities, sanitize_filename, urlencode};

// ─── Source trait ────────────────────────────────────────────────────────────

pub trait Source {
    fn app_name(&self) -> &'static str;
    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String>;
}

// ─── LrcLib ──────────────────────────────────────────────────────────────────

/// https://lrclib.net/api
pub struct LrcLibSource;

impl Source for LrcLibSource {
    fn app_name(&self) -> &'static str {
        "LrcLib"
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

// ─── NetEase Music ───────────────────────────────────────────────────────────

/// https://music.163.com/api
pub struct NetEaseSource;

impl Source for NetEaseSource {
    fn app_name(&self) -> &'static str {
        "NetEase Music"
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

// ─── QQ Music ────────────────────────────────────────────────────────────────

/// https://c.y.qq.com lyric search (base64-encoded response, HTML entities).
pub struct QqMusicSource;

impl Source for QqMusicSource {
    fn app_name(&self) -> &'static str {
        "QQ Music"
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

// ─── LRC parsing ─────────────────────────────────────────────────────────────

/// Parse an LRC document: `[mm:ss.xx] text` lines, possibly several timestamps
/// per line. Lines without a valid timestamp (metadata) are dropped.
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

/// Collect every `[mm:ss(.xx)]` timestamp anywhere in `line`, returning the
/// times (in ms) and the remaining text. Non-time `[...]` tags (e.g. `[ti:…]`)
/// are treated as literal text.
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
        // Not a timestamp — keep the bracket literally and move on.
        body.push('[');
        rest = &rest[pos + 1..];
    }
    body.push_str(rest);
    (times, body)
}

/// Parse `mm:ss` or `mm:ss.xx` into milliseconds.
fn parse_time(tag: &str) -> Option<u64> {
    let (min_s, sec_s) = tag.split_once(':')?;
    let min: u64 = min_s.trim().parse().ok()?;
    let sec: f64 = sec_s.trim().parse().ok()?;
    Some(min * 60_000 + (sec * 1000.0).round() as u64)
}

// ─── Cache ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CachedLyrics {
    source: String,
    lines: Vec<LyricsLine>,
}

/// Layered fetcher with on-disk caching.
pub struct LyricsFetcher {
    sources: Vec<Box<dyn Source>>,
    cache_dir: PathBuf,
    /// `title + artist` key of the last attempt, to avoid redundant refetches.
    last_fetched_for: String,
}

impl LyricsFetcher {
    pub fn new(config_dir: &Path) -> LyricsFetcher {
        LyricsFetcher {
            sources: vec![
                Box::new(LrcLibSource),
                Box::new(NetEaseSource),
                Box::new(QqMusicSource),
            ],
            cache_dir: config_dir.join("cache"),
            last_fetched_for: String::new(),
        }
    }

    pub fn last_fetched_for(&self) -> &str {
        &self.last_fetched_for
    }

    fn cache_path(&self, title: &str, artist: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}-{}.json", sanitize_filename(title), sanitize_filename(artist)))
    }

    pub fn read_cache(&self, title: &str, artist: &str) -> Option<Vec<LyricsLine>> {
        let raw = fs::read_to_string(self.cache_path(title, artist)).ok()?;
        let parsed: CachedLyrics = serde_json::from_str(&raw).ok()?;
        if parsed.lines.is_empty() {
            return None;
        }
        Some(parsed.lines)
    }

    fn write_cache(&self, title: &str, artist: &str, source: &str, lines: &[LyricsLine]) {
        let _ = fs::create_dir_all(&self.cache_dir);
        let cached = CachedLyrics {
            source: source.to_string(),
            lines: lines.to_vec(),
        };
        let _ = fs::write(
            self.cache_path(title, artist),
            serde_json::to_string(&cached).unwrap_or_default(),
        );
    }

    /// Fetch timed lyrics for a track, checking the cache first. Returns the
    /// lines plus a human-readable provenance string.
    pub fn fetch(&mut self, title: &str, artist: &str) -> Option<(Vec<LyricsLine>, String)> {
        self.last_fetched_for = format!("{title}{artist}");

        if let Some(cached) = self.read_cache(title, artist) {
            return Some((cached, "cache".to_string()));
        }

        for source in &self.sources {
            match source.fetch(title, artist) {
                Ok(lines) if !lines.is_empty() => {
                    self.write_cache(title, artist, source.app_name(), &lines);
                    return Some((lines, source.app_name().to_string()));
                }
                _ => continue,
            }
        }

        None
    }
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
}
