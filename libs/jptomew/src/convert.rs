use unicode_normalization::UnicodeNormalization;

use crate::classify::*;
use crate::dict::{KanjiLookup, KANJI_ALIASES, TAIL_GROUPS};
use crate::models::*;
use crate::models::{DictKey, Reading};

pub(crate) fn transliterate(input: &str, dict: &KanjiLookup) -> TranslitResult {
    let cleaned = normalize_input(input);
    let chars: Vec<char> = cleaned.chars().collect();
    let mut result = TranslitResult::with_capacity(cleaned.len());

    let mut buf = String::new();

    let mut prev_rom = ScriptKind::Space;

    let mut cap_next = false;
    let mut cap_sent = false;
    let mut cap_sent_done = false;

    macro_rules! flush {
        () => {
            flush_buf(
                &mut buf,
                &mut result,
                &mut prev_rom,
                &mut cap_next,
                &mut cap_sent,
                &mut cap_sent_done,
            )
        };
    }

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let kind = classify(c);

        match kind {
            ScriptKind::Hiragana | ScriptKind::Katakana => {
                buf.push(c);
                i += 1;
            }

            ScriptKind::Ideograph => {
                let resolved = resolve_kanji(&chars[i..], &buf, dict);
                flush!();
                match resolved {
                    Some((hira, consumed, continues)) => {
                        buf.push_str(&hira);
                        if continues {
                        } else {
                            flush!();
                        }
                        i += consumed;
                    }
                    None => {
                        result.hiragana.push(c);
                        result.romaji.push(c);
                        prev_rom = ScriptKind::Ideograph;
                        i += 1;
                    }
                }
            }

            ScriptKind::Space => {
                flush!();
                if c == '・' {
                    result.hiragana.push(c);
                    result.romaji.push(' ');
                } else {
                    result.hiragana.push(c);
                    result.romaji.push(c);
                }
                prev_rom = ScriptKind::Space;
                i += 1;
            }

            kind => {
                flush!();
                result.hiragana.push(c);

                let is_jpunct = is_japanese_punct(c);
                if is_japanese_kind(prev_rom) || is_jpunct {
                    manage_trailing_space(
                        &mut result.romaji,
                        prev_rom.emits_space_after() && kind.emits_space_before(),
                    );
                }

                if is_jpunct && kind == ScriptKind::Other {
                    result.romaji.extend(c.nfkc());
                } else if let Some(lat) = punct_to_latin(c) {
                    result.romaji.push_str(lat);
                    if triggers_capitalization(lat.chars().next().unwrap_or('x'))
                        && kind != ScriptKind::JoiningPunct
                    {
                        cap_next = true;
                        cap_sent = true;
                    }
                } else {
                    result.romaji.push(c);
                }
                prev_rom = kind;
                i += 1;
            }
        }
    }
    flush!();
    result
}

fn flush_buf(
    buf: &mut String,
    result: &mut TranslitResult,
    prev_rom: &mut ScriptKind,
    cap_next: &mut bool,
    cap_sent: &mut bool,
    cap_sent_done: &mut bool,
) {
    if buf.is_empty() {
        return;
    }
    let hira = katakana_to_hiragana(buf);
    result.hiragana.push_str(&hira);
    let mut rom = kana_to_latin(&hira);
    if *cap_next {
        rom = cap_first(&rom);
        *cap_next = false;
    }
    if *cap_sent && !*cap_sent_done {
        result.romaji = cap_first(&result.romaji);
        *cap_sent_done = true;
    }
    manage_trailing_space(&mut result.romaji, prev_rom.emits_space_after());
    result.romaji.push_str(&rom);
    buf.clear();
    *prev_rom = ScriptKind::Hiragana;
}

fn is_japanese_kind(kind: ScriptKind) -> bool {
    matches!(
        kind,
        ScriptKind::Hiragana | ScriptKind::Katakana | ScriptKind::Ideograph
    )
}

fn is_ideograph(c: char) -> bool {
    let code = c as u32;
    (0x4E00..=0x9FFF).contains(&code) || (0x3400..=0x4DBF).contains(&code)
}

fn resolve_kanji(text: &[char], buf: &str, dict: &KanjiLookup) -> Option<(String, usize, bool)> {
    let mut best: Option<(String, usize, bool)> = None;
    for len in 1..=text.len() {
        let key: String = text[..len].iter().collect();
        let mut readings = match dict.find_readings(DictKey::borrowed(&key)) {
            Some(r) => r,
            None => break,
        };

        let spans_kana = text[..len].iter().any(|&c| !is_ideograph(c));
        let applied = readings.find_map(|r| match r {
            Reading::Plain { hira } => Some((hira, len, spans_kana)),
            Reading::Conditional { hira, tail_byte } => {
                let &next = text.get(len)?;
                let group = TAIL_GROUPS.get(&tail_byte)?;
                group.contains(&next).then(|| {
                    let mut hira = hira;
                    hira.push(next);
                    (hira, len + 1, true)
                })
            }
            Reading::Contextual { hira, context } => {
                buf.contains(&context).then_some((hira, len, spans_kana))
            }
        });
        if applied.is_some() {
            best = applied;
        }
    }
    best
}

fn normalize_input(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut repeat_count: Option<usize> = None;

    for (i, &c) in chars.iter().enumerate() {
        if c == REPETITION_MARK {
            if repeat_count.is_none() {
                repeat_count = Some(1);
                for &following in &chars[i + 1..] {
                    if following == REPETITION_MARK {
                        if let Some(n) = repeat_count.as_mut() {
                            *n += 1;
                        }
                    } else {
                        break;
                    }
                }
            }
            if let Some(count) = repeat_count {
                let lookback = out.chars().rev().nth(count - 1);
                if let Some(prev_char) = lookback {
                    out.push(prev_char);
                }
            }
        } else {
            repeat_count = None;
            let resolved = KANJI_ALIASES.get(&c).copied().unwrap_or(c);
            if is_fullwidth_punct(c) {
                out.push(resolved);
            } else {
                out.extend(resolved.nfkc());
            }
        }
    }
    out
}

fn combined_romaji(a: char, b: char) -> Option<&'static str> {
    match (a, b) {
        ('き', 'ゃ') => Some("kya"),
        ('き', 'ゅ') => Some("kyu"),
        ('き', 'ょ') => Some("kyo"),
        ('ぎ', 'ゃ') => Some("gya"),
        ('ぎ', 'ゅ') => Some("gyu"),
        ('ぎ', 'ょ') => Some("gyo"),
        ('し', 'ゃ') => Some("sha"),
        ('し', 'ゅ') => Some("shu"),
        ('し', 'ょ') => Some("sho"),
        ('じ', 'ゃ') => Some("ja"),
        ('じ', 'ゅ') => Some("ju"),
        ('じ', 'ょ') => Some("jo"),
        ('ち', 'ゃ') => Some("cha"),
        ('ち', 'ゅ') => Some("chu"),
        ('ち', 'ょ') => Some("cho"),
        ('に', 'ゃ') => Some("nya"),
        ('に', 'ゅ') => Some("nyu"),
        ('に', 'ょ') => Some("nyo"),
        ('ひ', 'ゃ') => Some("hya"),
        ('ひ', 'ゅ') => Some("hyu"),
        ('ひ', 'ょ') => Some("hyo"),
        ('び', 'ゃ') => Some("bya"),
        ('び', 'ゅ') => Some("byu"),
        ('び', 'ょ') => Some("byo"),
        ('ぴ', 'ゃ') => Some("pya"),
        ('ぴ', 'ゅ') => Some("pyu"),
        ('ぴ', 'ょ') => Some("pyo"),
        ('み', 'ゃ') => Some("mya"),
        ('み', 'ゅ') => Some("myu"),
        ('み', 'ょ') => Some("myo"),
        ('り', 'ゃ') => Some("rya"),
        ('り', 'ゅ') => Some("ryu"),
        ('り', 'ょ') => Some("ryo"),

        ('ふ', 'ぁ') => Some("fa"),
        ('ふ', 'ぃ') => Some("fi"),
        ('ふ', 'ぇ') => Some("fe"),
        ('ふ', 'ぉ') => Some("fo"),
        _ => None,
    }
}

fn single_romaji(c: char) -> Option<&'static str> {
    match c {
        'あ' => Some("a"),
        'い' => Some("i"),
        'う' => Some("u"),
        'え' => Some("e"),
        'お' => Some("o"),
        'か' => Some("ka"),
        'き' => Some("ki"),
        'く' => Some("ku"),
        'け' => Some("ke"),
        'こ' => Some("ko"),
        'さ' => Some("sa"),
        'し' => Some("shi"),
        'す' => Some("su"),
        'せ' => Some("se"),
        'そ' => Some("so"),
        'た' => Some("ta"),
        'ち' => Some("chi"),
        'つ' => Some("tsu"),
        'て' => Some("te"),
        'と' => Some("to"),
        'な' => Some("na"),
        'に' => Some("ni"),
        'ぬ' => Some("nu"),
        'ね' => Some("ne"),
        'の' => Some("no"),
        'は' => Some("ha"),
        'ひ' => Some("hi"),
        'ふ' => Some("fu"),
        'へ' => Some("he"),
        'ほ' => Some("ho"),
        'ま' => Some("ma"),
        'み' => Some("mi"),
        'む' => Some("mu"),
        'め' => Some("me"),
        'も' => Some("mo"),
        'や' => Some("ya"),
        'ゆ' => Some("yu"),
        'よ' => Some("yo"),
        'ら' => Some("ra"),
        'り' => Some("ri"),
        'る' => Some("ru"),
        'れ' => Some("re"),
        'ろ' => Some("ro"),
        'わ' => Some("wa"),
        'ゐ' => Some("wi"),
        'ゑ' => Some("we"),
        'を' => Some("wo"),
        'ん' => Some("n"),
        'が' => Some("ga"),
        'ぎ' => Some("gi"),
        'ぐ' => Some("gu"),
        'げ' => Some("ge"),
        'ご' => Some("go"),
        'ざ' => Some("za"),
        'じ' => Some("ji"),
        'ず' => Some("zu"),
        'ぜ' => Some("ze"),
        'ぞ' => Some("zo"),
        'だ' => Some("da"),
        'ぢ' => Some("ji"),
        'づ' => Some("zu"),
        'で' => Some("de"),
        'ど' => Some("do"),
        'ば' => Some("ba"),
        'び' => Some("bi"),
        'ぶ' => Some("bu"),
        'べ' => Some("be"),
        'ぼ' => Some("bo"),
        'ぱ' => Some("pa"),
        'ぴ' => Some("pi"),
        'ぷ' => Some("pu"),
        'ぺ' => Some("pe"),
        'ぽ' => Some("po"),
        'ゔ' => Some("vu"),
        'ゕ' => Some("ka"),
        'ゖ' => Some("ke"),
        'ぁ' => Some("a"),
        'ぃ' => Some("i"),
        'ぅ' => Some("u"),
        'ぇ' => Some("e"),
        'ぉ' => Some("o"),
        'ゎ' => Some("wa"),
        _ => None,
    }
}

fn last_vowel(rom: &str) -> Option<char> {
    rom.chars()
        .rev()
        .find(|ch| matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u'))
}

fn kana_to_latin(hira: &str) -> String {
    let chars: Vec<char> = hira.chars().collect();
    let mut out = String::with_capacity(hira.len());
    let mut last_full_vowel: Option<char> = None;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == LONG_VOWEL {
            if let Some(v) = last_full_vowel {
                out.push(v);
            }
            i += 1;
            continue;
        }

        if i + 1 < chars.len() {
            if let Some(rom) = combined_romaji(c, chars[i + 1]) {
                out.push_str(rom);
                last_full_vowel = last_vowel(rom);
                i += 2;
                continue;
            }
        }

        if c == 'っ' {
            if let Some(&next) = chars.get(i + 1) {
                if next != 'っ' {
                    if let Some(rom) = single_romaji(next) {
                        if let Some(first) = rom.chars().next() {
                            if first.is_ascii_alphabetic() {
                                out.push(first);
                                i += 1;
                                continue;
                            }
                        }
                    }
                }
            }
            out.push_str("tsu");
            i += 1;
            continue;
        }

        if c == 'ん' {
            let next_is_vowel_y = chars
                .get(i + 1)
                .and_then(|&n| single_romaji(n))
                .is_some_and(|r| r.starts_with(['a', 'e', 'i', 'o', 'u', 'y']));
            out.push_str(if next_is_vowel_y { "n'" } else { "n" });
            i += 1;
            continue;
        }

        if let Some(rom) = single_romaji(c) {
            out.push_str(rom);
            last_full_vowel = last_vowel(rom);
        } else {
            out.push(c);
        }
        i += 1;
    }

    out
}

fn katakana_to_hiragana(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let code = c as u32;
        match code {
            0x30A1..=0x30F6 => {
                out.push(char::from_u32(code - (0x30A1 - 0x3041)).unwrap_or(c));
            }
            0x30F7 => out.push_str("ゔぁ"),
            0x30F8 => out.push_str("ゔぃ"),
            0x30F9 => out.push_str("ゔぇ"),
            0x30FA => out.push_str("ゔぉ"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katakana_to_hiragana_basic() {
        assert_eq!(katakana_to_hiragana("カタカナ"), "かたかな");
        assert_eq!(katakana_to_hiragana("ア"), "あ");
        assert_eq!(katakana_to_hiragana(""), "");
    }

    #[test]
    fn single_kana_covers_all() {
        assert_eq!(single_romaji('あ'), Some("a"));
        assert_eq!(single_romaji('し'), Some("shi"));
        assert_eq!(single_romaji('ん'), Some("n"));
    }

    #[test]
    fn kana_to_latin_basic() {
        assert_eq!(kana_to_latin("さくら"), "sakura");
        assert_eq!(kana_to_latin("きょう"), "kyou");
        assert_eq!(kana_to_latin("しゃ"), "sha");
        assert_eq!(kana_to_latin("じゅ"), "ju");
        assert_eq!(kana_to_latin("ちょ"), "cho");
    }

    #[test]
    fn kana_to_latin_gemination() {
        assert_eq!(kana_to_latin("きって"), "kitte");
        assert_eq!(kana_to_latin("がっこう"), "gakkou");
        assert_eq!(kana_to_latin("ざっし"), "zasshi");
    }

    #[test]
    fn kana_to_latin_long_vowel() {
        assert_eq!(kana_to_latin("ばー"), "baa");

        assert_eq!(kana_to_latin("こーひ"), "koohi");
    }

    #[test]
    fn kana_to_latin_fu_combo() {
        assert_eq!(kana_to_latin("ふぁ"), "fa");
        assert_eq!(kana_to_latin("ふぃ"), "fi");
        assert_eq!(kana_to_latin("っふぇ"), "ffe");
    }

    #[test]
    fn transliterate_full() {
        let dict = KanjiLookup::load();
        let res = transliterate("こんにちは世界!", &dict);
        assert_eq!(res.hiragana, "こんにちはせかい!");
        assert_eq!(res.romaji, "konnichiha sekai!");
    }

    #[test]
    fn okurigana_gemination_stays_attached() {
        let dict = KanjiLookup::load();

        let res = transliterate("渋谷で待ってる", &dict);
        assert_eq!(res.romaji, "shibuya de matteru");
    }
}
