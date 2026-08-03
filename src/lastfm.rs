//! Last.fm playback source via the public Audioscrobbler API.
//!
//! Last.fm has no playback *position*, so `progress_ms` is always reported as
//! zero and the engine's local clock advances it between polls. `track.getInfo`
//! supplies the track length so the sync stays sane.

use serde_json::Value;

use crate::connector::{FetchError, PlayerState};
use crate::net;
use crate::util::urlencode;

const LASTFM_API: &str = "https://ws.audioscrobbler.com/2.0/";

/// A detection lag estimate (ms) is only trusted when it falls in this range.
/// Anything larger almost certainly means the player paused or started long
/// before mewsic began polling — in that case the caller uses a default.
pub const TRUSTED_LAG_MS: u64 = 10_000;

/// Fetch the currently-scrobbling track for `username`, plus the Unix end
/// time of the previous *completed* scrobble. That end time is an upper bound
/// for when the current song started, which lets the engine compensate for the
/// fact that it only *notices* the track a few seconds in. Returns `Ok(None)`
/// when nothing is playing right now.
pub fn fetch_player(
    api_key: &str,
    username: &str,
) -> Result<Option<(PlayerState, Option<u64>)>, FetchError> {
    let url = format!(
        "{LASTFM_API}?method=user.getrecenttracks&user={}&api_key={}&limit=2&format=json",
        urlencode(username),
        urlencode(api_key)
    );
    let resp = net::agent()
        .get(&url)
        .call()
        .map_err(|e| FetchError::Other(e.to_string()))?;
    let json: Value = resp.into_json().map_err(|e| FetchError::Other(e.to_string()))?;

    let prev_uts = previous_scrobble_uts(&json);
    let mut state = match parse_nowplaying(&json)? {
        Some(s) => s,
        None => return Ok(None),
    };
    state.duration_ms = fetch_duration_ms(api_key, &state.artist, &state.name).unwrap_or(0);
    Ok(Some((state, prev_uts)))
}

/// Unix end time of the most recent completed scrobble (`track[1]`) — the
/// track that played right before the current nowplaying one.
fn previous_scrobble_uts(json: &Value) -> Option<u64> {
    let track = json.pointer("/recenttracks/track/1")?;
    track
        .get("date")
        .and_then(|d| d.get("uts"))
        .and_then(as_u64_value)
}

/// Estimate how long ago the current song started, in ms, from `now_secs`
/// (wall clock) and `prev_secs` (previous scrobble's Unix end time). Returns
/// `None` when the estimate isn't trustworthy — clock skew, a pause, or a
/// track that began before mewsic started polling.
pub fn measure_lag(now_secs: u64, prev_secs: u64) -> Option<u64> {
    let lag = now_secs.checked_sub(prev_secs)? * 1000;
    if lag > TRUSTED_LAG_MS {
        return None;
    }
    Some(lag)
}

/// Parse a `user.getRecentTracks` response into the nowplaying track, or
/// `Ok(None)` when nothing is currently playing.
fn parse_nowplaying(json: &Value) -> Result<Option<PlayerState>, FetchError> {
    if let Some(err) = json.get("error") {
        let msg = json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("last.fm error");
        return Err(FetchError::Other(format!("last.fm: {msg} ({err})")));
    }

    let track = match json.pointer("/recenttracks/track/0") {
        Some(t) if !t.is_null() => t,
        _ => return Ok(None),
    };

    let now_playing = track
        .get("@attr")
        .and_then(|a| a.get("nowplaying"))
        .and_then(|v| v.as_str())
        == Some("true");
    if !now_playing {
        return Ok(None);
    }

    let name = track
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // `artist`/`album` are objects (`{"#text": "…", "mbid": "…"}`) in JSON
    // responses, but some edge cases return a plain string — handle both.
    let artist = track
        .get("artist")
        .map(json_text)
        .unwrap_or_default();
    if name.is_empty() || artist.is_empty() {
        return Ok(None);
    }

    let mbid = track
        .get("mbid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(Some(PlayerState {
        is_playing: true,
        progress_ms: 0,
        duration_ms: 0, // filled in by `fetch_player` after the getInfo call
        // Same track replayed back-to-back must still count as a new song,
        // hence a synthetic id instead of name+artist alone.
        track_id: format!("{mbid}|{artist}|{name}"),
        name,
        artist,
    }))
}

/// `track.getInfo` reports the length, but its unit is inconsistent — some
/// tracks come back in seconds ("242") and some already in milliseconds
/// ("226000"), always as a JSON string. Values ≥ 60_000 are treated as ms;
/// smaller values are seconds. Failure is non-fatal.
fn fetch_duration_ms(api_key: &str, artist: &str, track: &str) -> Option<u64> {
    let url = format!(
        "{LASTFM_API}?method=track.getInfo&artist={}&track={}&api_key={}&format=json",
        urlencode(artist),
        urlencode(track),
        urlencode(api_key)
    );
    let resp = net::agent().get(&url).call().ok()?;
    let json: Value = resp.into_json().ok()?;
    let raw = json
        .pointer("/track/duration")
        .and_then(as_u64_value)?;
    if raw == 0 {
        return None;
    }
    Some(if raw >= 60_000 { raw } else { raw.saturating_mul(1000) })
}

/// Extract a nested `#text` (JSON `{"#text": "…"}`) or a plain string value.
fn json_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(map) => map
            .get("#text")
            .and_then(|t| t.as_str())
            .map(String::from)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Read a u64 from either a JSON number or a numeric string.
fn as_u64_value(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nowplaying_with_object_artist() {
        let json = serde_json::json!({
            "recenttracks": { "track": [{
                "@attr": { "nowplaying": "true" },
                "artist": { "mbid": "x", "#text": "Beyoncé" },
                "name": "A Minecraft Parody",
                "mbid": "track-id"
            }]}
        });
        let s = parse_nowplaying(&json).unwrap().unwrap();
        assert_eq!(s.name, "A Minecraft Parody");
        assert_eq!(s.artist, "Beyoncé");
        assert!(s.is_playing);
        assert_eq!(s.track_id, "track-id|Beyoncé|A Minecraft Parody");
    }

    #[test]
    fn parses_nowplaying_with_string_artist() {
        let json = serde_json::json!({
            "recenttracks": { "track": [{
                "@attr": { "nowplaying": "true" },
                "artist": "Plain Artist",
                "name": "Song"
            }]}
        });
        let s = parse_nowplaying(&json).unwrap().unwrap();
        assert_eq!(s.artist, "Plain Artist");
    }

    #[test]
    fn last_scrobbled_track_is_not_nowplaying() {
        let json = serde_json::json!({
            "recenttracks": { "track": [{
                "artist": { "#text": "A" },
                "name": "B",
                "date": { "uts": "123" }
            }]}
        });
        assert!(parse_nowplaying(&json).unwrap().is_none());
    }

    #[test]
    fn duration_seconds_vs_millis() {
        // Simulated /track/duration values.
        assert_eq!(as_u64_value(&Value::from("242")), Some(242));
        assert_eq!(as_u64_value(&Value::from(226000)), Some(226000));
        let secs = as_u64_value(&serde_json::json!("242")).unwrap();
        assert_eq!(if secs >= 60_000 { secs } else { secs * 1000 }, 242_000);
        let ms = as_u64_value(&serde_json::json!("226000")).unwrap();
        assert_eq!(if ms >= 60_000 { ms } else { ms * 1000 }, 226_000);
    }

    #[test]
    fn api_error_is_reported() {
        let json = serde_json::json!({ "error": 10, "message": "Invalid API key" });
        let err = parse_nowplaying(&json).unwrap_err();
        assert!(format!("{err:?}").contains("Invalid API key"));
    }

    #[test]
    fn previous_scrobble_uts_is_read_from_second_track() {
        let json = serde_json::json!({
            "recenttracks": { "track": [
                { "@attr": { "nowplaying": "true" }, "name": "Now" },
                { "name": "Prev", "date": { "uts": "1700000000" } }
            ]}
        });
        assert_eq!(previous_scrobble_uts(&json), Some(1_700_000_000));
        let no_prev = serde_json::json!({ "recenttracks": { "track": [
            { "@attr": { "nowplaying": "true" }, "name": "Now" }
        ]}});
        assert_eq!(previous_scrobble_uts(&no_prev), None);
    }

    #[test]
    fn lag_is_measured_from_previous_scrobble_end() {
        // Played 3 s ago: trusted, converted to ms.
        assert_eq!(measure_lag(1_700_000_005, 1_700_000_000), Some(5_000));
        // Started before mewsic began polling / long pause: untrusted.
        assert_eq!(measure_lag(1_700_000_100, 1_700_000_000), None);
        // Clock skew: untrusted.
        assert_eq!(measure_lag(1_700_000_000, 1_700_000_001), None);
        // Tight gapless transition: still trusted (even 0 ms).
        assert_eq!(measure_lag(1_700_000_001, 1_700_000_001), Some(0));
    }
}

