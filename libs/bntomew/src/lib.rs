mod consts;
mod convert;
mod dict;

pub use convert::TranslitResult;

pub fn transliterate<S: AsRef<str>>(text: S) -> TranslitResult {
    convert::transliterate(text.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_words() {
        assert_eq!(transliterate("টেবিল").romanized, "table");
        assert_eq!(transliterate("স্কুল").romanized, "school");
        assert_eq!(transliterate("চেয়ার").romanized, "chair");
    }

    #[test]
    fn simple_words() {
        assert_eq!(transliterate("বাংলা").romanized, "bangla");
        assert_eq!(transliterate("আমার").romanized, "amar");
        assert_eq!(transliterate("ধন্যবাদ").romanized, "dhonnobad");
        assert_eq!(transliterate("ভালোবাসা").romanized, "bhalobasha");
        assert_eq!(transliterate("শব্দ").romanized, "shobdo");
        assert_eq!(transliterate("বাংলাদেশ").romanized, "bangladesh");
    }

    #[test]
    fn example_sentence() {
        let res = transliterate("বাংলাদেশ দক্ষিণ এশিয়ার একটি স্বাধীন সার্বভৌম রাষ্ট্র।");
        assert_eq!(
            res.romanized,
            "bangladesh dokkhin asiar ekti shadin sarbovum rashtro."
        );
    }

    #[test]
    fn suffixes() {
        assert_eq!(transliterate("বাংলাদেশের").romanized, "bangladesher");
        assert_eq!(transliterate("এশিয়ার").romanized, "asiar");
        assert_eq!(transliterate("স্কুলের").romanized, "schooler");
    }

    #[test]
    fn latin_passes_through() {
        assert_eq!(transliterate("Hello 123").romanized, "Hello 123");
    }
}
