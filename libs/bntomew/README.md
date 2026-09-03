# bntomew

Transliterate Bangla (Bengali script) into the romanized Bangla that today's
generation actually uses — often called **banglish** (Bangla + English).

## How it works

bntomew does **not** apply strict academic transcription rules, because modern
speakers don't use them. Instead it is built around two layers that mirror how
Roman Bangla is really written:

### 1. Lexicon (data-driven, primary)

A curated dictionary maps whole Bangla words directly to their modern
romanization. This covers the two categories that rules get wrong:

- **Borrowed words** — Bangla words with an English/Latin form that people spell
  in Latin, e.g.

  | Bangla | Romanized |
  |--------|-----------|
  | টেবিল | table |
  | স্কুল | school |
  | চেয়ার | chair |
  | কম্পিউটার | computer |
  | সিনেমা | cinema |

- **Conjunct-final words** — Bangla words where a conjunct ending keeps its
  vowel in casual writing, e.g. রাষ্ট্র → `rashtro`, শব্দ → `shobdo`.

Lexicon words can absorb common bound suffixes (এর, র, তে, কে, ...), so
বাংলাদেশের → `bangladesher`, এশিয়ার → `asiar`.

### 2. Syllable engine (fallback)

Any word not in the lexicon is converted with a rule-based abugida engine
(consonants, matras, independent vowels, and the Bangla inherent vowel `o`).
Bangla drops the word-final inherent vowel, so বাংলা → `bangla` (not
`banglao`).

## Usage

```rust
let res = bntomew::transliterate("বাংলাদেশ দক্ষিণ এশিয়ার একটি স্বাধীন সার্বভৌম রাষ্ট্র।");
assert_eq!(res.romanized, "bangladesh dokkhin asiar ekti shadin sarbovum rashtro.");
assert_eq!(res.bengali, "বাংলাদেশ দক্ষিণ এশিয়ার একটি স্বাধীন সার্বভৌম রাষ্ট্র।");
```

## Example

| Bangla | Romanized |
|--------|-----------|
| আমি ভালো আছি | ami bhalo achi |
| ধন্যবাদ | dhonnobad |
| ভালোবাসা | bhalobasha |
| স্বাধীন সার্বভৌম রাষ্ট্র | shadin sarbovum rashtro |
| বাংলাদেশ | bangladesh |

## License

MIT — see [LICENSE](LICENSE).
