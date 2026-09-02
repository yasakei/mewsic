use crate::config::Settings;
use crate::state::{LyricsLine, Playback};
use crate::util::{crop, format_seconds, letters_only};

pub const MAX_STATUS_LENGTH: usize = 128;

pub fn build_status(settings: &Settings, playback: &Playback, line: &LyricsLine) -> (String, String) {
    let line = if settings.lyrics.romanize {
        LyricsLine {
            time: line.time,
            text: crate::romanize::romanize(&line.text),
        }
    } else {
        line.clone()
    };
    if settings.view.advanced.enabled {
        (
            render_template(
                &settings.view.advanced.template,
                playback,
                &line,
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

fn render_template(template: &str, playback: &Playback, line: &LyricsLine) -> String {
    let song = &playback.song_name;
    let author = &playback.song_author;
    let ts = format_seconds(line.time / 1000);
    let cropped = |s: &str| -> String {
        let s = s.trim();
        match s.find(" -").or_else(|| s.find('(')) {
            Some(i) => s[..i].trim().to_string(),
            None => s.to_string(),
        }
    };

    let mut out = template.to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Playback;

    #[test]
    fn build_status_romanizes_when_enabled() {
        let mut settings = Settings::default();
        settings.view.label = true;
        let line = LyricsLine {
            time: 62_500,
            text: "さくら".to_string(),
        };
        let playback = Playback::default();
        let (text, _) = build_status(&settings, &playback, &line);
        assert!(text.contains("さくら"), "{text}");
        assert!(!text.contains("sakura"), "{text}");

        settings.lyrics.romanize = true;
        let (text, _) = build_status(&settings, &playback, &line);
        assert!(text.contains("sakura"), "{text}");
        assert!(!text.contains("さくら"), "{text}");
    }

    #[test]
    fn build_status_template_uses_romanized_text() {
        let mut settings = Settings::default();
        settings.view.advanced.enabled = true;
        settings.view.advanced.template = "{lyrics}".to_string();
        settings.lyrics.romanize = true;
        let line = LyricsLine {
            time: 0,
            text: "안녕".to_string(),
        };
        let (text, _) = build_status(&settings, &Playback::default(), &line);
        assert_eq!(text, "annyeong");
    }
}
