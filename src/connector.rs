use serde_json::json;

use crate::net;

const DISCORD_API: &str = "https://discord.com/api/v10";
const SPOTIFY_API: &str = "https://api.spotify.com/v1";

#[derive(Debug)]
pub enum FetchError {
    Unauthorized,
    Other(String),
}

#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    pub is_playing: bool,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub track_id: String,
    pub name: String,
    pub artist: String,
}

pub fn fetch_spotify_token(discord_token: &str) -> Result<String, FetchError> {
    let url = format!("{DISCORD_API}/users/@me/connections");
    let resp = net::agent()
        .get(&url)
        .set("Authorization", discord_token)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => FetchError::Unauthorized,
            other => FetchError::Other(other.to_string()),
        })?;

    let json: serde_json::Value = resp.into_json().map_err(|e| FetchError::Other(e.to_string()))?;

    let connections = json.as_array().ok_or_else(|| {
        FetchError::Other("unexpected connections response".to_string())
    })?;

    for conn in connections {
        if conn.get("type").and_then(|v| v.as_str()) == Some("spotify") {
            if let Some(token) = conn.get("access_token").and_then(|v| v.as_str()) {
                return Ok(token.to_string());
            }
        }
    }
    Err(FetchError::Other(
        "no Spotify account connected to Discord".to_string(),
    ))
}

pub fn fetch_player(spotify_token: &str) -> Result<Option<PlayerState>, FetchError> {
    let url = format!("{SPOTIFY_API}/me/player");
    let resp = net::agent()
        .get(&url)
        .set("Authorization", &format!("Bearer {spotify_token}"))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => FetchError::Unauthorized,
            other => FetchError::Other(other.to_string()),
        })?;

    if resp.status() == 204 {
        return Ok(None);
    }

    let json: serde_json::Value = resp.into_json().map_err(|e| FetchError::Other(e.to_string()))?;
    let item = json.get("item");
    if item.is_none() || item.unwrap().is_null() {
        return Ok(None);
    }
    let item = item.unwrap();

    let artist = item
        .get("artists")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(Some(PlayerState {
        is_playing: json.get("is_playing").and_then(|v| v.as_bool()).unwrap_or(false),
        progress_ms: json.get("progress_ms").and_then(|v| v.as_u64()).unwrap_or(0),
        duration_ms: item.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
        track_id: item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        name: item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        artist,
    }))
}

pub fn patch_status(discord_token: &str, text: &str, emoji: &str) -> Result<(), FetchError> {
    let url = format!("{DISCORD_API}/users/@me/settings");

    let expires = if text.is_empty() {
        serde_json::Value::Null
    } else {
        json!(crate::util::iso_now_plus(60))
    };
    let emoji_name = if emoji.is_empty() {
        serde_json::Value::Null
    } else {
        json!(emoji)
    };

    let body = json!({
        "custom_status": {
            "text": text,
            "emoji_id": null,
            "emoji_name": emoji_name,
            "expires_at": expires,
        }
    });

    net::agent()
        .patch(&url)
        .set("Authorization", discord_token)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => FetchError::Unauthorized,
            other => FetchError::Other(other.to_string()),
        })?;

    Ok(())
}

pub fn validate_token(token: &str) -> bool {
    let url = format!("{DISCORD_API}/users/@me");
    net::agent()
        .get(&url)
        .set("Authorization", token)
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

pub fn cleanup_title(title: &str) -> String {
    let t = title.trim();
    match t.find(" (") {
        Some(idx) => t[..idx].trim().to_string(),
        None => t.to_string(),
    }
}
