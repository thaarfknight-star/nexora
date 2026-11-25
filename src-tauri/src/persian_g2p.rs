//! **Nexora Persian G2P & Pronunciation Enhancer**
//!
//! Pure-Rust, zero-dependency Persian text normalizer and pronunciation guide
//! placed directly in front of Piper TTS (`espeak-ng`) to dramatically improve
//! naturalness for TV series, YouTube videos, lectures, and games:
//! - Converts Western/Persian/Arabic numerals (`$100,000`, `25%`, `2026`) into
//!   natural spoken Persian words (`صد هزار دلار`, `بیست و پنج درصد`).
//! - Transliterates common English tech, gaming, and YouTube terms (`Nexora`,
//!   `YouTube`, `MrBeast`, `GPU`, `FPS`, `Discord`, `AI`, etc.) into phonetic Persian.
//! - Inserts Persian Ezafe (`ِ`) on high-frequency prepositions & construct nouns
//!   so `espeak-ng` links phrases naturally instead of pausing awkwardly.
//! - Normalizes Arabic character variants, ZWNJ, and punctuation pauses.

/// Enhances raw Persian text for natural neural TTS pronunciation.
pub fn enhance_persian_for_tts(raw: &str) -> String {
    let normalized = normalize_chars(raw);
    let with_terms = transliterate_common_terms(&normalized);
    let with_nums = expand_numbers_in_text(&with_terms);
    let with_ezafe = apply_ezafe_hints(&with_nums);

    let trimmed = with_ezafe.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if !matches!(trimmed.chars().last(), Some('.' | '!' | '?' | '؟' | '،')) {
        format!("{trimmed}.")
    } else {
        trimmed.to_string()
    }
}

fn normalize_chars(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    let mut prev_space = true;
    for ch in raw.chars() {
        let mapped = match ch {
            'ي' | 'ى' => 'ی',
            'ك' => 'ک',
            'ة' => 'ه',
            'أ' | 'إ' => 'ا',
            '۰' | '٠' => '0',
            '۱' | '١' => '1',
            '۲' | '٢' => '2',
            '۳' | '٣' => '3',
            '۴' | '٤' => '4',
            '۵' | '٥' => '5',
            '۶' | '٦' => '6',
            '۷' | '٧' => '7',
            '۸' | '٨' => '8',
            '۹' | '٩' => '9',
            '\u{200b}' | '\u{feff}' => continue,
            '[' | ']' | '(' | ')' | '"' | '«' | '»' | '*' | '#' | '_' => ' ',
            ',' => '،',
            '?' => '؟',
            _ => ch,
        };
        if mapped.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn transliterate_common_terms(text: &str) -> String {
    const TERMS: &[(&str, &str)] = &[
        ("nexora", "نُکسورا"),
        ("youtube", "یوتیوب"),
        ("mrbeast", "مِستر بیست"),
        ("discord", "دیسکورد"),
        ("spotify", "اسپاتیفای"),
        ("windows", "ویندوز"),
        ("google", "گوگل"),
        ("github", "گیت‌هاب"),
        ("nvidia", "انویدیا"),
        ("whisper", "ویسپر"),
        ("gemini", "جمنای"),
        ("chatgpt", "چت‌جی‌پی‌تی"),
        ("minecraft", "ماینکرفت"),
        ("fortnite", "فورتنایت"),
        ("valorant", "ولورانت"),
        ("steam", "استیم"),
        ("twitch", "توییچ"),
        ("netflix", "نتفلیکس"),
        ("tedx", "تِداِکس"),
        ("ted", "تِد"),
        ("gpu", "جی‌پی‌یو"),
        ("cpu", "سی‌پی‌یو"),
        ("ram", "رَم"),
        ("fps", "اِف‌پی‌اِس"),
        ("ai", "اِی‌آی"),
        ("ok", "اوکی"),
        ("okay", "اوکی"),
        ("gg", "جی‌جی"),
        ("bro", "داداش"),
        ("boss", "باس"),
    ];

    let mut out_words = Vec::new();
    for token in text.split_whitespace() {
        let stripped = token.trim_matches(|c: char| !c.is_alphanumeric());
        let lower = stripped.to_ascii_lowercase();
        if let Some((_, fa)) = TERMS.iter().find(|(en, _)| *en == lower) {
            let replaced = token.replacen(stripped, fa, 1);
            out_words.push(replaced);
        } else {
            out_words.push(token.to_string());
        }
    }
    out_words.join(" ")
}

fn expand_numbers_in_text(text: &str) -> String {
    let mut out_words = Vec::new();
    for token in text.split_whitespace() {
        let has_dollar = token.starts_with('$') || token.ends_with('$');
        let has_percent = token.ends_with('%') || token.starts_with('%');
        let core = token
            .trim_matches(|c: char| matches!(c, '$' | '%' | '،' | '.' | '!' | '؟'))
            .replace(['،', ','], "");
        if !core.is_empty() && core.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = core.parse::<u64>() {
                if n <= 999_999_999 {
                    let mut phrase = number_to_persian(n);
                    if has_dollar {
                        phrase.push_str(" دلار");
                    } else if has_percent {
                        phrase.push_str(" درصد");
                    }
                    if let Some(last) = token.chars().last() {
                        if matches!(last, '.' | '!' | '؟' | '،') {
                            phrase.push(last);
                        }
                    }
                    out_words.push(phrase);
                    continue;
                }
            }
        }
        out_words.push(token.to_string());
    }
    out_words.join(" ")
}

/// Converts a non-negative integer (`0..=999_999_999`) to natural Persian words.
pub fn number_to_persian(n: u64) -> String {
    if n == 0 {
        return "صفر".to_string();
    }
    const ONES: &[&str] = &[
        "", "یک", "دو", "سه", "چهار", "پنج", "شش", "هفت", "هشت", "نه",
    ];
    const TEENS: &[&str] = &[
        "ده",
        "یازده",
        "دوازده",
        "سیزده",
        "چهارده",
        "پانزده",
        "شانزده",
        "هفده",
        "هجده",
        "نوزده",
    ];
    const TENS: &[&str] = &[
        "",
        "",
        "بیست",
        "سی",
        "چهل",
        "پنجاه",
        "شصت",
        "هفتاد",
        "هشتاد",
        "نود",
    ];
    const HUNDREDS: &[&str] = &[
        "",
        "صد",
        "دویست",
        "سیصد",
        "چهارصد",
        "پانصد",
        "ششصد",
        "هفتصد",
        "هشتصد",
        "نهصد",
    ];

    fn under_1000(v: u64) -> String {
        let mut parts = Vec::new();
        let h = (v / 100) as usize;
        let rem = v % 100;
        if h > 0 {
            parts.push(HUNDREDS[h]);
        }
        if (10..20).contains(&rem) {
            parts.push(TEENS[(rem - 10) as usize]);
        } else {
            let t = (rem / 10) as usize;
            let o = (rem % 10) as usize;
            if t > 0 {
                parts.push(TENS[t]);
            }
            if o > 0 {
                parts.push(ONES[o]);
            }
        }
        parts.join(" و ")
    }

    let mut groups = Vec::new();
    let millions = n / 1_000_000;
    let thousands = (n % 1_000_000) / 1_000;
    let rest = n % 1_000;

    if millions > 0 {
        groups.push(format!("{} میلیون", under_1000(millions)));
    }
    if thousands > 0 {
        if thousands == 1 {
            groups.push("هزار".to_string());
        } else {
            groups.push(format!("{} هزار", under_1000(thousands)));
        }
    }
    if rest > 0 {
        groups.push(under_1000(rest));
    }
    groups.join(" و ")
}

/// Inserts Persian Ezafe (`ِ`) on common construct prepositions & nouns when
/// followed by another Persian word, improving `espeak-ng` phrasing.
fn apply_ezafe_hints(text: &str) -> String {
    const EZAFE_TRIGGERS: &[&str] = &[
        "برای",
        "توی",
        "روی",
        "زیر",
        "پشت",
        "جلوی",
        "کنار",
        "وسط",
        "داخل",
        "بین",
        "سمت",
        "طرف",
        "موقع",
        "زمان",
        "دلیل",
        "علت",
        "اصل",
        "نوع",
        "شکل",
        "حالت",
        "کیفیت",
        "سرعت",
        "صدای",
        "بازی",
        "ویدیوی",
        "قسمت",
        "شروع",
        "پایان",
        "بخش",
        "نوبت",
        "دنبال",
        "مقابل",
    ];

    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return text.to_string();
    }

    let mut out = Vec::with_capacity(words.len());
    for i in 0..words.len() {
        let w = words[i];
        let has_next_word = i + 1 < words.len()
            && !matches!(
                words[i + 1],
                "و" | "یا" | "که" | "را" | "رو" | "از" | "به" | "در" | "با" | "است" | "هست"
            )
            && !matches!(w.chars().last(), Some('.' | '!' | '؟' | '،'));

        if has_next_word && EZAFE_TRIGGERS.contains(&w) {
            out.push(format!("{w}\u{0650}"));
        } else {
            out.push(w.to_string());
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_numbers_and_currency_to_persian_words() {
        assert_eq!(number_to_persian(0), "صفر");
        assert_eq!(number_to_persian(100_000), "صد هزار");
        assert_eq!(number_to_persian(2026), "دو هزار و بیست و شش");
        let s = enhance_persian_for_tts("جایزه $50,000 و 25% شانس");
        assert!(s.contains("پنجاه هزار دلار"));
        assert!(s.contains("بیست و پنج درصد"));
    }

    #[test]
    fn transliterates_common_youtube_and_gaming_terms() {
        let s = enhance_persian_for_tts("سلام به Nexora در YouTube با FPS بالا");
        assert!(s.contains("نُکسورا"));
        assert!(s.contains("یوتیوب"));
        assert!(s.contains("اِف‌پی‌اِس"));
    }

    #[test]
    fn adds_ezafe_mark_on_construct_prepositions() {
        let s = enhance_persian_for_tts("داخل دایره بمان");
        assert!(s.contains("داخل\u{0650} دایره"));
    }
}
