//! Build the Discord status string from the current lyric line and settings.

use crate::config::Settings;
use crate::state::{LyricsLine, Playback};
use crate::util::{crop, format_seconds, letters_only};

pub const MAX_STATUS_LENGTH: usize = 128;

/// Compose the status text + emoji for a line according to the view settings.
pub fn build_status(settings: &Settings, playback: &Playback, line: &LyricsLine) -> (String, String) {
    if settings.view.advanced.enabled {
        (
            render_template(
                &settings.view.advanced.template,
                playback,
                line,
            ),
            settings.view.advanced.emoji.clone(),
        )
    } else {
        let mut parts: Vec<String> = Vec::new();
        if settings.view.timestamp {
            parts.push(format!("[{}]", format_seconds(line.time / 1000)));
        }
        if settings.view.label {
            parts.push("Song lyrics -".to_string());
        }
        parts.push(line.text.replace('♪', "🎶"));
        (crop(&parts.join(" "), MAX_STATUS_LENGTH), settings.view.emoji.clone())
    }
}

/// Interpolate `{placeholder}` tokens in an advanced template.
fn render_template(template: &str, playback: &Playback, line: &LyricsLine) -> String {
    let song = &playback.song_name;
    let author = &playback.song_author;
    let ts = format_seconds(line.time / 1000);
    let cropped = |s: &str| -> String {
        let s = s.trim();
        // Strip a trailing " - ..." (feat./remix suffix) or "(...)" tag.
        match s.find(" -").or_else(|| s.find('(')) {
            Some(i) => s[..i].trim().to_string(),
            None => s.to_string(),
        }
    };

    let mut out = template.to_string();
    // Longest-first ordering keeps `{lyrics_upper}` safe from `{lyrics}`.
    let tokens = [
        ("{lyrics_upper_letters_only}", letters_only(&line.text.to_uppercase())),
        ("{lyrics_lower_letters_only}", letters_only(&line.text.to_lowercase())),
        ("{lyrics_letters_only}", letters_only(&line.text)),
        ("{song_name_upper_cropped}", cropped(&song.to_uppercase())),
        ("{song_name_lower_cropped}", cropped(&song.to_lowercase())),
        ("{song_name_cropped}", cropped(song)),
        ("{song_author_upper}", author.to_uppercase()),
        ("{song_author_lower}", author.to_lowercase()),
        ("{lyrics_upper}", line.text.to_uppercase()),
        ("{lyrics_lower}", line.text.to_lowercase()),
        ("{song_name_upper}", song.to_uppercase()),
        ("{song_name_lower}", song.to_lowercase()),
        ("{song_name}", song.clone()),
        ("{song_author}", author.clone()),
        ("{lyrics}", line.text.clone()),
        ("{timestamp}", ts),
    ];

    for (key, value) in tokens {
        out = out.replace(key, &value);
    }
    out.replace('♪', "🎶")
}
