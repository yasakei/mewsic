# jptomew

Transliterate hiragana, katakana and kanji (Japanese text) into rōmaji (Latin/Roman alphabet).

## Usage

```rust
let res = jptomew::convert("こんにちは世界!");
assert_eq!(res.hiragana, "こんにちはせかい!");
assert_eq!(res.romaji, "konnichiha sekai!");
```

Check if a string contains Japanese characters:

```rust
use jptomew::IsJapanese;
assert_eq!(jptomew::is_japanese("Abc"), IsJapanese::False);
assert_eq!(jptomew::is_japanese("日本"), IsJapanese::Maybe);
assert_eq!(jptomew::is_japanese("ラスト"), IsJapanese::True);
```

## License

MIT — see [LICENSE](LICENSE).
