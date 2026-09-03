use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

pub fn romanize(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();

    let has_kana = chars.iter().any(|&c| is_kana(c));
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        if c == '\u{200C}' || c == '\u{200D}' {
            i += 1;
            continue;
        }

        let table = table();

        if is_cjk(c) {
            if let Some((word, romaji)) = table.word_at(&chars, i) {
                out.push_str(romaji);
                i += word.chars().count();
                continue;
            }
        }

        if has_kana && is_japanese(c) {
            let start = i;
            while i < chars.len() && is_japanese(chars[i]) {
                i += 1;
            }
            let segment: &str =
                &text[chars_to_byte_offset(text, start)..chars_to_byte_offset(text, i)];
            out.push_str(&jptomew::transliterate(segment).romaji);
            continue;
        }

        if is_bengali(c) {
            // Bengali is transliterated by bntomew (modern banglish), which
            // replaces the generic Indic abugida table below.
            let start = i;
            while i < chars.len() && is_bengali(chars[i]) {
                i += 1;
            }
            let segment: &str =
                &text[chars_to_byte_offset(text, start)..chars_to_byte_offset(text, i)];
            out.push_str(&bntomew::transliterate(segment).romanized);
            continue;
        }

        if is_arabic(c) {
            let start = i;
            while i < chars.len() && is_arabic(chars[i]) {
                i += 1;
            }
            let segment: &str =
                &text[chars_to_byte_offset(text, start)..chars_to_byte_offset(text, i)];
            out.push_str(&any_ascii::any_ascii(segment));
            continue;
        }

        if let Some(ab) = table.abugida_for(c) {
            let start = i;
            while i < chars.len() && ab.word_char(chars[i]) {
                i += 1;
            }
            out.push_str(&romanize_indic(&chars[start..i], ab));
            continue;
        }

        if is_hangul(c) {
            let start = i;
            while i < chars.len() && is_hangul(chars[i]) {
                i += 1;
            }
            let segment: &str =
                &text[chars_to_byte_offset(text, start)..chars_to_byte_offset(text, i)];
            out.push_str(&any_ascii::any_ascii(segment).to_lowercase());
            continue;
        }

        if let Some(map) = table.chart_map(c) {
            out.push_str(&map);
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn is_kana(c: char) -> bool {
    let code = c as u32;
    matches!(code, 0x3041..=0x3096 | 0x309D..=0x309F | 0x30A1..=0x30FE)
}

fn is_cjk(c: char) -> bool {
    let code = c as u32;
    (0x4E00..=0x9FFF).contains(&code) || (0x3400..=0x4DBF).contains(&code)
}

fn is_japanese(c: char) -> bool {
    is_kana(c) || is_cjk(c) || matches!(c, '々' | '〆' | '〇')
}

fn is_bengali(c: char) -> bool {
    let code = c as u32;
    (0x0980..=0x09FF).contains(&code)
}

fn is_arabic(c: char) -> bool {
    let code = c as u32;
    matches!(
        code,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF
    )
}

fn is_hangul(c: char) -> bool {
    let code = c as u32;
    matches!(
        code,
        0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xD7B0..=0xD7FF
    )
}

fn chars_to_byte_offset(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

static TABLE: std::sync::OnceLock<Table> = std::sync::OnceLock::new();

include!(concat!(env!("OUT_DIR"), "/romanize_index.rs"));

pub fn init(config_dir: &Path) {
    let _ = TABLE.set(Table::load(config_dir));
}

fn table() -> &'static Table {
    TABLE.get_or_init(Table::builtin)
}

#[derive(Debug)]
struct Chart {
    name: String,
    capitalize: bool,
    letters: HashMap<char, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Hindi,
    Bengali,

    Punjabi,
    Plain,
}

#[derive(Debug)]
struct Abugida {
    name: String,
    dialect: Dialect,
    start: u32,
    end: u32,
    punctuation: String,
    halant: char,
    anusvara: char,
    candrabindu: char,
    visarga: char,
    nukta: Option<char>,

    gemination: char,
    inherent: String,
    labials: Vec<char>,
    digits: Vec<char>,
    consonants: HashMap<char, String>,
    vowels: HashMap<char, String>,
    matras: HashMap<char, String>,

    conjuncts: HashMap<String, String>,
    nukta_forms: HashMap<char, char>,

    conjunct_letters: HashMap<char, String>,
}

#[derive(Debug, Default)]
pub struct Table {
    charts: Vec<Chart>,
    abugidas: Vec<Abugida>,

    words: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct RawTable {
    #[serde(default)]
    scripts: Vec<RawScript>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum RawScript {
    Chart {
        name: String,
        #[serde(default)]
        capitalize_first: bool,
        #[serde(default)]
        letters: HashMap<char, String>,
    },

    Words {
        #[serde(rename = "name")]
        _name: String,
        #[serde(default)]
        words: HashMap<String, String>,
    },
    Abugida {
        name: String,
        #[serde(default)]
        dialect: String,
        #[serde(default)]
        range_start: u32,
        #[serde(default)]
        range_end: u32,
        #[serde(default)]
        punctuation: String,
        halant: Option<char>,
        anusvara: Option<char>,
        candrabindu: Option<char>,
        visarga: Option<char>,
        nukta: Option<char>,
        gemination: Option<char>,
        #[serde(default)]
        inherent: String,
        #[serde(default)]
        labials: String,
        #[serde(default)]
        digits: String,
        #[serde(default)]
        consonants: HashMap<char, String>,
        #[serde(default)]
        vowels: HashMap<char, String>,
        #[serde(default)]
        matras: HashMap<char, String>,
        #[serde(default)]
        conjuncts: HashMap<String, String>,
        #[serde(default)]
        nukta_forms: HashMap<char, char>,
        #[serde(default)]
        conjunct_letters: HashMap<char, String>,
    },
}

impl Table {
    fn builtin() -> Table {
        let mut tables: Vec<Table> = Vec::new();
        for (name, raw) in romanize_data() {
            match Table::parse(raw) {
                Ok(t) => tables.push(t),
                Err(e) => panic!("romanize/{name}.toml must parse: {e}"),
            }
        }
        let mut merged = Table::default();
        for t in tables {
            merged.merge(t);
        }
        merged
    }

    pub fn load(config_dir: &Path) -> Table {
        let mut table = Self::builtin();
        let dir = config_dir.join("romanize");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                if path.file_stem().and_then(|s| s.to_str()) == Some("template") {
                    continue;
                }
                if let Ok(raw) = fs::read_to_string(&path) {
                    match Table::parse(&raw) {
                        Ok(user) => table.merge(user),
                        Err(e) => crate::log::write(&format!("ignoring {}: {e}", path.display())),
                    }
                }
            }
        }
        table
    }

    fn parse(raw: &str) -> Result<Table, toml::de::Error> {
        let raw: RawTable = toml::from_str(raw)?;
        let mut table = Table::default();
        for script in raw.scripts {
            match script {
                RawScript::Chart {
                    name,
                    capitalize_first,
                    letters,
                } => table.charts.push(Chart {
                    name,
                    capitalize: capitalize_first,
                    letters,
                }),
                RawScript::Abugida {
                    name,
                    dialect,
                    range_start,
                    range_end,
                    punctuation,
                    halant,
                    anusvara,
                    candrabindu,
                    visarga,
                    nukta,
                    gemination,
                    inherent,
                    labials,
                    digits,
                    consonants,
                    vowels,
                    matras,
                    conjuncts,
                    nukta_forms,
                    conjunct_letters,
                } => table.abugidas.push(Abugida {
                    name,
                    dialect: match dialect.as_str() {
                        "hindi" => Dialect::Hindi,
                        "bengali" => Dialect::Bengali,
                        "punjabi" => Dialect::Punjabi,
                        _ => Dialect::Plain,
                    },
                    start: range_start,
                    end: range_end,
                    punctuation,
                    halant: halant.unwrap_or('\0'),
                    anusvara: anusvara.unwrap_or('\0'),
                    candrabindu: candrabindu.unwrap_or('\0'),
                    visarga: visarga.unwrap_or('\0'),
                    nukta,
                    gemination: gemination.unwrap_or('\0'),
                    inherent,
                    labials: labials.chars().collect(),
                    digits: digits.chars().collect(),
                    consonants,
                    vowels,
                    matras,
                    conjuncts,
                    nukta_forms,
                    conjunct_letters,
                }),
                RawScript::Words { words, .. } => {
                    table.add_words(words);
                }
            }
        }
        Ok(table)
    }

    fn add_words(&mut self, words: HashMap<String, String>) {
        self.words.extend(words);

        self.words
            .sort_by_key(|(word, _)| std::cmp::Reverse(word.chars().count()));
    }

    fn word_at(&self, chars: &[char], i: usize) -> Option<&(String, String)> {
        if self.words.is_empty() {
            return None;
        }
        let tail: String = chars[i..].iter().take(12).collect();
        self.words.iter().find(|(word, _)| tail.starts_with(word))
    }

    fn merge(&mut self, mut user: Table) {
        for chart in user.charts.drain(..) {
            match self.charts.iter_mut().find(|c| c.name == chart.name) {
                Some(base) => base.letters.extend(chart.letters),
                None => self.charts.push(chart),
            }
        }
        for abugida in user.abugidas.drain(..) {
            match self.abugidas.iter_mut().find(|a| a.name == abugida.name) {
                Some(base) => base.overlay(abugida),
                None => self.abugidas.push(abugida),
            }
        }
        if !user.words.is_empty() {
            let words: HashMap<String, String> = user.words.into_iter().collect();
            self.add_words(words);
        }
    }

    fn chart_map(&self, c: char) -> Option<String> {
        for chart in &self.charts {
            if let Some(v) = chart.letters.get(&c) {
                return Some(v.clone());
            }
            if chart.capitalize && c.is_uppercase() {
                if let Some(low) = c.to_lowercase().next() {
                    if let Some(v) = chart.letters.get(&low) {
                        return Some(capitalize_first(v));
                    }
                }
            }
        }
        None
    }

    fn abugida_for(&self, c: char) -> Option<&Abugida> {
        let code = c as u32;
        self.abugidas
            .iter()
            .find(|a| code >= a.start && code <= a.end && !a.punctuation.contains(c))
    }
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::with_capacity(s.len());
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

impl Abugida {
    fn word_char(&self, c: char) -> bool {
        let code = c as u32;
        code >= self.start && code <= self.end && !self.punctuation.contains(c)
    }

    fn consonant(&self, c: char) -> Option<&str> {
        self.consonants.get(&c).map(|s| s.as_str())
    }

    fn vowel(&self, c: char) -> Option<&str> {
        self.vowels.get(&c).map(|s| s.as_str())
    }

    fn matra(&self, c: char) -> Option<&str> {
        self.matras.get(&c).map(|s| s.as_str())
    }

    fn conjunct(&self, c1: char, c2: char) -> Option<&str> {
        let mut key = String::with_capacity(6);
        key.push(c1);
        key.push(self.halant);
        key.push(c2);
        self.conjuncts.get(&key).map(|s| s.as_str())
    }

    fn digit_value(&self, c: char) -> Option<char> {
        self.digits
            .iter()
            .position(|&d| d == c)
            .map(|i| char::from(b'0' + i as u8))
    }

    fn nasal(&self, next_onset: char, candrabindu: bool) -> &'static str {
        if next_onset != '\0' && self.labials.contains(&next_onset) {
            return "m";
        }
        if candrabindu {
            return "n";
        }
        match self.dialect {
            Dialect::Punjabi if next_onset == '\0' => "",
            Dialect::Bengali if next_onset != '\0' => "ng",
            _ => "n",
        }
    }

    fn overlay(&mut self, o: Abugida) {
        if o.start != 0 {
            self.start = o.start;
        }
        if o.end != 0 {
            self.end = o.end;
        }
        if o.halant != '\0' {
            self.halant = o.halant;
        }
        if o.anusvara != '\0' {
            self.anusvara = o.anusvara;
        }
        if o.candrabindu != '\0' {
            self.candrabindu = o.candrabindu;
        }
        if o.visarga != '\0' {
            self.visarga = o.visarga;
        }
        if o.nukta.is_some() {
            self.nukta = o.nukta;
        }
        if o.gemination != '\0' {
            self.gemination = o.gemination;
        }
        if !o.inherent.is_empty() {
            self.inherent = o.inherent;
        }
        if !o.punctuation.is_empty() {
            self.punctuation = o.punctuation;
        }
        if !o.labials.is_empty() {
            self.labials = o.labials;
        }
        if !o.digits.is_empty() {
            self.digits = o.digits;
        }
        self.consonants.extend(o.consonants);
        self.vowels.extend(o.vowels);
        self.matras.extend(o.matras);
        self.conjuncts.extend(o.conjuncts);
        self.nukta_forms.extend(o.nukta_forms);
        self.conjunct_letters.extend(o.conjunct_letters);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Vowel {
    Inherent,

    None,

    Written(String),
}

#[derive(Debug, Clone)]
struct Syl {
    onset: char,
    vowel: Vowel,

    nasal: bool,

    candrabindu: bool,
    visarga: bool,

    special: Option<String>,

    onset_covered: bool,

    geminated: bool,
}

fn romanize_indic(chars: &[char], ab: &Abugida) -> String {
    let mut syls: Vec<Syl> = Vec::with_capacity(chars.len());
    let mut geminate_next = false;
    for &c in chars {
        if c == ab.halant {
            if let Some(last) = syls.last_mut() {
                if last.onset != '\0' {
                    last.vowel = Vowel::None;
                }
            }
            continue;
        }
        if let Some(m) = ab.matra(c) {
            if let Some(last) = syls.last_mut() {
                if last.onset != '\0' {
                    last.vowel = Vowel::Written(m.to_string());
                }
            }
            continue;
        }
        if c == ab.anusvara {
            if let Some(last) = syls.last_mut() {
                last.nasal = true;
            } else {
                syls.push(Syl {
                    onset: '\0',
                    vowel: Vowel::Inherent,
                    nasal: true,
                    candrabindu: false,
                    visarga: false,
                    special: None,
                    onset_covered: false,
                    geminated: false,
                });
            }
            continue;
        }
        if c == ab.candrabindu {
            if let Some(last) = syls.last_mut() {
                last.candrabindu = true;
            } else {
                syls.push(Syl {
                    onset: '\0',
                    vowel: Vowel::Inherent,
                    nasal: false,
                    candrabindu: true,
                    visarga: false,
                    special: None,
                    onset_covered: false,
                    geminated: false,
                });
            }
            continue;
        }
        if c == ab.visarga {
            if let Some(last) = syls.last_mut() {
                last.visarga = true;
            }
            continue;
        }
        if ab.nukta == Some(c) {
            if let Some(last) = syls.last_mut() {
                if last.onset != '\0' {
                    if let Some(composed) = ab.nukta_forms.get(&last.onset) {
                        last.onset = *composed;
                    }
                }
            }
            continue;
        }
        if let Some(v) = ab.vowel(c) {
            syls.push(Syl {
                onset: '\0',
                vowel: Vowel::Written(v.to_string()),
                nasal: false,
                candrabindu: false,
                visarga: false,
                special: None,
                onset_covered: false,
                geminated: false,
            });
            continue;
        }
        if ab.gemination != '\0' && c == ab.gemination {
            geminate_next = true;
            continue;
        }
        if ab.consonant(c).is_some() {
            let mut onset_covered = false;
            if let Some(last) = syls.last_mut() {
                if last.onset != '\0' && matches!(last.vowel, Vowel::None) && last.special.is_none()
                {
                    if let Some(romaji) = ab.conjunct(last.onset, c) {
                        last.special = Some(romaji.to_string());
                        onset_covered = true;
                    }
                }
            }
            syls.push(Syl {
                onset: c,
                vowel: Vowel::Inherent,
                nasal: false,
                candrabindu: false,
                visarga: false,
                special: None,
                onset_covered,
                geminated: std::mem::take(&mut geminate_next),
            });
            continue;
        }
        if let Some(d) = ab.digit_value(c) {
            syls.push(Syl {
                onset: d,
                vowel: Vowel::Written(String::new()),
                nasal: false,
                candrabindu: false,
                visarga: false,
                special: None,
                onset_covered: false,
                geminated: false,
            });
            continue;
        }

        syls.push(Syl {
            onset: c,
            vowel: Vowel::Written(String::new()),
            nasal: false,
            candrabindu: false,
            visarga: false,
            special: None,
            onset_covered: false,
            geminated: false,
        });
    }

    if syls.len() >= 2 {
        let last_idx = syls.len() - 1;
        let conjunct_prev = matches!(syls[last_idx - 1].vowel, Vowel::None);
        let last = &mut syls[last_idx];
        if matches!(last.vowel, Vowel::Inherent) {
            let keep = match ab.dialect {
                Dialect::Punjabi => false,
                Dialect::Bengali => conjunct_prev,
                Dialect::Hindi => conjunct_prev && matches!(last.onset, 'य' | 'व' | 'र'),
                Dialect::Plain => false,
            };
            if !keep {
                last.vowel = Vowel::None;
            }
        }
    }

    let mut out = String::new();
    for (i, syl) in syls.iter().enumerate() {
        let next_onset = syls.get(i + 1).map(|s| s.onset).unwrap_or('\0');
        if let Some(sp) = &syl.special {
            out.push_str(sp);
        } else if !syl.onset_covered && syl.onset != '\0' {
            let romaji = if matches!(syl.vowel, Vowel::None) {
                ab.conjunct_letters
                    .get(&syl.onset)
                    .map(|s| s.as_str())
                    .or_else(|| ab.consonant(syl.onset))
            } else {
                ab.consonant(syl.onset)
            };
            if let Some(romaji) = romaji {
                if syl.geminated {
                    out.push(romaji.chars().next().unwrap_or('\0'));
                }
                out.push_str(romaji);
            } else {
                out.push(syl.onset);
            }
        }
        if syl.vowel == Vowel::Inherent {
            out.push_str(&ab.inherent);
        } else if let Vowel::Written(ref m) = syl.vowel {
            out.push_str(m);
        }

        if !syl.candrabindu && syl.nasal {
            out.push_str(ab.nasal(next_onset, false));
        }
        if syl.visarga {
            out.push('h');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text_passes_through() {
        assert_eq!(romanize("Hello world 123!"), "Hello world 123!");
    }

    #[test]
    fn japanese_kana() {
        assert_eq!(romanize("さくら"), "sakura");
        assert_eq!(romanize("ありがとう"), "arigatou");
        assert_eq!(romanize("にほんご"), "nihongo");
        assert_eq!(romanize("とうきょう"), "toukyou");
        assert_eq!(romanize("しんぶん"), "shinbun");
        assert_eq!(romanize("かんい"), "kan'i");
        assert_eq!(romanize("コーヒー"), "koohii");
    }

    #[test]
    fn japanese_yoon_and_gemination() {
        assert_eq!(romanize("きょう"), "kyou");
        assert_eq!(romanize("しゃ"), "sha");
        assert_eq!(romanize("じゅ"), "ju");
        assert_eq!(romanize("ちょ"), "cho");
        assert_eq!(romanize("きって"), "kitte");
        assert_eq!(romanize("がっこう"), "gakkou");
        assert_eq!(romanize("ざっし"), "zasshi");
        assert_eq!(romanize("ふぁ"), "fa");
    }

    #[test]
    fn korean_hangul() {
        // Korean now via `any_ascii` per user request
        assert_eq!(romanize("안녕하세요"), "annyeonghaseyo");
        assert_eq!(romanize("사랑해"), "salanghae");
        assert_eq!(romanize("감사"), "gamsa");
        assert_eq!(romanize("한국"), "hangug");
    }

    #[test]
    fn cyrillic() {
        assert_eq!(romanize("Привет"), "Privet");
        assert_eq!(romanize("спасибо"), "spasibo");
        assert_eq!(romanize("Москва"), "Moskva");
        assert_eq!(romanize("песня"), "pesnya");
    }

    #[test]
    fn greek() {
        assert_eq!(romanize("Καλημέρα"), "Kalimera");
        assert_eq!(romanize("αγάπη"), "agapi");
    }

    #[test]
    fn arabic_letters() {
        // Arabic now uses `any_ascii` per https://crates.io/crates/any_ascii/0.1.2
        assert_eq!(romanize("كتاب"), "ktb");
        assert_eq!(romanize("سلام"), "slm");
    }

    #[test]
    fn chinese_passes_through() {
        assert_eq!(romanize("晴天"), "晴天");
    }

    #[test]
    fn hindi_words() {
        assert_eq!(romanize("नमस्ते"), "namaste");
        assert_eq!(romanize("किताब"), "kitaab");
        assert_eq!(romanize("कर्म"), "karm");
        assert_eq!(romanize("धर्म"), "dharm");
        assert_eq!(romanize("सत्य"), "satya");
        assert_eq!(romanize("प्यार"), "pyaar");
        assert_eq!(romanize("दिल"), "dil");
        assert_eq!(romanize("हिंदी"), "hindii");
        assert_eq!(romanize("अच्छा"), "achchhaa");
        assert_eq!(romanize("कैसे हो"), "kaise ho");
        assert_eq!(
            romanize("मैं तुमसे प्यार करता हूँ"),
            "main tumase pyaar karataa huu"
        );
    }

    #[test]
    fn hindi_conjuncts_and_nasals() {
        assert_eq!(romanize("राष्ट्र"), "raashtra");
        assert_eq!(romanize("सूत्र"), "suutra");
        assert_eq!(romanize("संग"), "sang");
        assert_eq!(romanize("संभव"), "sambhav");
        assert_eq!(romanize("हैदराबाद"), "haidaraabaad");
        assert_eq!(romanize("लड़का"), "larakaa");
        assert_eq!(romanize("ज़िंदगी"), "zindagii");
        assert_eq!(romanize("संख्या"), "sankhyaa");
    }

    #[test]
    fn bengali_words() {
        assert_eq!(romanize("বাংলা"), "bangla");
        assert_eq!(romanize("ধন্যবাদ"), "dhonnobad");
        assert_eq!(romanize("ভালোবাসা"), "bhalobasha");
        assert_eq!(romanize("শব্দ"), "shobdo");
        assert_eq!(romanize("স্বপ্ন"), "shopno");
        assert_eq!(romanize("বাংলাদেশ"), "bangladesh");
        assert_eq!(romanize("আমার"), "amar");
        assert_eq!(romanize("খুঁজে"), "khuje");
        assert_eq!(romanize("সমুদ্র"), "somudro");
        assert_eq!(romanize("কখন"), "kokhon");
        assert_eq!(romanize("বাস"), "bus");
    }

    #[test]
    fn bengali_real_lyric_lines() {
        assert_eq!(romanize("আমি বাংলায় গান গাই"), "ami banglay gan gai");
        assert_eq!(romanize("আমি আমার আমিকে চিরদিন"), "ami amar amike chirodin");
        assert_eq!(romanize("আমি বাংলায় দেখি স্বপ্ন"), "ami banglay dekhi shopno");
        assert_eq!(romanize("আমি বাংলায় বাঁধি সুর"), "ami banglay badhi sur");
        assert_eq!(romanize("হেঁটেছি এতটা দূর"), "hetechhi etota dur");
    }

    #[test]
    fn punjabi_words() {
        assert_eq!(romanize("ਤੇਰੀ"), "teri");
        assert_eq!(romanize("ਮੇਰਾ"), "mera");
        assert_eq!(romanize("ਦਿਲ"), "dil");
        assert_eq!(romanize("ਮੈਨੂੰ"), "mainu");
        assert_eq!(romanize("ਪੰਜਾਬ"), "panjab");
        assert_eq!(romanize("ਹੋਰ"), "hor");

        assert_eq!(romanize("ਘਰ"), "ghar");
        assert_eq!(romanize("ਕਰ"), "kar");
        assert_eq!(romanize("ਸ਼ਹਿਰ"), "sahir");

        assert_eq!(romanize("ਕੱਕਾ"), "kakka");
        assert_eq!(romanize("ਪੱਖੀ"), "pakkhi");
        assert_eq!(romanize("ਸੱਚਾ"), "saccha");

        assert_eq!(romanize("ਸਤ ਸ੍ਰੀ ਅਕਾਲ"), "sat sri akal");
    }

    #[test]
    fn hindi_gy_conjunct() {
        assert_eq!(romanize("ज्ञान"), "gyaan");
        assert_eq!(romanize("विद्या"), "vidyaa");
    }

    #[test]
    fn indic_digits_and_mixed() {
        assert_eq!(romanize("१२३"), "123");
        assert_eq!(romanize("১২৩৪"), "1234");
        assert_eq!(romanize("गाना २"), "gaanaa 2");
    }

    #[test]
    fn builtin_table_loads_from_toml() {
        let table = Table::builtin();
        // Arabic now handled by `any_ascii` (https://crates.io/crates/any_ascii/0.1.2), not a chart
        assert!(table.charts.len() >= 2);
        assert_eq!(table.abugidas.len(), 2);
        assert_eq!(table.chart_map('а').as_deref(), Some("a"));
        assert_eq!(table.chart_map('Ж').as_deref(), Some("Zh"));
        assert_eq!(
            table.abugida_for('क').map(|a| a.name.as_str()),
            Some("devanagari")
        );
        assert!(
            table.abugida_for('।').is_none(),
            "danda must not start an indic word"
        );
    }

    #[test]
    fn user_toml_overlays_builtin_by_script_name() {
        let dir =
            std::env::temp_dir().join(format!("mewsic-romanize-merge-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("romanize"));

        std::fs::write(
            dir.join("romanize/cyrillic.toml"),
            r#"
[[scripts]]
name = "cyrillic"
kind = "chart"
capitalize_first = true
[scripts.letters]
"х" = "h"
"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("romanize/georgian.toml"),
            r#"
[[scripts]]
name = "georgian"
kind = "chart"
[scripts.letters]
"ა" = "a"
"ბ" = "b"
"გ" = "g"
"დ" = "d"
"ე" = "e"
"#,
        )
        .unwrap();

        let table = Table::load(&dir);

        assert_eq!(table.chart_map('х').as_deref(), Some("h"));
        assert_eq!(table.chart_map('а').as_deref(), Some("a"));

        assert_eq!(table.chart_map('ა').as_deref(), Some("a"));
        assert_eq!(table.chart_map('ბ').as_deref(), Some("b"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_user_toml_is_ignored() {
        let dir = std::env::temp_dir().join(format!("mewsic-romanize-bad-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("romanize"));
        std::fs::write(dir.join("romanize/cyrillic.toml"), "this is { not toml").unwrap();
        let table = Table::load(&dir);
        assert_eq!(table.chart_map('а').as_deref(), Some("a"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod debug_words {
    #[test]
    fn test_word_dict() {
        let cases = vec![
            ("今日", "kyou"),
            ("世界", "sekai"),
            ("愛", "ai"),
            ("愛してる", "aishiteru"),
            ("会いたい", "aitai"),
            ("大丈夫", "daijoubu"),
            ("約束", "yakusoku"),
            ("大切", "taisetsu"),
            ("素晴らしい", "subarashii"),
            ("渋谷で待ってる", "shibuya de matteru"),
            ("儚い想い", "hakanaiomoi"),
        ];
        for (word, expected) in &cases {
            let r = crate::romanize::romanize(word);
            if !expected.is_empty() {
                assert_eq!(&r, expected, "word_at failed for {word}");
            }
            eprintln!("{word} -> {r}");
        }
    }
}

#[cfg(test)]
mod probe_suzume {

    #[test]
    fn probe() {
        let text = match std::fs::read_to_string("/tmp/suzume_lyrics.txt") {
            Ok(t) => t,
            Err(_) => return,
        };
        for line in text.lines() {
            eprintln!("{line}\n    -> {}", crate::romanize::romanize(line));
        }
    }
}
