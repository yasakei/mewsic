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
    fn fetch_timed(
        &self,
        title: &str,
        artist: &str,
        duration_ms: u64,
    ) -> Result<Vec<LyricsLine>, String> {
        let _ = duration_ms;
        self.fetch(title, artist)
    }
}

pub fn pick_duration_index(durations_ms: &[u64], want_ms: u64, tolerance_ms: u64) -> Option<usize> {
    let mut best: Option<(usize, u64)> = None;
    for (i, have) in durations_ms.iter().enumerate() {
        let diff = have.abs_diff(want_ms);
        if diff <= tolerance_ms && best.is_none_or(|(_, d)| diff < d) {
            best = Some((i, diff));
        }
    }
    best.map(|(i, _)| i)
}

pub fn normalize_title(title: &str) -> String {
    let out = strip_video_suffix(title);
    if out.is_empty() {
        title.trim().to_string()
    } else {
        out
    }
}

pub fn normalize_artist(artist: &str) -> String {
    let mut out = artist.trim().to_string();
    let lower = out.to_lowercase();
    if let Some(base) = lower.strip_suffix(" - topic") {
        out = artist.trim()[..base.len()].trim_end().to_string();
    } else if lower.ends_with("vevo") && out.len() > 4 {
        out = out[..out.len() - 4].trim_end().to_string();
    }
    if out.is_empty() {
        artist.trim().to_string()
    } else {
        out
    }
}

pub fn search_queries(title: &str, artist: &str) -> Vec<(String, String)> {
    let mut queries = vec![(normalize_title(title), normalize_artist(artist))];
    let raw_title = title.trim().to_string();
    let raw_artist = artist.trim().to_string();
    if let Some((left, right)) = raw_title.split_once(" - ") {
        let candidate = (normalize_title(right.trim()), normalize_artist(left.trim()));
        if !queries.contains(&candidate) {
            queries.push(candidate);
        }
    }
    if is_channel_artist(&raw_artist) {
        for t in [queries[0].0.clone(), raw_title.clone()] {
            let candidate = (t, String::new());
            if !queries.contains(&candidate) {
                queries.push(candidate);
            }
        }
    }
    let raw = (raw_title, raw_artist);
    if !queries.contains(&raw) {
        queries.push(raw);
    }
    queries
}

pub fn is_channel_artist(artist: &str) -> bool {
    let lower = artist.to_lowercase();
    if lower.ends_with("vevo") {
        return true;
    }
    [
        "labels",
        "label",
        "vevo",
        "records",
        "recordings",
        "music",
        "entertainment",
        "official",
        "tv",
        "topic",
        "channel",
        "studio",
    ]
    .iter()
    .any(|k| lower.split(|c: char| !c.is_alphanumeric()).any(|w| w == *k))
}

const VIDEO_KEYWORDS: &[&str] = &[
    "official",
    "mv",
    "m/v",
    "music video",
    "lyric video",
    "lyrics",
    "official audio",
    "audio",
    "visualizer",
    "visualiser",
    "teaser",
    "trailer",
];

const VIDEO_SUFFIXES: &[&str] = &[
    " - official video",
    " - official music video",
    " - official audio",
    " - audio",
    " - mv",
    " - m/v",
    " - lyrics",
    " - lyric video",
    " - visualizer",
    " - visualiser",
];

pub struct LrcLibSource;

impl Source for LrcLibSource {
    fn app_name(&self) -> String {
        "LrcLib".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        self.fetch_timed(title, artist, 0)
    }

    fn fetch_timed(
        &self,
        title: &str,
        artist: &str,
        duration_ms: u64,
    ) -> Result<Vec<LyricsLine>, String> {
        if duration_ms > 0 {
            let url = format!(
                "https://lrclib.net/api/get?track_name={}&artist_name={}&duration={}",
                urlencode(title),
                urlencode(artist),
                duration_ms / 1000
            );
            if let Ok(lines) = Self::get_synced(&url) {
                return Ok(lines);
            }
            if let Some(lines) = self.search_best(title, artist, duration_ms) {
                return Ok(lines);
            }
        }
        let url = format!(
            "https://lrclib.net/api/get?track_name={}&artist_name={}",
            urlencode(title),
            urlencode(artist)
        );
        Self::get_synced(&url)
    }
}

impl LrcLibSource {
    fn get_synced(url: &str) -> Result<Vec<LyricsLine>, String> {
        let resp = net::lyrics_agent()
            .get(url)
            .call()
            .map_err(|e| e.to_string())?;
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

    fn search_best(&self, title: &str, artist: &str, duration_ms: u64) -> Option<Vec<LyricsLine>> {
        let url = format!(
            "https://lrclib.net/api/search?track_name={}&artist_name={}",
            urlencode(title),
            urlencode(artist)
        );
        let resp = net::lyrics_agent().get(&url).call().ok()?;
        if resp.status() != 200 {
            return None;
        }
        let json: serde_json::Value = resp.into_json().ok()?;
        let tracks = json.as_array()?;
        let mut candidates: Vec<(u64, i64)> = Vec::new();
        for track in tracks {
            if track
                .get("instrumental")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            let synced = track
                .get("syncedLyrics")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if synced.trim().is_empty() {
                continue;
            }
            let duration_ms = track
                .get("duration")
                .and_then(|v| v.as_f64())
                .map(|d| (d * 1000.0) as u64)
                .unwrap_or(0);
            if duration_ms == 0 {
                continue;
            }
            candidates.push((
                duration_ms,
                track.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
            ));
        }
        if candidates.is_empty() {
            return None;
        }
        let durations: Vec<u64> = candidates.iter().map(|(d, _)| *d).collect();
        let winner = pick_duration_index(&durations, duration_ms, 30_000)?;
        let id = candidates[winner].1;
        if id == 0 {
            return None;
        }
        Self::get_synced(&format!("https://lrclib.net/api/get/{id}")).ok()
    }
}

pub struct NetEaseSource;

impl Source for NetEaseSource {
    fn app_name(&self) -> String {
        "NetEase Music".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        self.fetch_timed(title, artist, 0)
    }

    fn fetch_timed(
        &self,
        title: &str,
        artist: &str,
        duration_ms: u64,
    ) -> Result<Vec<LyricsLine>, String> {
        let song_id = self.song_id(title, artist, duration_ms)?;

        let url =
            format!("https://music.163.com/api/song/lyric?tv=-1&kv=-1&lv=-1&os=pc&id={song_id}");
        let resp = self.post(&url).map_err(|e| e.to_string())?;
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
        net::lyrics_agent()
            .post(url)
            .set("Referer", "https://music.163.com")
            .set("Cookie", "appver=2.0.2")
            .set("X-Real-IP", "202.96.0.0")
            .call()
            .map_err(Box::new)
    }

    fn song_id(&self, title: &str, artist: &str, duration_ms: u64) -> Result<i64, String> {
        let url = format!(
            "https://music.163.com/api/search/get?s={}&type=1&offset=0&sub=false&limit=10",
            urlencode(&format!("{title}-{artist}"))
        );
        let resp = self.post(&url).map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;

        let songs = json
            .pointer("/result/songs")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "song not found".to_string())?;
        if songs.is_empty() {
            return Err("song not found".into());
        }
        if duration_ms > 0 {
            let durations: Vec<u64> = songs
                .iter()
                .map(|s| s.get("duration").and_then(|v| v.as_u64()).unwrap_or(0))
                .collect();
            if durations.iter().any(|d| *d > 0) {
                if let Some(i) = pick_duration_index(&durations, duration_ms, 30_000) {
                    return songs[i]
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .ok_or_else(|| "no song id in search".into());
                }
            }
        }
        songs[0]
            .get("id")
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
        let resp = net::lyrics_agent()
            .get(&url)
            .set("Referer", "http://y.qq.com/portal/player.html")
            .call()
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;

        let b64 = json.get("lyric").and_then(|v| v.as_str()).unwrap_or("");
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
        let resp = net::lyrics_agent()
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

pub type CaptionTrack = (String, bool);
pub type CaptionCandidate = (String, u64, Vec<CaptionTrack>);

pub struct YouTubeSource;

impl Source for YouTubeSource {
    fn app_name(&self) -> String {
        "YouTube captions".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        self.fetch_timed(title, artist, 0)
    }

    fn fetch_timed(
        &self,
        title: &str,
        artist: &str,
        duration_ms: u64,
    ) -> Result<Vec<LyricsLine>, String> {
        let query = format!("{title} {artist}");
        let raw = innertube_search(&query)?;
        let mut ids = Vec::new();
        collect_video_ids(
            &serde_json::from_str(&raw).map_err(|e| e.to_string())?,
            &mut ids,
            0,
        );
        if ids.is_empty() {
            return Err("no video found".into());
        }
        let mut infos: Vec<CaptionCandidate> = Vec::new();
        for id in ids.iter().take(MAX_CANDIDATE_VIDEOS) {
            if let Some((length_ms, tracks)) = player_captions(id) {
                if !tracks.is_empty() {
                    infos.push((id.clone(), length_ms, tracks));
                }
            }
        }
        if infos.is_empty() {
            return Err("no captioned video found".into());
        }
        let pick = pick_captioned_video(&infos, duration_ms).ok_or("no matching video")?;
        let (_, _, tracks) = &infos[pick];
        Self::captions_for_tracks(tracks)
    }
}

impl YouTubeSource {
    pub fn fetch_by_video(video_id: &str) -> Result<Vec<LyricsLine>, String> {
        let meta = video_meta(video_id).ok_or("video unavailable")?;
        if meta.tracks.is_empty() {
            return Err("video has no captions".into());
        }
        Self::captions_for_tracks(&meta.tracks)
    }

    pub fn video_title_artist(video_id: &str) -> Option<(String, String)> {
        let meta = video_meta(video_id)?;
        let (song, artist) = split_youtube_title(&meta.title)?;
        let artist = if artist.is_empty() {
            meta.channel
        } else {
            artist
        };
        Some((song, artist))
    }

    fn captions_for_tracks(tracks: &[(String, bool)]) -> Result<Vec<LyricsLine>, String> {
        let manual = tracks.iter().find(|(_, asr)| !asr);
        let url = manual
            .or_else(|| tracks.first())
            .map(|(u, _)| u.clone())
            .ok_or("no caption track")?;
        let xml = net::lyrics_agent()
            .get(&url)
            .set("User-Agent", YT_ANDROID_UA)
            .call()
            .map_err(|e| e.to_string())?;
        if xml.status() != 200 {
            return Err(format!("caption track responded {}", xml.status()));
        }
        let body = xml.into_string().map_err(|e| e.to_string())?;
        let lines = parse_timedtext(&body);
        if lines.is_empty() {
            return Err("caption track empty".into());
        }
        Ok(lines)
    }
}

const YT_ANDROID_UA: &str = "com.google.android.youtube/20.10.38 (Linux; U; Android 14) gzip";
const MAX_CANDIDATE_VIDEOS: usize = 5;

pub fn split_youtube_title(raw: &str) -> Option<(String, String)> {
    let work = strip_video_suffix(raw);
    if work.is_empty() {
        return None;
    }

    let quoted = [
        ('\u{2018}', '\u{2019}'),
        ('\u{201C}', '\u{201D}'),
        ('\u{300C}', '\u{300D}'),
        ('\u{300E}', '\u{300F}'),
        ('\u{00AB}', '\u{00BB}'),
        ('\u{201A}', '\u{2019}'),
        ('\u{2039}', '\u{203A}'),
    ];
    for (open, close) in quoted {
        if let Some(start) = work.find(open) {
            let inner: String = work[start + open.len_utf8()..]
                .chars()
                .take_while(|c| *c != close)
                .collect();
            let inner = inner.trim();
            if inner.is_empty() {
                continue;
            }
            let mut artist = work[..start].to_string();
            artist = strip_trailing_separator(&artist);
            artist = strip_native_name(&artist);
            let artist = artist.trim().to_string();
            return Some((inner.to_string(), artist));
        }
    }

    for quote in ['\'', '"'] {
        if let Some((inner, artist)) = ascii_quoted(&work, quote) {
            return Some((inner, artist));
        }
    }

    if let Some((left, right)) = work.split_once(" - ") {
        let left = strip_native_name(left);
        let right = right.trim();
        if !left.trim().is_empty() && !right.is_empty() {
            return Some((right.to_string(), left.trim().to_string()));
        }
    }

    let first = work.split(['/', '、']).next().unwrap_or("").trim();
    if first.len() >= 2 && first.len() < work.len() {
        let first = first.trim_matches(|c: char| c == '♪' || c == '【' || c == '】');
        if !first.is_empty() {
            return Some((first.to_string(), String::new()));
        }
    }

    None
}

pub fn has_synced_lyrics(song: &str, artist: &str, duration_ms: u64) -> bool {
    let song = song.trim();
    if song.is_empty() {
        return false;
    }
    for (t, a) in search_queries(song, artist).iter().take(2) {
        let found = LrcLibSource
            .fetch_timed(t, a, duration_ms)
            .map(|l| !l.is_empty())
            .unwrap_or(false)
            || NetEaseSource
                .fetch_timed(t, a, duration_ms)
                .map(|l| !l.is_empty())
                .unwrap_or(false)
            || QqMusicSource
                .fetch_timed(t, a, duration_ms)
                .map(|l| !l.is_empty())
                .unwrap_or(false);
        if found {
            return true;
        }
    }
    false
}

pub fn normalize_match(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub fn songs_match(a: &str, b: &str) -> bool {
    let na = normalize_match(a);
    let nb = normalize_match(b);
    if na.is_empty() || nb.is_empty() {
        return true;
    }
    if na == nb || na.contains(&nb) || nb.contains(&na) {
        return true;
    }
    let tokens: Vec<String> = na
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(String::from)
        .collect();
    let other: Vec<String> = nb
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(String::from)
        .collect();
    if tokens.is_empty() || other.is_empty() {
        return true;
    }
    tokens.iter().any(|t| other.iter().any(|o| o == t))
}

/// A player may report the raw video title, a bare channel name, or a clean
/// song name. Only reject the video when both sides yield a song that clearly
/// disagrees; unknown metadata is allowed rather than silently dropping a
/// legitimate match.
pub fn video_matches_song(player_title: &str, player_artist: &str, video_song: &str) -> bool {
    if let Some((song, _)) = split_youtube_title(player_title) {
        return songs_match(&song, video_song);
    }
    let title = player_title.trim();
    if title.is_empty() {
        return true;
    }
    if is_channel_artist(player_artist) && !is_channel_artist(title) {
        return songs_match(title, video_song);
    }
    if is_channel_artist(title) {
        return true;
    }
    songs_match(title, video_song)
}

fn ascii_quoted(work: &str, quote: char) -> Option<(String, String)> {
    let bytes: Vec<char> = work.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != quote {
            i += 1;
            continue;
        }
        let preceded_ok = i == 0 || {
            let prev = bytes[i - 1];
            prev.is_whitespace() || "-([".contains(prev)
        };
        if !preceded_ok {
            i += 1;
            continue;
        }
        if let Some(offset) = (i + 1..bytes.len()).find(|&j| bytes[j] == quote) {
            let inner: String = bytes[i + 1..offset].iter().collect();
            let inner = inner.trim();
            if inner.is_empty() {
                i += 1;
                continue;
            }
            let artist: String = bytes[..i].iter().collect();
            let artist = strip_trailing_separator(&artist);
            let artist = strip_native_name(&artist);
            return Some((inner.to_string(), artist.trim().to_string()));
        }
        break;
    }
    None
}

fn strip_native_name(artist: &str) -> String {
    let trimmed = artist.trim_end();
    if !trimmed.ends_with(')') {
        return trimmed.to_string();
    }
    match trimmed.rfind('(') {
        Some(i) if i > 0 => trimmed[..i].trim_end().to_string(),
        _ => trimmed.to_string(),
    }
}

fn strip_trailing_separator(artist: &str) -> String {
    artist
        .trim_end()
        .trim_end_matches(['-', '\u{2013}', '\u{2014}', '|', ',', ':', '\u{2018}'])
        .trim_end()
        .to_string()
}

fn strip_video_suffix(raw: &str) -> String {
    let mut out = raw.trim().to_string();
    loop {
        let lower = out.to_lowercase();
        let cut = if lower.ends_with(']') {
            out.rfind('[').filter(|&i| {
                VIDEO_KEYWORDS
                    .iter()
                    .any(|k| lower[i + 1..lower.len() - 1].contains(k))
            })
        } else if lower.ends_with(')') {
            out.rfind('(').filter(|&i| {
                VIDEO_KEYWORDS
                    .iter()
                    .any(|k| lower[i + 1..lower.len() - 1].contains(k))
            })
        } else {
            VIDEO_SUFFIXES
                .iter()
                .find(|s| lower.ends_with(*s))
                .map(|s| lower.len() - s.len())
        };
        match cut {
            Some(i) => out = out[..i].trim_end().to_string(),
            None => break,
        }
    }
    out
}

#[cfg(any(test, target_os = "linux"))]
pub fn youtube_id_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let lower = url.to_lowercase();
    if !lower.contains("youtube.com") && !lower.contains("youtu.be") {
        return None;
    }
    if let Some(pos) = lower.find("v=") {
        let id: String = url[pos + 2..].chars().take(11).collect();
        if valid_video_id(&id) {
            return Some(id);
        }
    }
    for marker in [
        "youtu.be/",
        "youtube.com/shorts/",
        "youtube.com/embed/",
        "youtube.com/live/",
    ] {
        if let Some(pos) = lower.find(marker) {
            let id: String = url[pos + marker.len()..].chars().take(11).collect();
            if valid_video_id(&id) {
                return Some(id);
            }
        }
    }
    None
}

fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn collect_video_ids(json: &serde_json::Value, out: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    match json {
        serde_json::Value::Object(map) => {
            for key in ["videoRenderer", "compactVideoRenderer"] {
                if let Some(id) = map
                    .get(key)
                    .and_then(|r| r.get("videoId"))
                    .and_then(|v| v.as_str())
                {
                    if valid_video_id(id) && !out.iter().any(|existing| existing == id) {
                        out.push(id.to_string());
                    }
                }
            }
            for value in map.values() {
                collect_video_ids(value, out, depth + 1);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_video_ids(item, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn innertube_search(query: &str) -> Result<String, String> {
    let url = "https://www.youtube.com/youtubei/v1/search?contentCheckOk=True";
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "ANDROID",
                "clientVersion": "20.10.38",
                "androidSdkVersion": 30,
                "hl": "en",
                "gl": "US",
            }
        },
        "query": query,
    })
    .to_string();
    let resp = net::lyrics_agent()
        .post(url)
        .set("User-Agent", YT_ANDROID_UA)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .map_err(|e| e.to_string())?;
    if resp.status() != 200 {
        return Err(format!("search responded {}", resp.status()));
    }
    resp.into_string().map_err(|e| e.to_string())
}

pub fn video_rank(has_manual: bool, duration_diff: u64) -> (bool, u64) {
    (!has_manual, duration_diff)
}

pub fn pick_captioned_video(candidates: &[CaptionCandidate], want_ms: u64) -> Option<usize> {
    let mut best: Option<((bool, u64), usize)> = None;
    for (i, (_, len, tracks)) in candidates.iter().enumerate() {
        if tracks.is_empty() {
            continue;
        }
        let has_manual = tracks.iter().any(|(_, asr)| !asr);
        let diff = if want_ms > 0 {
            len.abs_diff(want_ms)
        } else {
            0
        };
        if want_ms > 0 && diff > 30_000 {
            continue;
        }
        let key = video_rank(has_manual, diff);
        if best.as_ref().is_none_or(|(b, _)| key < *b) {
            best = Some((key, i));
        }
    }
    best.map(|(_, i)| i)
}

pub fn parse_timedtext(xml: &str) -> Vec<LyricsLine> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(p) = rest.find("<p ") {
        let tag_end = match rest[p..].find('>') {
            Some(e) => p + e,
            None => break,
        };
        let tag = &rest[p..tag_end];
        let time = attr_ms(tag, "t=\"").unwrap_or(0);
        let close = match rest[tag_end..].find("</p>") {
            Some(e) => tag_end + e,
            None => break,
        };
        let raw = &rest[tag_end + 1..close];
        let mut text = String::with_capacity(raw.len());
        let mut in_tag = false;
        for c in raw.chars() {
            if c == '<' {
                in_tag = true;
                continue;
            }
            if c == '>' {
                in_tag = false;
                continue;
            }
            if !in_tag {
                text.push(c);
            }
        }
        let text = crate::util::decode_html_entities(&text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace("♪", "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !text
            .trim_matches(|c| c == '[' || c == ']' || c == ' ')
            .is_empty()
        {
            out.push(LyricsLine { time, text });
        }
        rest = &rest[close + 4..];
    }
    out.sort_by_key(|l| l.time);
    out
}

fn attr_ms(tag: &str, key: &str) -> Option<u64> {
    let pos = tag.find(key)?;
    let start = pos + key.len();
    let end = tag[start..].find('"')?;
    tag[start..start + end].parse::<u64>().ok()
}

pub struct YouTubeVideo {
    pub length_ms: u64,
    pub tracks: Vec<(String, bool)>,
    pub title: String,
    pub channel: String,
}

pub fn video_meta(video_id: &str) -> Option<YouTubeVideo> {
    let url = "https://www.youtube.com/youtubei/v1/player?contentCheckOk=True&racyCheckOk=True";
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "ANDROID",
                "clientVersion": "20.10.38",
                "androidSdkVersion": 30,
                "hl": "en",
                "gl": "US",
            }
        },
        "videoId": video_id,
    })
    .to_string();
    let resp = net::lyrics_agent()
        .post(url)
        .set("User-Agent", YT_ANDROID_UA)
        .set("Content-Type", "application/json")
        .send_string(&body)
        .ok()?;
    if resp.status() != 200 {
        return None;
    }
    let json: serde_json::Value = resp.into_json().ok()?;
    let length_ms = json
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        * 1000;
    let title = json
        .pointer("/videoDetails/title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let channel = json
        .pointer("/videoDetails/author")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut tracks_out = Vec::new();
    if let Some(tracks) = json
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(|v| v.as_array())
    {
        for track in tracks {
            let base = track.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
            if base.is_empty() {
                continue;
            }
            let asr = track.get("kind").and_then(|v| v.as_str()) == Some("asr");
            tracks_out.push((base.to_string(), asr));
        }
    }
    Some(YouTubeVideo {
        length_ms,
        tracks: tracks_out,
        title,
        channel,
    })
}

fn player_captions(video_id: &str) -> Option<(u64, Vec<(String, bool)>)> {
    let meta = video_meta(video_id)?;
    Some((meta.length_ms, meta.tracks))
}

pub struct LastFmSource {
    pub api_key: String,
}

impl Source for LastFmSource {
    fn app_name(&self) -> String {
        "Last.fm".to_string()
    }

    fn fetch(&self, title: &str, artist: &str) -> Result<Vec<LyricsLine>, String> {
        match crate::lastfm::fetch_lyrics(&self.api_key, artist, title)? {
            Some(lines) => Ok(lines),
            None => Err("no synced lyrics".into()),
        }
    }
}

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
        let req = net::lyrics_agent().get(&url);
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

    #[serde(default)]
    romanized: Vec<LyricsLine>,
}

pub struct LyricsFetcher {
    cache_dir: PathBuf,
    last_fetched_for: String,
    last_provider_sig: String,

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

    pub fn providers_changed(&self, lyrics: &crate::config::LyricsSettings) -> bool {
        self.last_fetched_for.is_empty() || self.last_provider_sig != provider_sig(lyrics)
    }

    pub fn romanize_changed(&self, lyrics: &crate::config::LyricsSettings) -> bool {
        self.last_romanize
            .is_some_and(|prev| prev != lyrics.romanize)
    }

    fn cache_path(&self, title: &str, artist: &str) -> PathBuf {
        self.cache_dir.join(format!(
            "{}-{}.json",
            sanitize_filename(title),
            sanitize_filename(artist)
        ))
    }

    pub fn read_cache(
        &mut self,
        title: &str,
        artist: &str,
        romanize: bool,
    ) -> Option<Vec<LyricsLine>> {
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

    fn video_cache_path(&self, video_id: &str) -> PathBuf {
        self.cache_dir
            .join(format!("youtube-{}.json", sanitize_filename(video_id)))
    }

    fn video_meta_cache_path(&self, video_id: &str) -> PathBuf {
        self.cache_dir
            .join(format!("youtube-{}.meta.json", sanitize_filename(video_id)))
    }

    pub fn video_title_artist(&self, video_id: &str) -> Option<(String, String)> {
        if let Ok(raw) = fs::read_to_string(self.video_meta_cache_path(video_id)) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let (Some(song), Some(artist)) = (
                    value.get("song").and_then(|v| v.as_str()),
                    value.get("artist").and_then(|v| v.as_str()),
                ) {
                    return Some((song.to_string(), artist.to_string()));
                }
            }
        }
        let (song, artist) = YouTubeSource::video_title_artist(video_id)?;
        let _ = fs::create_dir_all(&self.cache_dir);
        let value = serde_json::json!({ "song": song, "artist": artist });
        let _ = fs::write(
            self.video_meta_cache_path(video_id),
            serde_json::to_string(&value).unwrap_or_default(),
        );
        Some((song, artist))
    }

    pub fn fetch_youtube_exact(
        &mut self,
        video_id: &str,
        romanize: bool,
        player_title: &str,
        player_artist: &str,
    ) -> Option<(Vec<LyricsLine>, String)> {
        if let Some((song, _)) = YouTubeSource::video_title_artist(video_id) {
            if !video_matches_song(player_title, player_artist, &song) {
                return None;
            }
        }
        self.last_romanize = Some(romanize);
        if let Ok(raw) = fs::read_to_string(self.video_cache_path(video_id)) {
            if let Ok(mut parsed) = serde_json::from_str::<CachedLyrics>(&raw) {
                if !parsed.lines.is_empty() {
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
                            let _ = fs::write(
                                self.video_cache_path(video_id),
                                serde_json::to_string(&parsed).unwrap_or_default(),
                            );
                        }
                        return Some((parsed.romanized, "cache".to_string()));
                    }
                    return Some((parsed.lines, "cache".to_string()));
                }
            }
        }
        match YouTubeSource::fetch_by_video(video_id) {
            Ok(lines) if !lines.is_empty() => {
                let _ = fs::create_dir_all(&self.cache_dir);
                let source = YouTubeSource.app_name();
                let mut cached = CachedLyrics {
                    source: source.clone(),
                    lines: lines.clone(),
                    romanized: Vec::new(),
                };
                if romanize {
                    cached.romanized = cached
                        .lines
                        .iter()
                        .map(|l| LyricsLine {
                            time: l.time,
                            text: crate::romanize::romanize(&l.text),
                        })
                        .collect();
                }
                let out = if romanize {
                    cached.romanized.clone()
                } else {
                    cached.lines.clone()
                };
                let _ = fs::write(
                    self.video_cache_path(video_id),
                    serde_json::to_string(&cached).unwrap_or_default(),
                );
                self.last_romanize = Some(romanize);
                Some((out, source))
            }
            _ => None,
        }
    }

    fn enabled_sources(
        &self,
        lyrics: &crate::config::LyricsSettings,
        lastfm_key: &str,
    ) -> Vec<Box<dyn Source>> {
        let mut out: Vec<Box<dyn Source>> = Vec::new();
        for id in &lyrics.providers {
            match id.as_str() {
                "lrclib" => out.push(Box::new(LrcLibSource)),
                "netease" => out.push(Box::new(NetEaseSource)),
                "qqmusic" => out.push(Box::new(QqMusicSource)),
                "youtube" => out.push(Box::new(YouTubeSource)),
                "lastfm" => {
                    if !lastfm_key.trim().is_empty() {
                        out.push(Box::new(LastFmSource {
                            api_key: lastfm_key.to_string(),
                        }));
                    }
                }
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
        duration_ms: u64,
        lyrics: &crate::config::LyricsSettings,
        lastfm_key: &str,
    ) -> Option<(Vec<LyricsLine>, String)> {
        self.last_fetched_for = format!("{title}{artist}");
        self.last_provider_sig = provider_sig(lyrics);

        let queries = search_queries(title, artist);
        for (q_title, q_artist) in &queries {
            if let Some(cached) = self.read_cache(q_title, q_artist, lyrics.romanize) {
                return Some((cached, "cache".to_string()));
            }
        }

        let (canonical_title, canonical_artist) = queries[0].clone();
        let sources = self.enabled_sources(lyrics, lastfm_key);
        for (q_title, q_artist) in &queries {
            for source in &sources {
                match source.fetch_timed(q_title, q_artist, duration_ms) {
                    Ok(lines) if !lines.is_empty() => {
                        self.write_cache(
                            &canonical_title,
                            &canonical_artist,
                            &source.app_name(),
                            &lines,
                        );
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
                                &canonical_title,
                                &canonical_artist,
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
        }

        None
    }
}

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
    fn mv_titles_normalize_to_base() {
        assert_eq!(
            normalize_title("Blinding Lights (Official Video)"),
            "Blinding Lights"
        );
        assert_eq!(normalize_title("Dynamite (Official MV)"), "Dynamite");
        assert_eq!(normalize_title("Butter [M/V]"), "Butter");
        assert_eq!(
            normalize_title("How You Like That (M/V)"),
            "How You Like That"
        );
        assert_eq!(normalize_title("Song (Lyric Video)"), "Song");
        assert_eq!(normalize_title("Song (Official Audio)"), "Song");
        assert_eq!(normalize_title("Song (Audio)"), "Song");
        assert_eq!(
            normalize_title("Cruel Summer (Official Video)"),
            "Cruel Summer"
        );
    }

    #[test]
    fn version_markers_survive() {
        assert_eq!(
            normalize_title("Drowning (Avicii Remix)"),
            "Drowning (Avicii Remix)"
        );
        assert_eq!(normalize_title("Song - Live"), "Song - Live");
        assert_eq!(
            normalize_title("Cruel Summer (Taylor's Version)"),
            "Cruel Summer (Taylor's Version)"
        );
        assert_eq!(
            normalize_title("Song (feat. Someone)"),
            "Song (feat. Someone)"
        );
        assert_eq!(
            normalize_title("Song (Remastered 2021)"),
            "Song (Remastered 2021)"
        );
    }

    #[test]
    fn real_brackets_survive() {
        assert_eq!(normalize_title("Song (Pt. 2)"), "Song (Pt. 2)");
        assert_eq!(normalize_title("Alive"), "Alive");
        assert_eq!(normalize_title("APT."), "APT.");
        assert_eq!(normalize_title("夜に駆ける"), "夜に駆ける");
    }

    #[test]
    fn topic_and_vevo_artists_normalize() {
        assert_eq!(normalize_artist("The Weeknd - Topic"), "The Weeknd");
        assert_eq!(normalize_artist("BTS - Topic"), "BTS");
        assert_eq!(normalize_artist("TaylorSwiftVEVO"), "TaylorSwift");
        assert_eq!(normalize_artist("YOASOBI"), "YOASOBI");
        assert_eq!(normalize_artist("ROSÉ & Bruno Mars"), "ROSÉ & Bruno Mars");
    }

    #[test]
    fn channel_artist_adds_title_only_queries() {
        assert!(is_channel_artist("HYBE LABELS"));
        assert!(is_channel_artist("TaylorSwiftVEVO"));
        assert!(is_channel_artist("Artist - Topic"));
        assert!(!is_channel_artist("The Weeknd"));
        assert!(!is_channel_artist("YOASOBI"));
        let q = search_queries("ILLIT", "HYBE LABELS");
        assert!(q.contains(&("ILLIT".to_string(), String::new())));
    }

    #[test]
    fn dash_title_splits_artist_first() {
        let q = search_queries("ILLIT - Magnetic", "ILLIT");
        assert!(q.contains(&("Magnetic".to_string(), "ILLIT".to_string())));
    }

    #[test]
    fn queries_try_normalized_first() {
        let q = search_queries("Dynamite (Official MV)", "BTS - Topic");
        assert_eq!(q[0], ("Dynamite".to_string(), "BTS".to_string()));
        assert!(q.contains(&(
            "Dynamite (Official MV)".to_string(),
            "BTS - Topic".to_string()
        )));
        let clean = search_queries("APT.", "ROSÉ");
        assert_eq!(clean.len(), 1);
    }

    #[test]
    fn duration_picks_closest_within_tolerance() {
        let durs = vec![210_000, 243_000, 180_000];
        assert_eq!(pick_duration_index(&durs, 242_000, 30_000), Some(1));
        assert_eq!(pick_duration_index(&durs, 300_000, 30_000), None);
        assert_eq!(pick_duration_index(&[], 200_000, 30_000), None);
        assert_eq!(pick_duration_index(&durs, 180_500, 30_000), Some(2));
    }

    #[test]
    fn lrclib_float_seconds_parse_to_ms() {
        let track: serde_json::Value =
            serde_json::from_str(r#"{"duration": 199.0, "id": 1}"#).unwrap();
        let ms = track
            .get("duration")
            .and_then(|v| v.as_f64())
            .map(|d| (d * 1000.0) as u64)
            .unwrap_or(0);
        assert_eq!(ms, 199_000);
        let int_track: serde_json::Value =
            serde_json::from_str(r#"{"duration": 199, "id": 2}"#).unwrap();
        let ms = int_track
            .get("duration")
            .and_then(|v| v.as_f64())
            .map(|d| (d * 1000.0) as u64)
            .unwrap_or(0);
        assert_eq!(ms, 199_000);
    }

    #[test]
    fn timedtext_xml_becomes_lines() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?><timedtext format="3"><body><p t="1360" d="1680">[♪♪♪]</p><p t="18640" d="3240">♪ We&#39;re no strangers to love ♪</p><p t="22640" d="4320">You know the rules
and so do I</p></body></timedtext>"#;
        let lines = parse_timedtext(xml);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time, 18_640);
        assert_eq!(lines[0].text, "We're no strangers to love");
        assert_eq!(lines[1].text, "You know the rules and so do I");
    }

    #[test]
    fn search_json_yields_deduped_video_ids() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"contents":[
                {"compactVideoRenderer":{"videoId":"gdZLi9oWNZg"}},
                {"videoRenderer":{"videoId":"gdZLi9oWNZg"}},
                {"videoRenderer":{"videoId":"short"}},
                {"videoRenderer":{"videoId":"abc123_-XYZ"}}
            ]}"#,
        )
        .unwrap();
        let mut ids = Vec::new();
        collect_video_ids(&json, &mut ids, 0);
        assert_eq!(
            ids,
            vec!["gdZLi9oWNZg".to_string(), "abc123_-XYZ".to_string()]
        );
        let mut empty = Vec::new();
        collect_video_ids(&serde_json::json!({"nothing": true}), &mut empty, 0);
        assert!(empty.is_empty());
    }

    #[test]
    fn unrelated_video_is_rejected() {
        let raw = "ILLIT (아일릿) ‘Magnetic’ Official MV";
        assert!(video_matches_song(raw, "HYBE LABELS", "Magnetic"));
        assert!(video_matches_song("Magnetic", "ILLIT", "Magnetic"));
        assert!(video_matches_song("", "", "Magnetic"));
        assert!(
            !video_matches_song(raw, "HYBE LABELS", "Dynamite"),
            "a different song's video must not be used"
        );
        assert!(
            !video_matches_song("Dynamite", "BTS", "Magnetic"),
            "player reports another song"
        );
        assert!(
            video_matches_song("HYBE LABELS", "HYBE LABELS", "Magnetic"),
            "channel-only metadata must not block a real match"
        );
    }

    #[test]
    fn songs_match_ignores_punctuation_and_case() {
        assert!(songs_match("Magnetic", "magnetic"));
        assert!(songs_match("Song (Official MV)", "Song"));
        assert!(songs_match("APT.", "APT"));
        assert!(!songs_match("Magnetic", "Dynamite"));
        assert!(songs_match("", "anything"));
    }

    #[test]
    fn youtube_title_splits_artist_and_song() {
        assert_eq!(
            split_youtube_title("ILLIT (아일린) ‘Magnetic’ Official MV"),
            Some(("Magnetic".to_string(), "ILLIT".to_string()))
        );
        assert_eq!(
            split_youtube_title("NewJeans (노진스) 'OMG' Official MV"),
            Some(("OMG".to_string(), "NewJeans".to_string()))
        );
        assert_eq!(
            split_youtube_title("Stray Kids (스트레이 키지) - “아무노랜” Official MV"),
            Some(("아무노랜".to_string(), "Stray Kids".to_string()))
        );
        assert_eq!(
            split_youtube_title("aespa 'Drama' Official MV"),
            Some(("Drama".to_string(), "aespa".to_string()))
        );
        assert_eq!(
            split_youtube_title("BLACKPINK - 'Stay' Official Video"),
            Some(("Stay".to_string(), "BLACKPINK".to_string()))
        );
        assert_eq!(
            split_youtube_title("Adele - Hello (Official Video)"),
            Some(("Hello".to_string(), "Adele".to_string()))
        );
        assert_eq!(split_youtube_title("Official MV"), None);
    }

    #[test]
    fn mpris_url_yields_video_id() {
        assert_eq!(
            youtube_id_from_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            youtube_id_from_url("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            youtube_id_from_url("https://www.youtube.com/shorts/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            youtube_id_from_url("https://open.spotify.com/track/abc"),
            None
        );
        assert_eq!(youtube_id_from_url(""), None);
    }

    #[test]
    fn captioned_pick_prefers_manual_then_duration() {
        let no_tracks: Vec<(String, bool)> = Vec::new();
        let cands = vec![
            ("a".to_string(), 225_000, vec![("u".to_string(), false)]),
            ("b".to_string(), 243_000, vec![("u".to_string(), true)]),
            ("c".to_string(), 0, no_tracks),
        ];
        assert_eq!(pick_captioned_video(&cands, 242_000), Some(0));
        assert_eq!(pick_captioned_video(&cands, 400_000), None);
        assert_eq!(pick_captioned_video(&cands, 0), Some(0));
        assert_eq!(pick_captioned_video(&[], 200_000), None);

        let only_asr = vec![
            ("x".to_string(), 100_000, vec![("u".to_string(), true)]),
            ("y".to_string(), 300_000, vec![("u".to_string(), true)]),
        ];
        assert_eq!(pick_captioned_video(&only_asr, 299_000), Some(1));
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
        let sources = fetcher.enabled_sources(&lyrics, "");
        assert_eq!(sources.len(), 1);

        let empty = LyricsSettings {
            providers: vec![],
            ..LyricsSettings::default()
        };
        assert!(fetcher.enabled_sources(&empty, "").is_empty());
    }

    #[test]
    fn lastfm_provider_is_skipped_without_an_api_key() {
        let fetcher = LyricsFetcher::new(std::path::Path::new("/nonexistent"));
        let lyrics = crate::config::LyricsSettings {
            providers: vec!["lastfm".into()],
            ..crate::config::LyricsSettings::default()
        };
        assert!(fetcher.enabled_sources(&lyrics, "").is_empty());
        assert!(fetcher.enabled_sources(&lyrics, "   ").is_empty());
        assert_eq!(fetcher.enabled_sources(&lyrics, "key").len(), 1);
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
        assert!(fetcher.enabled_sources(&empty_custom, "").is_empty());

        let filled_custom = LyricsSettings {
            custom: Some(CustomProvider {
                name: "My".into(),
                url: "https://example.com/{title}".into(),
                api_key: None,
                json_path: None,
            }),
            ..empty_custom
        };
        assert_eq!(fetcher.enabled_sources(&filled_custom, "").len(), 1);
    }

    #[test]
    fn providers_changed_detects_provider_edits() {
        use crate::config::LyricsSettings;
        let mut fetcher = LyricsFetcher::new(std::path::Path::new("/nonexistent"));

        assert!(fetcher.providers_changed(&LyricsSettings::default()));

        fetcher.last_fetched_for = "songartist".to_string();

        let mut lyrics = LyricsSettings {
            providers: vec!["lrclib".into(), "netease".into()],
            romanize: false,
            custom: None,
        };
        fetcher.last_provider_sig = provider_sig(&lyrics);

        assert!(!fetcher.providers_changed(&lyrics));

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
        let dir =
            std::env::temp_dir().join(format!("mewsic-lyrics-cache-{tag}-{}", std::process::id()));
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

        let lines = fetcher.read_cache("Song", "Artist", true).unwrap();
        assert_eq!(lines[0].text, "kyouha");
        let raw = std::fs::read_to_string(dir.join("cache/Song-Artist.json")).unwrap();
        assert!(raw.contains("kyouha"), "romanized copy must be persisted");
        assert!(raw.contains("今日は"), "original must be kept");

        let lines = fetcher.read_cache("Song", "Artist", false).unwrap();
        assert_eq!(lines[0].text, "今日は");

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
