use crate::connector::PlayerState;

pub fn fetch_player(preferred_player: &str) -> Option<PlayerState> {
    let _ = preferred_player;
    #[cfg(target_os = "linux")]
    return mpris::fetch(preferred_player);
    #[cfg(target_os = "macos")]
    return macos::fetch(preferred_player);
    #[cfg(target_os = "windows")]
    return windows::fetch();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = preferred_player;
        return None;
    }
}

#[cfg(target_os = "linux")]
mod mpris {
    use super::PlayerState;

    const DBUS_TIMEOUT_MS: i32 = 1500;

    pub fn fetch(preferred: &str) -> Option<PlayerState> {
        let finder = mpris::PlayerFinder::new().ok()?;
        let players = finder.find_all().ok()?;
        if players.is_empty() {
            return None;
        }

        let mut ranked: Vec<(u8, bool, mpris::Player)> = Vec::new();
        for mut player in players {
            player.set_dbus_timeout_ms(DBUS_TIMEOUT_MS);
            let Ok(status) = player.get_playback_status() else {
                continue;
            };
            let playing = match status {
                mpris::PlaybackStatus::Playing => true,
                mpris::PlaybackStatus::Paused => false,
                mpris::PlaybackStatus::Stopped => continue,
            };
            ranked.push((
                rank(playing, is_preferred(&player, preferred)),
                playing,
                player,
            ));
        }
        ranked.sort_by_key(|(rank, _, _)| *rank);
        if ranked.is_empty() {
            return None;
        }

        let contenders: Vec<String> = ranked
            .iter()
            .filter(|(rank, _, _)| *rank <= 1)
            .map(|(_, _, p)| p.identity().to_string())
            .collect();
        if contenders.len() > 1 {
            crate::log::write(&format!(
                "several players at once ({}); following {}",
                contenders.join(", "),
                ranked[0].2.identity(),
            ));
        }

        for (_, playing, player) in &ranked {
            if let Some(state) = state_for(player, *playing) {
                return Some(state);
            }
        }
        None
    }

    fn rank(playing: bool, preferred: bool) -> u8 {
        match (playing, preferred) {
            (true, true) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 3,
        }
    }

    fn is_preferred(player: &mpris::Player, preferred: &str) -> bool {
        let preferred = preferred.trim().to_lowercase();
        if preferred.is_empty() {
            return false;
        }
        player
            .bus_name_trimmed()
            .to_lowercase()
            .contains(&preferred)
            || player.identity().to_lowercase().contains(&preferred)
    }

    fn state_for(player: &mpris::Player, playing: bool) -> Option<PlayerState> {
        let meta = player.get_metadata().ok()?;
        let title = meta.title().unwrap_or("").trim().to_string();
        if title.is_empty() {
            return None;
        }
        let artist = meta.artists().map(|a| a.join(", ")).unwrap_or_default();
        let duration_ms = meta.length_in_microseconds().unwrap_or(0) / 1000;
        let progress_ms = player.get_position_in_microseconds().unwrap_or(0) / 1000;
        let youtube_id = meta.url().and_then(crate::lyrics::youtube_id_from_url);
        let track_id = match &youtube_id {
            Some(id) => format!("local:yt:{id}"),
            None => format!("local:{title}\0{artist}"),
        };
        Some(PlayerState {
            is_playing: playing,
            progress_ms,
            duration_ms,
            track_id,
            name: crate::connector::cleanup_title(&title),
            artist,
            youtube_id,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::rank;

        #[test]
        fn fetch_never_panics() {
            let _ = super::fetch("");
        }

        #[test]
        fn ranking_prefers_playing_then_preferred() {
            assert_eq!(rank(true, true), 0);
            assert_eq!(rank(true, false), 1);
            assert_eq!(rank(false, true), 2);
            assert_eq!(rank(false, false), 3);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::PlayerState;
    use std::time::Duration;

    const SEP: char = '\x1f';
    const TIMEOUT: Duration = Duration::from_secs(5);

    pub fn fetch(preferred: &str) -> Option<PlayerState> {
        let music_first = {
            let p = preferred.trim().to_lowercase();
            p.contains("music") || p.contains("apple")
        };
        if music_first {
            if let Some(s) = query_music() {
                return parse(&s, false);
            }
            return query_spotify().and_then(|s| parse(&s, true));
        }
        if let Some(s) = query_spotify() {
            return parse(&s, true);
        }
        query_music().and_then(|s| parse(&s, false))
    }

    fn query_spotify() -> Option<String> {
        let script = "try\ntell application \"Spotify\" to return (player state as string) & (character id 31) & (name of current track) & (character id 31) & (artist of current track) & (character id 31) & (player position as string) & (character id 31) & (duration of current track as string) & (character id 31) & (database ID of current track as string)\nend try";
        run_osascript(script)
    }

    fn query_music() -> Option<String> {
        let script = "try\ntell application \"Music\" to return (player state as string) & (character id 31) & (name of current track) & (character id 31) & (artist of current track) & (character id 31) & (player position as string) & (character id 31) & (duration of current track as string) & (character id 31) & (persistent ID of current track as string)\nend try";
        run_osascript(script)
    }

    fn run_osascript(script: &str) -> Option<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let script = script.to_string();
        std::thread::spawn(move || {
            let out = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output()
                .ok();
            let _ = tx.send(out);
        });
        let out = rx.recv_timeout(TIMEOUT).ok()??;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.is_empty() {
            return None;
        }
        Some(text)
    }

    fn parse(output: &str, duration_in_ms: bool) -> Option<PlayerState> {
        let parts: Vec<&str> = output.split(SEP).collect();
        if parts.len() != 6 {
            return None;
        }
        let is_playing = parts[0].trim() == "playing";
        if parts[0].trim() == "stopped" {
            return None;
        }
        let title = parts[1].trim();
        if title.is_empty() {
            return None;
        }
        let artist = parts[2].trim().to_string();
        let progress_ms = (parts[3].trim().parse::<f64>().ok()? * 1000.0) as u64;
        let raw_duration: f64 = parts[4].trim().parse().ok()?;
        let duration_ms = if duration_in_ms {
            raw_duration as u64
        } else {
            (raw_duration * 1000.0) as u64
        };
        let id = parts[5].trim();
        Some(PlayerState {
            is_playing,
            progress_ms,
            duration_ms,
            track_id: format!("local:{id}"),
            name: crate::connector::cleanup_title(title),
            artist,
            youtube_id: None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::parse;

        #[test]
        fn spotify_output_parses() {
            let out = "playing\x1fBlinding Lights\x1fThe Weeknd\x1f12.5\x1f200000\x1fabc123";
            let s = parse(out, true).unwrap();
            assert!(s.is_playing);
            assert_eq!(s.name, "Blinding Lights");
            assert_eq!(s.artist, "The Weeknd");
            assert_eq!(s.progress_ms, 12_500);
            assert_eq!(s.duration_ms, 200_000);
        }

        #[test]
        fn music_app_seconds_become_ms() {
            let out = "paused\x1fSong\x1fArtist\x1f30.0\x1f180.0\x1fxyz";
            let s = parse(out, false).unwrap();
            assert!(!s.is_playing);
            assert_eq!(s.progress_ms, 30_000);
            assert_eq!(s.duration_ms, 180_000);
        }

        #[test]
        fn stopped_or_empty_is_none() {
            assert!(parse("stopped\x1fa\x1fb\x1f0\x1f0\x1fid", true).is_none());
            assert!(parse("playing\x1f\x1fb\x1f0\x1f0\x1fid", true).is_none());
            assert!(parse("garbage", true).is_none());
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::PlayerState;
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(8);

    pub fn fetch() -> Option<PlayerState> {
        run_smtc().and_then(|s| parse(&s))
    }

    fn run_smtc() -> Option<String> {
        let script = r#"
try {
  $mgr = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync().GetAwaiter().GetResult()
  $s = $mgr.GetCurrentSession()
  if ($null -eq $s) { exit 1 }
  $info = $s.TryGetMediaPropertiesAsync().GetAwaiter().GetResult()
  $pb = $s.GetPlaybackInfo()
  $tl = $s.GetTimelineProperties()
  $sep = [char]31
  "$($pb.PlaybackStatus)$sep$($info.Title)$sep$($info.Artist)$sep$($tl.Position.TotalMilliseconds)$sep$($tl.EndTime.TotalMilliseconds)"
} catch { exit 1 }
"#;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .output()
                .ok();
            let _ = tx.send(out);
        });
        let out = rx.recv_timeout(TIMEOUT).ok()??;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.is_empty() {
            return None;
        }
        Some(text)
    }

    fn parse(output: &str) -> Option<PlayerState> {
        let parts: Vec<&str> = output.split('\x1f').collect();
        if parts.len() != 5 {
            return None;
        }
        match parts[0].trim() {
            "Playing" => {}
            "Paused" => {}
            _ => return None,
        }
        let title = parts[1].trim();
        if title.is_empty() {
            return None;
        }
        Some(PlayerState {
            is_playing: parts[0].trim() == "Playing",
            progress_ms: parts[3].trim().parse::<f64>().unwrap_or(0.0) as u64,
            duration_ms: parts[4].trim().parse::<f64>().unwrap_or(0.0) as u64,
            track_id: format!("local:{title}\0{}", parts[2].trim()),
            name: crate::connector::cleanup_title(title),
            artist: parts[2].trim().to_string(),
            youtube_id: None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::parse;

        #[test]
        fn smtc_output_parses() {
            let out = "Playing\x1fSong\x1fArtist\x1f65000\x1f210000";
            let s = parse(out).unwrap();
            assert!(s.is_playing);
            assert_eq!(s.progress_ms, 65_000);
            assert_eq!(s.duration_ms, 210_000);
        }

        #[test]
        fn closed_or_empty_is_none() {
            assert!(parse("Closed\x1fa\x1fb\x1f0\x1f0").is_none());
            assert!(parse("Playing\x1f\x1fb\x1f0\x1f0").is_none());
            assert!(parse("garbage").is_none());
        }
    }
}
