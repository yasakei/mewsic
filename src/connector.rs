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
    let resp = net::discord_agent()
        .get(&url)
        .set("Authorization", discord_token)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => FetchError::Unauthorized,
            other => FetchError::Other(other.to_string()),
        })?;

    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| FetchError::Other(e.to_string()))?;

    let connections = json
        .as_array()
        .ok_or_else(|| FetchError::Other("unexpected connections response".to_string()))?;

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
    let resp = net::spotify_agent()
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

    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| FetchError::Other(e.to_string()))?;
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
        is_playing: json
            .get("is_playing")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        progress_ms: json
            .get("progress_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        duration_ms: item
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        track_id: item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        name: item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        artist,
    }))
}

pub fn fetch_status(discord_token: &str) -> Result<serde_json::Value, FetchError> {
    let url = format!("{DISCORD_API}/users/@me/settings");
    let resp = net::discord_agent()
        .get(&url)
        .set("Authorization", discord_token)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(401, _) => FetchError::Unauthorized,
            other => FetchError::Other(other.to_string()),
        })?;
    let settings: serde_json::Value = resp
        .into_json()
        .map_err(|e| FetchError::Other(e.to_string()))?;

    Ok(settings
        .get("custom_status")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

pub fn restore_status(
    discord_token: &str,
    custom_status: &serde_json::Value,
) -> Result<(), FetchError> {
    let url = format!("{DISCORD_API}/users/@me/settings");
    let body = json!({ "custom_status": custom_status });

    net::discord_agent()
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

pub fn patch_status(discord_token: &str, text: &str, emoji: &str) -> Result<(), FetchError> {
    let url = format!("{DISCORD_API}/users/@me/settings");

    let expires = if text.is_empty() {
        serde_json::Value::Null
    } else {
        json!(crate::util::iso_now_plus(60))
    };
    let (emoji_name, emoji_id) = parse_emoji(emoji);
    let emoji_name = if emoji_name.is_empty() {
        serde_json::Value::Null
    } else {
        json!(emoji_name)
    };
    let emoji_id = match emoji_id {
        Some(id) => json!(id),
        None => serde_json::Value::Null,
    };

    let body = json!({
        "custom_status": {
            "text": text,
            "emoji_id": emoji_id,
            "emoji_name": emoji_name,
            "expires_at": expires,
        }
    });

    net::discord_agent()
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

/// Parses the configured emoji into the Discord `(emoji_name, emoji_id)` pair.
///
/// A plain unicode emoji like `🎧` becomes `("🎧", None)` — Discord renders
/// standard emoji with `emoji_id = null`. A server/custom emoji pasted as
/// `<:pepesad:812345678901234567>` or `pepesad:812345678901234567` becomes
/// `("pepesad", Some("812..."))`, which is how Discord expects custom emoji;
/// without the `emoji_id` a server emoji renders as a blank/broken box.
fn parse_emoji(emoji: &str) -> (String, Option<String>) {
    let trimmed = emoji.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    // Discord mention syntax: <:name:id> (and <a:name:id> for animated).
    if let Some(rest) = trimmed.strip_prefix('<') {
        let inner = rest.strip_suffix('>').unwrap_or(rest);
        let inner = inner.trim_start_matches('a').trim_start_matches(':');
        if let Some((name, id)) = inner.split_once(':') {
            if !name.is_empty() && !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                return (name.to_string(), Some(id.to_string()));
            }
        }
    }
    // Bare `name:id` form.
    if let Some((name, id)) = trimmed.split_once(':') {
        if !name.is_empty() && !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            return (name.to_string(), Some(id.to_string()));
        }
    }
    // Treat as a plain unicode emoji.
    (trimmed.to_string(), None)
}

pub fn validate_token(token: &str) -> bool {
    let url = format!("{DISCORD_API}/users/@me");
    net::discord_agent()
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

#[cfg(test)]
mod tests {
    use super::parse_emoji;

    #[test]
    fn unicode_emoji_has_no_id() {
        assert_eq!(parse_emoji("🎧"), ("🎧".to_string(), None));
    }

    #[test]
    fn server_emoji_mention_is_split() {
        assert_eq!(
            parse_emoji("<:pepesad:812345678901234567>"),
            (
                "pepesad".to_string(),
                Some("812345678901234567".to_string())
            )
        );
    }

    #[test]
    fn animated_server_emoji_is_split() {
        assert_eq!(
            parse_emoji("<a:pepeDance:998877665544332211>"),
            (
                "pepeDance".to_string(),
                Some("998877665544332211".to_string())
            )
        );
    }

    #[test]
    fn bare_name_id_is_split() {
        assert_eq!(
            parse_emoji("pepesad:812345678901234567"),
            (
                "pepesad".to_string(),
                Some("812345678901234567".to_string())
            )
        );
    }

    #[test]
    fn empty_is_empty() {
        assert_eq!(parse_emoji(""), (String::new(), None));
        assert_eq!(parse_emoji("   "), (String::new(), None));
    }

    #[test]
    fn plain_text_falls_through_as_unicode() {
        // Not a valid name:id pair — treat whole string as the emoji name.
        assert_eq!(parse_emoji("abc"), ("abc".to_string(), None));
    }
}
