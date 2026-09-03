use crate::consts::*;
use crate::dict;

pub struct TranslitResult {
    pub bengali: String,
    pub romanized: String,
}

const SUFFIXES: &[(&str, &str)] = &[
    ("এর", "er"),
    ("ের", "er"),
    ("র", "r"),
    ("তে", "te"),
    ("য়", "y"),
    ("কে", "ke"),
    ("ও", "o"),
    ("ই", "i"),
    ("গুলো", "gulo"),
    ("রা", "ra"),
];

pub fn transliterate(input: &str) -> TranslitResult {
    let chars: Vec<char> = input.chars().collect();
    let mut bengali = String::with_capacity(input.len());
    let mut romanized = String::with_capacity(input.len());
    let words = dict::lexicon();

    fn is_bengali(c: char) -> bool {
        (BENGALI_START..=BENGALI_END).contains(&(c as u32))
    }

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        if c == '\u{200C}' || c == '\u{200D}' {
            i += 1;
            continue;
        }

        let mut matched = false;
        if !chars[..i].iter().rev().copied().next().is_some_and(is_bengali) {
            for (word, rom) in &words {
                let wchars: Vec<char> = word.chars().collect();
                if wchars.is_empty() || !chars[i..].starts_with(&wchars) {
                    continue;
                }
                let base_end = i + wchars.len();
                if let Some((suf, suf_rom)) = absorb_suffix(&chars, base_end) {
                    let end = base_end + suf.chars().count();
                    if end >= chars.len() || !is_bengali(chars[end]) {
                        bengali.push_str(word);
                        bengali.push_str(suf);
                        romanized.push_str(rom);
                        romanized.push_str(suf_rom);
                        i = end;
                        matched = true;
                        break;
                    }
                }
                if chars.get(base_end).is_some_and(|&n| is_bengali(n)) {
                    continue;
                }
                bengali.push_str(word);
                romanized.push_str(rom);
                i = base_end;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        if is_bengali(c) {
            let syl = convert_word(&chars, &mut i);
            bengali.push_str(&syl.source);
            romanized.push_str(&syl.roman);
            continue;
        }

        bengali.push(c);
        match c {
            '।' | '॥' => romanized.push('.'),
            '্' => {}
            _ if c.is_whitespace() => romanized.push(' '),
            _ => romanized.push(c),
        }
        i += 1;
    }

    let mut out = String::with_capacity(romanized.len());
    let mut prev_space = false;
    for c in romanized.chars() {
        if c == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    romanized = out;
    let romanized = romanized.trim().to_string();

    TranslitResult {
        bengali,
        romanized,
    }
}

fn absorb_suffix(chars: &[char], idx: usize) -> Option<(&'static str, &'static str)> {
    for (suf, rom) in SUFFIXES {
        let s: Vec<char> = suf.chars().collect();
        if !s.is_empty() && chars[idx..].starts_with(&s) {
            return Some((suf, rom));
        }
    }
    None
}

struct Syl {
    source: String,
    roman: String,
}

#[derive(Clone, Copy)]
enum Elem {
    Consonant(char),
    Vowel(&'static str),
    Independent(&'static str),
    Halant,
    Anusvara,
    Chandrabindu,
    Visarga,
    Digit(char),
    Other(char),
}

fn convert_word(chars: &[char], i: &mut usize) -> Syl {
    let mut source = String::new();
    let mut elems: Vec<Elem> = Vec::with_capacity(8);

    while *i < chars.len() {
        let c = chars[*i];
        let code = c as u32;
        if !(BENGALI_START..=BENGALI_END).contains(&code) {
            break;
        }

        if consonant(c).is_some() {
            elems.push(Elem::Consonant(c));
            source.push(c);
            *i += 1;
            continue;
        }
        if c == HALANT {
            elems.push(Elem::Halant);
            source.push(c);
            *i += 1;
            continue;
        }
        if let Some(m) = matra(c) {
            elems.push(Elem::Vowel(m));
            source.push(c);
            *i += 1;
            continue;
        }
        if let Some(v) = independent(c) {
            elems.push(Elem::Independent(v));
            source.push(c);
            *i += 1;
            continue;
        }
        if c == ANUSVARA {
            elems.push(Elem::Anusvara);
            source.push(c);
            *i += 1;
            continue;
        }
        if c == CHANDRABINDU {
            elems.push(Elem::Chandrabindu);
            source.push(c);
            *i += 1;
            continue;
        }
        if c == VISARGA {
            elems.push(Elem::Visarga);
            source.push(c);
            *i += 1;
            continue;
        }
        if let Some(d) = bengali_digit(c) {
            elems.push(Elem::Digit(d));
            source.push(c);
            *i += 1;
            continue;
        }
        elems.push(Elem::Other(c));
        source.push(c);
        *i += 1;
    }

    let roman = render(&elems);
    Syl { source, roman }
}

struct Syllable {
    base: String,
    explicit: Option<String>,
    head: char,
    inherent: bool,
    conjunct: bool,
    tail: String,
}

impl Syllable {
    fn is_other(&self) -> bool {
        self.base.is_empty() && !self.inherent && self.explicit.is_none()
    }
}

fn render(elems: &[Elem]) -> String {
    let n = elems.len();
    let mut syls: Vec<Syllable> = Vec::new();
    let mut idx = 0;

    while idx < n {
        match elems[idx] {
            Elem::Consonant(_) => {
                let mut cluster: Vec<char> = Vec::new();
                let mut j = idx;
                while let Some(Elem::Consonant(c)) = elems.get(j) {
                    cluster.push(*c);
                    j += 1;
                    match (elems.get(j), elems.get(j + 1)) {
                        (Some(Elem::Halant), Some(Elem::Consonant(_))) => {
                            j += 1;
                        }
                        _ => break,
                    }
                }
                let dead = matches!(elems.get(j), Some(Elem::Halant));
                let explicit = if let Some(Elem::Vowel(m)) = elems.get(j) {
                    Some((*m).to_string())
                } else {
                    None
                };

                let mut syl = Syllable {
                    base: conjunct_roman(&cluster),
                    head: cluster[0],
                    inherent: !dead && explicit.is_none(),
                    conjunct: cluster.len() > 1,
                    explicit,
                    tail: String::new(),
                };
                let mut k = j;
                if syl.explicit.is_some() || dead {
                    k += 1;
                }
                while k < n {
                    match elems[k] {
                        Elem::Anusvara => {
                            syl.tail.push_str("ng");
                            k += 1;
                        }
                        Elem::Chandrabindu => k += 1,
                        Elem::Visarga => k += 1,
                        _ => break,
                    }
                }
                syls.push(syl);

                idx = k;
            }
            Elem::Independent(v) => {
                syls.push(Syllable {
                    base: String::new(),
                    head: '\0',
                    explicit: Some(v.to_string()),
                    inherent: false,
                    conjunct: false,
                    tail: String::new(),
                });
                idx += 1;
            }
            Elem::Vowel(m) => {
                syls.push(Syllable {
                    base: String::new(),
                    head: '\0',
                    explicit: Some(m.to_string()),
                    inherent: false,
                    conjunct: false,
                    tail: String::new(),
                });
                idx += 1;
            }
            Elem::Digit(d) => {
                syls.push(Syllable {
                    base: String::new(),
                    explicit: None,
                    head: '\0',
                    inherent: false,
                    conjunct: false,
                    tail: d.to_string(),
                });
                idx += 1;
            }
            Elem::Anusvara | Elem::Chandrabindu | Elem::Visarga => idx += 1,
            Elem::Other(c) => {
                syls.push(Syllable {
                    base: c.to_string(),
                    head: '\0',
                    explicit: None,
                    inherent: false,
                    conjunct: false,
                    tail: String::new(),
                });
                idx += 1;
            }
            Elem::Halant => idx += 1,
        }
    }

    apply_schwa_deletion(&mut syls);

    let mut out = String::new();
    for s in &syls {
        out.push_str(&s.base);
        match &s.explicit {
            Some(v) if !v.is_empty() => out.push_str(v),
            None if s.inherent => out.push_str(inherent_mid(s.head)),
            _ => {}
        }
        out.push_str(&s.tail);
    }
    out
}

fn apply_schwa_deletion(syls: &mut [Syllable]) {
    let n = syls.len();
    if n == 0 {
        return;
    }

    let mut last_sig = n - 1;
    while last_sig > 0 && syls[last_sig].is_other() {
        last_sig -= 1;
    }

    let mut protect_from_left = false;
    let mut i = last_sig as isize;
    while i >= 0 {
        let this = i as usize;
        if !syls[this].inherent {
            protect_from_left = false;
            i -= 1;
            continue;
        }

        let mut delete = false;

        if this == last_sig {
            if syls[this].conjunct {
                delete = false;
            } else {
                let h = syls[this].head;
                let keep_final = matches!(
                    h,
                    'ল' | 'ট' | 'ত' | '\u{09DC}' | '\u{09DD}'
                ) && n == 2;
                delete = !keep_final;
            }
        } else if !protect_from_left {
            let left_vowel = i > 0 && syls[(i - 1) as usize].has_vowel();
            let right_vowel = i + 1 < n as isize && syls[(i + 1) as usize].has_vowel();
            if left_vowel && right_vowel {
                let schwa_head = syls[this].head;
                let next_head = syls[(i + 1) as usize].head;
                let is_rhotic = matches!(schwa_head, 'র' | '\u{09DC}' | '\u{09DD}');
                let next_is_retroflex =
                    matches!(next_head, 'ট' | 'ঠ' | 'ড' | 'ঢ' | 'ণ' | '\u{09DC}' | '\u{09DD}');
                if !is_rhotic && !next_is_retroflex {
                    delete = true;
                }
            }
        }

        if delete {
            syls[this].explicit = Some(String::new());
            protect_from_left = true;
        } else {
            protect_from_left = false;
        }
        i -= 1;
    }
}

impl Syllable {
    fn has_vowel(&self) -> bool {
        match &self.explicit {
            Some(v) => !v.is_empty(),
            None => self.inherent,
        }
    }
}

fn conjunct_roman(cluster: &[char]) -> String {
    if cluster.len() == 1 {
        return consonant(cluster[0]).unwrap_or("").to_string();
    }

    let last = cluster[cluster.len() - 1];
    if last == 'ব' && cluster.len() == 2 {
        let c1 = cluster[0];
        return match c1 {
            'স' => "sh".to_string(),
            _ => {
                let b1 = consonant(c1).unwrap_or("").to_string();
                let doubled = double_consonant(c1);
                if !doubled.is_empty() {
                    doubled
                } else {
                    format!("{b1}b")
                }
            }
        };
    }

    if last == 'ম' {
        let d = double_consonant(cluster[0]);
        if !d.is_empty() {
            return d;
        }
    }

    if last == '\u{09DF}' || (last == 'য' && cluster.len() == 2) {
        let d = double_consonant(cluster[0]);
        if !d.is_empty() {
            return d;
        }
    }

    if cluster.len() == 2 && cluster[0] == 'চ' && cluster[1] == 'ছ' {
        return "cch".to_string();
    }

    let mut out = String::new();
    for &c in cluster {
        out.push_str(consonant(c).unwrap_or(""));
    }
    out
}

fn double_consonant(c: char) -> String {
    match c {
        'ক' => "kk".into(),
        'খ' => "kkh".into(),
        'গ' => "gg".into(),
        'ঘ' => "ggh".into(),
        'চ' => "chch".into(),
        'ছ' => "chchh".into(),
        'জ' => "jj".into(),
        'ঝ' => "jjh".into(),
        'ট' => "tt".into(),
        'ঠ' => "tth".into(),
        'ড' => "dd".into(),
        'ঢ' => "ddh".into(),
        'ত' => "tt".into(),
        'থ' => "tth".into(),
        'দ' => "dd".into(),
        'ধ' => "ddh".into(),
        'ন' => "nn".into(),
        'প' => "pp".into(),
        'ফ' => "ff".into(),
        'ব' => "bb".into(),
        'ভ' => "bbh".into(),
        'ম' => "mm".into(),
        'র' => "rr".into(),
        'ল' => "ll".into(),
        'শ' => "shsh".into(),
        'ষ' => "shsh".into(),
        'স' => "ss".into(),
        'হ' => "hh".into(),
        _ => String::new(),
    }
}

fn inherent_mid(c: char) -> &'static str {
    match c {
        'ঙ' | 'শ' => "a",
        _ => "o",
    }
}

fn consonant(c: char) -> Option<&'static str> {
    let v = match c {
        'ক' => "k",
        'খ' => "kh",
        'গ' => "g",
        'ঘ' => "gh",
        'ঙ' => "ng",
        'চ' => "ch",
        'ছ' => "ch",
        'জ' => "j",
        'ঝ' => "jh",
        'ঞ' => "n",
        'ট' => "t",
        'ঠ' => "th",
        'ড' => "d",
        'ঢ' => "dh",
        'ণ' => "n",
        'ত' => "t",
        'থ' => "th",
        'দ' => "d",
        'ধ' => "dh",
        'ন' => "n",
        'প' => "p",
        'ফ' => "f",
        'ব' => "b",
        'ভ' => "bh",
        'ম' => "m",
        'য' => "j",
        'র' => "r",
        'ল' => "l",
        'শ' => "sh",
        'ষ' => "sh",
        'স' => "s",
        'হ' => "h",
        '\u{09DC}' => "r",
        '\u{09DD}' => "rh",
        '\u{09DF}' => "y",
        'ৎ' => "t",
        _ => return None,
    };
    Some(v)
}

fn independent(c: char) -> Option<&'static str> {
    let v = match c {
        'অ' => "o",
        'আ' => "a",
        'ই' => "i",
        'ঈ' => "i",
        'উ' => "u",
        'ঊ' => "u",
        'ঋ' => "ri",
        'এ' => "e",
        'ঐ' => "oi",
        'ও' => "o",
        'ঔ' => "ou",
        '\u{09CF}' => "e",
        _ => return None,
    };
    Some(v)
}

fn matra(c: char) -> Option<&'static str> {
    let v = match c {
        'া' => "a",
        'ি' => "i",
        'ী' => "i",
        'ু' => "u",
        'ূ' => "u",
        'ৃ' => "ri",
        'ে' => "e",
        'ৈ' => "oi",
        'ো' => "o",
        'ৌ' => "ou",
        _ => return None,
    };
    Some(v)
}

fn bengali_digit(c: char) -> Option<char> {
    let code = c as u32;
    if (0x09E6..=0x09EF).contains(&code) {
        Some(char::from(b'0' + (code - 0x09E6) as u8))
    } else {
        None
    }
}
