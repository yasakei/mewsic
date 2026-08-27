use std::time::{SystemTime, UNIX_EPOCH};

pub fn format_seconds(total_secs: u64) -> String {
    let m = total_secs / 60;
    let s = total_secs % 60;
    format!("{m}:{s:02}")
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn sanitize_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

pub fn letters_only(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\'' | '"' | ',' | '.'))
        .collect()
}

pub fn crop(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

pub fn iso_now_plus(offset_secs: i64) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + offset_secs;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    let h = tod / 3600;
    let m = (tod % 3600) / 60;
    let s = tod % 60;
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

pub fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(semi) = chars[i + 1..].iter().position(|&c| c == ';') {
                let end = i + 1 + semi;
                let entity: String = chars[i + 1..end].iter().collect();
                if let Some(decoded) = named_entity(&entity) {
                    out.push_str(decoded);
                    i = end + 1;
                    continue;
                }
                if let Some(num) = entity.strip_prefix('#') {
                    if let Ok(cp) = num.parse::<u32>() {
                        if let Some(c) = char::from_u32(cp) {
                            out.push(c);
                            i = end + 1;
                            continue;
                        }
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn named_entity(name: &str) -> Option<&'static str> {
    match name {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        "nbsp" => Some(" "),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_format() {
        assert_eq!(format_seconds(0), "0:00");
        assert_eq!(format_seconds(137), "2:17");
        assert_eq!(format_seconds(3600), "60:00");
    }

    #[test]
    fn url_encoding() {
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("a/b"), "a%2Fb");
    }

    #[test]
    fn entity_decode() {
        assert_eq!(decode_html_entities("&#39;hi&#33;"), "'hi!");
        assert_eq!(decode_html_entities("a&amp;b&lt;c&gt;"), "a&b<c>");
    }

    #[test]
    fn civil_date() {
        assert_eq!(civil_from_days(19_358), (2023, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_089), (2025, 1, 1));
        assert_eq!(civil_from_days(20_454), (2026, 1, 1));
    }

    #[test]
    fn iso_format() {
        let s = iso_now_plus(0);
        assert!(s.ends_with('Z'));
        assert_eq!(s.len(), 20);
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_filename("A/B:C"), "A_B_C");
    }
}
