# bntomew

Bangla to romanized transliteration for modern usage.

`bntomew` converts Bengali script to the romanized form commonly used in contemporary writing, often referred to as banglish. It prioritizes how the language is currently written over strict academic transcription.

## Features

* Lexicon first. High frequency words map directly to their established romanized forms. This handles borrowed terms and conjunct sensitive vocabulary.
* Syllable engine fallback. Unknown words are processed through a rule based abugida engine that handles consonants, vowel signs, independent vowels and the inherent vowel.
* Suffix aware. Common bound suffixes such as `এর`, `র`, `তে`, `কে` are recognized when attached to lexicon entries.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
bntomew = { path = "libs/bntomew" }
```

Requires Rust 1.70 or later. Edition 2021.

## Usage

```rust
use bntomew::transliterate;

let result = transliterate("বাংলাদেশ দক্ষিণ এশিয়ার একটি স্বাধীন সার্বভৌম রাষ্ট্র।");
assert_eq!(result.romanized, "bangladesh dokkhin asiar ekti shadin sarbovum rashtro.");
assert_eq!(result.bengali, "বাংলাদেশ দক্ষিণ এশিয়ার একটি স্বাধীন সার্বভৌম রাষ্ট্র।");

let result = transliterate("আমি ভালো আছি");
assert_eq!(result.romanized, "ami bhalo achi");
```

`transliterate` returns `TranslitResult` with fields `bengali` and `romanized`. Input may contain mixed scripts. Non Bengali content passes through unchanged.

## Examples

| Bengali | Romanized |
|---|---|
| আমি ভালো আছি | ami bhalo achi |
| ধন্যবাদ | dhonnobad |
| ভালোবাসা | bhalobasha |
| স্বাধীন সার্বভৌম রাষ্ট্র | shadin sarbovum rashtro |
| বাংলাদেশ | bangladesh |

## Project Structure

* `src/consts.rs` script and sign constants
* `src/dict.rs` lexicon data
* `src/convert.rs` transliteration engine
* `src/lib.rs` public API

## Testing

```sh
cargo test -p bntomew
```

## License

MIT. See [LICENSE](LICENSE).
