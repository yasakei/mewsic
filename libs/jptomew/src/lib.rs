mod classify;
mod convert;
mod dict;
mod models;

pub use models::{JapaneseCheck, TranslitResult};

use dict::KanjiLookup;
use models::HIRAGANA_END;
use models::HIRAGANA_START;
use models::IDEOGRAPH_END;
use models::IDEOGRAPH_START;
use models::KATAKANA_END;
use models::KATAKANA_START;

static DICT: std::sync::OnceLock<KanjiLookup> = std::sync::OnceLock::new();

fn dictionary() -> &'static KanjiLookup {
    DICT.get_or_init(KanjiLookup::load)
}

pub fn transliterate<S: AsRef<str>>(text: S) -> TranslitResult {
    convert::transliterate(text.as_ref(), dictionary())
}

pub fn detect<S: AsRef<str>>(text: S) -> JapaneseCheck {
    let mut saw_ideograph = false;
    for c in text.as_ref().chars() {
        let code = c as u32;
        if (HIRAGANA_START..=HIRAGANA_END).contains(&code)
            || (KATAKANA_START..=KATAKANA_END).contains(&code)
        {
            return JapaneseCheck::Yes;
        }
        if (IDEOGRAPH_START..=IDEOGRAPH_END).contains(&code) {
            saw_ideograph = true;
        }
    }
    if saw_ideograph {
        JapaneseCheck::Possibly
    } else {
        JapaneseCheck::No
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliterate_sentence() {
        let res = transliterate("こんにちは世界!");
        assert_eq!(res.hiragana, "こんにちはせかい!");
        assert_eq!(res.romaji, "konnichiha sekai!");
    }

    #[test]
    fn transliterate_katakana() {
        let res = transliterate("カタカナ");
        assert_eq!(res.hiragana, "かたかな");
        assert_eq!(res.romaji, "katakana");
    }

    #[test]
    fn transliterate_kanji() {
        let res = transliterate("渋谷");
        assert_eq!(res.hiragana, "しぶや");
        assert_eq!(res.romaji, "shibuya");
    }

    #[test]
    fn detect_works() {
        assert_eq!(detect("Hello"), JapaneseCheck::No);
        assert_eq!(detect("日本"), JapaneseCheck::Possibly);
        assert_eq!(detect("ひらがな"), JapaneseCheck::Yes);
        assert_eq!(detect("カタカナ"), JapaneseCheck::Yes);
    }
}
