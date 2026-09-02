use crate::models::*;

pub(crate) fn classify(c: char) -> ScriptKind {
    let code = c as u32;
    match code {
        HIRAGANA_START..=HIRAGANA_END => ScriptKind::Hiragana,
        KATAKANA_START..=KATAKANA_END => ScriptKind::Katakana,
        IDEOGRAPH_START..=IDEOGRAPH_END => ScriptKind::Ideograph,

        0x30FC => ScriptKind::Katakana,
        _ if c.is_whitespace() => ScriptKind::Space,
        _ => classify_punct(c),
    }
}

fn classify_punct(c: char) -> ScriptKind {
    match c {
        '「' | '『' | '〈' | '《' | '【' | '〔' | '〖' | '〘' | '〝' | '＃' | '（' | '［'
        | '｛' => ScriptKind::LeadingPunct,

        '。' | '、' | '」' | '』' | '〉' | '》' | '】' | '〕' | '〗' | '〙' | '〟' | '：'
        | '；' | '！' | '？' | '）' | '］' | '｝' => ScriptKind::TrailingPunct,

        '〜' | '＿' => ScriptKind::JoiningPunct,

        '・' => ScriptKind::Space,
        _ if c.is_ascii_digit() => ScriptKind::Numeric,

        '.' | ',' => ScriptKind::Other,

        ':' | ';' | '!' | '?' | '#' | ')' | ']' | '}' => ScriptKind::TrailingPunct,
        '(' | '[' | '{' | '_' => ScriptKind::JoiningPunct,
        _ => ScriptKind::Other,
    }
}

pub(crate) fn is_japanese_punct(c: char) -> bool {
    matches!(
        c,
        '、' | '。'
            | '「'
            | '」'
            | '『'
            | '』'
            | '〜'
            | '・'
            | '〈'
            | '〉'
            | '《'
            | '》'
            | '【'
            | '〔'
            | '〗'
            | '〖'
            | '〘'
            | '〙'
            | '〝'
            | '〟'
            | '：'
            | '；'
            | '！'
            | '？'
            | '＃'
            | '）'
            | '］'
            | '｝'
            | '（'
            | '［'
            | '｛'
    )
}

pub(crate) fn is_fullwidth_punct(c: char) -> bool {
    let code = c as u32;
    matches!(code,
        0xFF01..=0xFF0F |
        0xFF1A..=0xFF1F |
        0xFF3B..=0xFF3F |
        0xFF5B..=0xFF60
    )
}

pub(crate) fn punct_to_latin(c: char) -> Option<&'static str> {
    match c {
        '、' => Some(","),
        '。' => Some("."),
        '「' | '』' | '〝' | '〟' | '『' => Some("\""),
        '〈' => Some("<"),
        '〉' => Some(">"),
        '《' => Some("«"),
        '》' => Some("»"),
        '【' | '〖' => Some("["),
        '】' | '〗' => Some("]"),
        '〔' | '〘' | '（' => Some("("),
        '〕' | '〙' | '）' => Some(")"),
        '〜' => Some("~"),
        '：' => Some(":"),
        '；' => Some(";"),
        '！' => Some("!"),
        '？' => Some("?"),
        '＃' => Some("#"),
        '［' => Some("["),
        '］' => Some("]"),
        '｛' => Some("{"),
        '｝' => Some("}"),
        _ => None,
    }
}

pub(crate) fn triggers_capitalization(c: char) -> bool {
    c == '.' || c == '!' || c == '?'
}

pub(crate) fn manage_trailing_space(buf: &mut String, want_space: bool) {
    if buf.is_empty() || buf.ends_with('\n') {
        return;
    }
    if buf.ends_with(' ') {
        if !want_space {
            buf.pop();
        }
    } else if want_space {
        buf.push(' ');
    }
}

pub(crate) fn cap_first(s: &str) -> String {
    let mut found = false;
    s.chars()
        .map(|c| {
            if !found && c.is_alphanumeric() {
                found = true;
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_kana() {
        assert_eq!(classify('あ'), ScriptKind::Hiragana);
        assert_eq!(classify('カ'), ScriptKind::Katakana);
        assert_eq!(classify('漢'), ScriptKind::Ideograph);
        assert_eq!(classify(' '), ScriptKind::Space);
        assert_eq!(classify('a'), ScriptKind::Other);
    }

    #[test]
    fn punct_mapping() {
        assert_eq!(punct_to_latin('。'), Some("."));
        assert_eq!(punct_to_latin('「'), Some("\""));
        assert_eq!(punct_to_latin('a'), None);
    }

    #[test]
    fn cap_first_works() {
        assert_eq!(cap_first("hello"), "Hello");
        assert_eq!(cap_first("123"), "123");
        assert_eq!(cap_first(""), "");
    }
}
