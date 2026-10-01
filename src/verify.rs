use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::net;
use crate::state::AppContext;

const POSITIVE_TTL_HOURS: u64 = 24 * 30;
const NEGATIVE_TTL_HOURS: u64 = 24 * 7;

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    ok: bool,
    at: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    hits: HashMap<String, Entry>,
}

fn now_hours() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 3600)
        .unwrap_or(0)
}

fn cache_path(ctx: &AppContext) -> std::path::PathBuf {
    ctx.config_dir.join("music-cache.json")
}

fn memory_cache() -> &'static std::sync::Mutex<Cache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(Cache::default()))
}

fn load(ctx: &AppContext) -> Cache {
    std::fs::read_to_string(cache_path(ctx))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn store(ctx: &AppContext, cache: &Cache) {
    let _ = std::fs::create_dir_all(&ctx.config_dir);
    if let Ok(raw) = serde_json::to_string(cache) {
        let _ = std::fs::write(cache_path(ctx), raw);
    }
}

fn key(song: &str, artist: &str) -> String {
    format!(
        "{}|{}",
        crate::lyrics::normalize_match(song),
        crate::lyrics::normalize_match(artist)
    )
}

fn fresh(entry: &Entry, now: u64) -> bool {
    let ttl = if entry.ok {
        POSITIVE_TTL_HOURS
    } else {
        NEGATIVE_TTL_HOURS
    };
    now.saturating_sub(entry.at) < ttl
}

fn reachable() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| {
        net::lyrics_agent()
            .get("https://lrclib.net/api/get?track_name=ping&artist_name=ping")
            .call()
            .map(|r| r.status() < 500)
            .unwrap_or(false)
    })
}

pub fn looks_like_music(ctx: &AppContext, song: &str, artist: &str, duration_ms: u64) -> bool {
    let song = song.trim();
    if song.is_empty() {
        return false;
    }
    if !reachable() {
        return true;
    }
    let k = key(song, artist);
    let now = now_hours();

    {
        let mem = memory_cache().lock().unwrap();
        if let Some(entry) = mem.hits.get(&k) {
            if fresh(entry, now) {
                return entry.ok;
            }
        }
    }

    let mut cache = load(ctx);
    if let Some(entry) = cache.hits.get(&k) {
        if fresh(entry, now) {
            let ok = entry.ok;
            memory_cache()
                .lock()
                .unwrap()
                .hits
                .insert(k, Entry { ok, at: now });
            return ok;
        }
    }

    let ok = crate::lyrics::has_synced_lyrics(song, artist, duration_ms);
    let entry = Entry { ok, at: now };
    memory_cache()
        .lock()
        .unwrap()
        .hits
        .insert(k.clone(), entry.clone());
    cache.hits.insert(k, entry);
    if cache.hits.len() > 2_000 {
        cache
            .hits
            .retain(|_, e| now.saturating_sub(e.at) < NEGATIVE_TTL_HOURS);
    }
    store(ctx, &cache);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_normalized() {
        assert_eq!(key("Song", "Artist"), key("song", "artist"));
        assert_eq!(key("S.O.N.G.", "Artist"), key("song", "artist"));
        assert_ne!(key("Song", "Artist"), key("Other", "Artist"));
    }

    #[test]
    fn cache_ttl_expires() {
        let now = 1000u64;
        let hit = Entry {
            ok: true,
            at: now - POSITIVE_TTL_HOURS + 1,
        };
        assert!(fresh(&hit, now), "fresh entry must hit");
        let stale = Entry {
            ok: true,
            at: now - POSITIVE_TTL_HOURS - 1,
        };
        assert!(!fresh(&stale, now), "stale entry must miss");
        let miss = Entry {
            ok: false,
            at: now - NEGATIVE_TTL_HOURS + 1,
        };
        assert!(fresh(&miss, now));
        let stale_miss = Entry {
            ok: false,
            at: now - NEGATIVE_TTL_HOURS - 1,
        };
        assert!(!fresh(&stale_miss, now));
    }

    #[test]
    fn empty_song_is_not_music() {
        let ctx = AppContext::new(
            crate::state::Shared::new(),
            std::sync::Arc::new(std::sync::RwLock::new(crate::config::Settings::default())),
            std::path::PathBuf::from("/nonexistent"),
        );
        assert!(!looks_like_music(&ctx, "   ", "Artist", 0));
    }
}
