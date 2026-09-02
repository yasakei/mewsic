use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TranslitResult {
    pub hiragana: String,

    pub romaji: String,
}

impl TranslitResult {
    pub(crate) fn with_capacity(n: usize) -> Self {
        Self {
            hiragana: String::with_capacity(n),
            romaji: String::with_capacity(n),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptKind {
    Hiragana,
    Katakana,
    Ideograph,
    Space,
    LeadingPunct,
    TrailingPunct,
    JoiningPunct,
    Numeric,
    Other,
}

impl ScriptKind {
    pub(crate) fn emits_space_after(self) -> bool {
        !matches!(self, ScriptKind::LeadingPunct | ScriptKind::JoiningPunct)
    }

    pub(crate) fn emits_space_before(self) -> bool {
        !matches!(self, ScriptKind::TrailingPunct | ScriptKind::JoiningPunct)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JapaneseCheck {
    No,

    Possibly,

    Yes,
}

impl From<JapaneseCheck> for bool {
    fn from(v: JapaneseCheck) -> Self {
        v != JapaneseCheck::No
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct DictKey<'a>(pub Cow<'a, str>);

impl<'a> DictKey<'a> {
    pub(crate) fn borrowed(s: &'a str) -> Self {
        Self(Cow::Borrowed(s))
    }
}

#[derive(Debug)]
pub(crate) enum Reading {
    Plain { hira: String },
    Conditional { hira: String, tail_byte: u8 },
    Contextual { hira: String, context: String },
}

pub(crate) struct ReadingSlice {
    data: &'static [u8],
    pos: usize,
}

impl ReadingSlice {
    pub(crate) fn new(data: &'static [u8]) -> Option<Self> {
        if data.is_empty() {
            None
        } else {
            Some(Self { data, pos: 0 })
        }
    }
}

impl Iterator for ReadingSlice {
    type Item = Reading;

    fn next(&mut self) -> Option<Self::Item> {
        let mut hira = String::new();
        let mut ctx = String::new();
        let mut tail: Option<u8> = None;
        let mut reading_ctx = false;

        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;

            if b & 0x80 != 0 {
                match b {
                    0x80 => reading_ctx = true,
                    0xFF => break,
                    other => tail = Some(other & 0x7F),
                }
            } else {
                let ch = match b {
                    0x7F => 'ー',
                    _ => char::from_u32(HIRAGANA_START + b as u32).unwrap_or('?'),
                };
                if reading_ctx {
                    ctx.push(ch);
                } else {
                    hira.push(ch);
                }
            }
        }

        if hira.is_empty() {
            return None;
        }

        Some(match tail {
            Some(t) => Reading::Conditional { hira, tail_byte: t },
            None if !ctx.is_empty() => Reading::Contextual { hira, context: ctx },
            _ => Reading::Plain { hira },
        })
    }
}

pub(crate) const HIRAGANA_START: u32 = 0x3041;

pub(crate) const HIRAGANA_END: u32 = 0x3096;

pub(crate) const KATAKANA_START: u32 = 0x30A1;

pub(crate) const KATAKANA_END: u32 = 0x30FA;

pub(crate) const IDEOGRAPH_START: u32 = 0x4E00;

pub(crate) const IDEOGRAPH_END: u32 = 0x9FAF;

pub(crate) const REPETITION_MARK: char = '々';

pub(crate) const LONG_VOWEL: char = 'ー';

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translit_result_capacity() {
        let r = TranslitResult::with_capacity(64);
        assert!(r.hiragana.capacity() >= 64);
        assert!(r.romaji.capacity() >= 64);
    }

    #[test]
    fn script_kind_spacing() {
        assert!(ScriptKind::Hiragana.emits_space_after());
        assert!(ScriptKind::Hiragana.emits_space_before());
        assert!(!ScriptKind::LeadingPunct.emits_space_after());
        assert!(ScriptKind::LeadingPunct.emits_space_before());
        assert!(ScriptKind::TrailingPunct.emits_space_after());
        assert!(!ScriptKind::TrailingPunct.emits_space_before());
    }

    #[test]
    fn japanese_check_into_bool() {
        assert!(!bool::from(JapaneseCheck::No));
        assert!(bool::from(JapaneseCheck::Possibly));
        assert!(bool::from(JapaneseCheck::Yes));
    }
}
