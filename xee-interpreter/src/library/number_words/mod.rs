mod rbnf;

/// Case control for number word formatting.
#[derive(Clone, Copy)]
pub(crate) enum WordCase {
    Lower,
    Upper,
    Title,
}

/// Format ordinal suffix for decimal numbers: st, nd, rd, th.
pub(crate) fn ordinal_suffix(number: i64) -> &'static str {
    let penult = ((number % 100) / 10) as i64;
    let ult = (number % 10) as i64;
    if penult == 1 {
        "th" // 11th, 12th, 13th
    } else {
        match ult {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    }
}

/// Dispatch number-to-words based on language, using CLDR RBNF rules.
pub(crate) fn number_to_words_lang(
    number: i64,
    case: WordCase,
    lang: Option<&str>,
    ordinal: Option<&str>,
) -> String {
    // Convert BCP 47 tag (hyphens) to CLDR key (underscores) for lookup.
    // The RBNF engine tries the full locale (e.g. "de_CH") first, then
    // falls back to the base language ("de"), then to English.
    let lang_code = lang
        .map(|s| s.replace('-', "_"))
        .unwrap_or_else(|| "en".to_string());

    let ruleset = if let Some(ord) = ordinal {
        if ord.starts_with('%') {
            // Direct CLDR scheme name (e.g. "%spellout-ordinal-masculine")
            ord
        } else {
            ordinal_ruleset(ord)
        }
    } else {
        // Use verbose form to include "and" (e.g. "one hundred and two").
        // Falls back to non-verbose if verbose variant doesn't exist.
        "%spellout-cardinal-verbose"
    };

    let result = format_with_verbose_fallback(&lang_code, ruleset, number);

    // CLDR RBNF uses hyphens and soft hyphens (U+00AD) as word separators.
    // XSLT conformance tests expect spaces ("twenty one"), so replace both.
    let result = result.replace('\u{00AD}', " ").replace('-', " ");

    match case {
        WordCase::Lower => result.to_lowercase(),
        WordCase::Upper => result.to_uppercase(),
        WordCase::Title => title_case_words(&result),
    }
}

/// Map an XSLT ordinal attribute value to a CLDR RBNF ruleset name.
///
/// German uses inflection suffixes like "-e", "-er", "-es", "-en":
///   - "-e" → "%spellout-ordinal" (base form)
///   - "-er" → "%spellout-ordinal-r" (masculine nominative)
///   - "-es" → "%spellout-ordinal-s" (neuter)
///   - "-en" → "%spellout-ordinal-n" (genitive/dative/plural)
///   - "-em" → "%spellout-ordinal-m" (dative)
///
/// For simple "yes" or other values, defaults to "%spellout-ordinal" with
/// masculine fallback handled by the RBNF engine.
fn ordinal_ruleset(ordinal: &str) -> &'static str {
    if let Some(suffix) = ordinal.strip_prefix("-e") {
        match suffix {
            "" => "%spellout-ordinal",
            "r" => "%spellout-ordinal-r",
            "s" => "%spellout-ordinal-s",
            "n" => "%spellout-ordinal-n",
            "m" => "%spellout-ordinal-m",
            _ => "%spellout-ordinal",
        }
    } else {
        // Default: "yes" or unspecified → try verbose ordinal first to include
        // "and" (e.g. "one hundred and fifteenth"). The caller will fall back
        // to non-verbose if this ruleset doesn't exist for the language.
        "%spellout-ordinal-verbose"
    }
}

/// Format a number using the given RBNF ruleset, falling back from a verbose
/// variant to its non-verbose equivalent when the language doesn't support it.
///
/// CLDR only provides `-verbose` rulesets for some languages (e.g. English).
/// When the verbose ruleset isn't found, RBNF returns the raw number as a
/// string — we detect that and retry with the base ruleset.
fn format_with_verbose_fallback(lang: &str, ruleset: &str, number: i64) -> String {
    let result = rbnf::format_number(lang, ruleset, number);
    if result == number.to_string() {
        let fallback = match ruleset {
            "%spellout-ordinal-verbose" => Some("%spellout-ordinal"),
            "%spellout-cardinal-verbose" => Some("%spellout-cardinal"),
            _ => None,
        };
        if let Some(fb) = fallback {
            return rbnf::format_number(lang, fb, number);
        }
    }
    result
}

/// Capitalize first character, leave the rest unchanged.
fn title_case_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result = c.to_uppercase().to_string();
            result.extend(chars);
            result
        }
    }
}

/// Words that should not be capitalized in title case (conjunctions).
const TITLE_CASE_STOPWORDS: &[&str] = &["and", "und"];

/// Capitalize the first character of each word, except stopwords (unless first).
fn title_case_words(s: &str) -> String {
    s.split(' ')
        .enumerate()
        .map(|(i, word)| {
            if i > 0 && TITLE_CASE_STOPWORDS.contains(&word) {
                word.to_string()
            } else {
                title_case_first(word)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_cardinal_zero() {
        assert_eq!(
            number_to_words_lang(0, WordCase::Lower, Some("en"), None),
            "zero"
        );
    }

    #[test]
    fn en_cardinal_thirteen() {
        assert_eq!(
            number_to_words_lang(13, WordCase::Lower, Some("en"), None),
            "thirteen"
        );
    }

    #[test]
    fn en_cardinal_twenty_one() {
        assert_eq!(
            number_to_words_lang(21, WordCase::Lower, Some("en"), None),
            "twenty one"
        );
    }

    #[test]
    fn en_cardinal_115() {
        assert_eq!(
            number_to_words_lang(115, WordCase::Lower, Some("en"), None),
            "one hundred and fifteen"
        );
    }

    #[test]
    fn en_cardinal_million() {
        let r = number_to_words_lang(2134816, WordCase::Title, Some("en"), None);
        // Verbose form: includes commas and "and"
        assert!(r.starts_with("Two Million"), "got: {}", r);
    }

    #[test]
    fn en_ordinal_first() {
        assert_eq!(
            number_to_words_lang(1, WordCase::Lower, Some("en"), Some("yes")),
            "first"
        );
    }

    #[test]
    fn en_ordinal_twelfth() {
        assert_eq!(
            number_to_words_lang(12, WordCase::Lower, Some("en"), Some("yes")),
            "twelfth"
        );
    }

    #[test]
    fn en_ordinal_twentieth() {
        assert_eq!(
            number_to_words_lang(20, WordCase::Lower, Some("en"), Some("yes")),
            "twentieth"
        );
    }

    #[test]
    fn en_ordinal_twenty_first() {
        assert_eq!(
            number_to_words_lang(21, WordCase::Lower, Some("en"), Some("yes")),
            "twenty first"
        );
    }

    #[test]
    fn en_ordinal_hundredth() {
        assert_eq!(
            number_to_words_lang(100, WordCase::Lower, Some("en"), Some("yes")),
            "one hundredth"
        );
    }

    #[test]
    fn en_ordinal_115() {
        assert_eq!(
            number_to_words_lang(115, WordCase::Lower, Some("en"), Some("yes")),
            "one hundred and fifteenth"
        );
    }

    #[test]
    fn de_cardinal_three() {
        assert_eq!(
            number_to_words_lang(3, WordCase::Lower, Some("de"), None),
            "drei"
        );
    }

    #[test]
    fn de_cardinal_twenty_one() {
        let result = number_to_words_lang(21, WordCase::Lower, Some("de"), None);
        assert!(
            result.contains("ein") && result.contains("zwanzig"),
            "got: {}",
            result
        );
    }

    #[test]
    fn de_ordinal_first() {
        // Default ordinal (no inflection suffix) uses base form
        let result = number_to_words_lang(1, WordCase::Lower, Some("de"), Some("yes"));
        assert!(result.contains("erst"), "got: {}", result);
    }

    #[test]
    fn de_ordinal_inflection_e() {
        // ordinal="-e" → base form
        assert_eq!(
            number_to_words_lang(3, WordCase::Lower, Some("de"), Some("-e")),
            "dritte"
        );
    }

    #[test]
    fn de_ordinal_inflection_er() {
        // ordinal="-er" → masculine
        assert_eq!(
            number_to_words_lang(10, WordCase::Lower, Some("de"), Some("-er")),
            "zehnter"
        );
    }

    #[test]
    fn de_ordinal_inflection_es() {
        // ordinal="-es" → neuter
        assert_eq!(
            number_to_words_lang(13, WordCase::Lower, Some("de"), Some("-es")),
            "dreizehntes"
        );
    }

    #[test]
    fn de_ordinal_inflection_en() {
        // ordinal="-en" → genitive/plural
        let result = number_to_words_lang(20, WordCase::Lower, Some("de"), Some("-en"));
        assert!(result.contains("zwanzigsten"), "got: {}", result);
    }

    #[test]
    fn de_ordinal_2134816_er() {
        // Conformance test 0813: ordinal="-er" format="Ww" lang="de"
        let result = number_to_words_lang(2134816, WordCase::Title, Some("de"), Some("-er"));
        // Soft hyphens become spaces, title case applied (skipping "und")
        assert!(result.contains("Millionen"), "got: {}", result);
        assert!(result.ends_with("Sechzehnter"), "got: {}", result);
        assert!(result.contains(" und "), "und should be lowercase, got: {}", result);
    }

    #[test]
    fn case_upper() {
        assert_eq!(
            number_to_words_lang(3, WordCase::Upper, Some("en"), None),
            "THREE"
        );
    }

    #[test]
    fn case_title() {
        assert_eq!(
            number_to_words_lang(3, WordCase::Title, Some("en"), None),
            "Three"
        );
    }

    #[test]
    fn unknown_lang_falls_back_to_english() {
        assert_eq!(
            number_to_words_lang(3, WordCase::Lower, Some("xx"), None),
            "three"
        );
    }

    #[test]
    fn none_lang_is_english() {
        assert_eq!(
            number_to_words_lang(42, WordCase::Lower, None, None),
            "forty two"
        );
    }

    #[test]
    fn de_cardinal_2134816_title() {
        let r = number_to_words_lang(2134816, WordCase::Title, Some("de"), None);
        // Conformance test 0812 expects something matching:
        // Zwei Millionen Einhundertvierunddreißigtausendachthundertsechzehn
        assert!(r.contains("illionen") || r.contains("illion"), "got: {}", r);
    }

    #[test]
    fn de_ordinal_10() {
        let r = number_to_words_lang(10, WordCase::Lower, Some("de"), Some("yes"));
        // Conformance test 0813 expects "zehnter"
        assert!(r.contains("zehnt"), "got: {}", r);
    }

    #[test]
    fn it_ordinal_first() {
        let r = number_to_words_lang(1, WordCase::Lower, Some("it"), Some("yes"));
        // Conformance test 0829 expects "primo"
        assert!(r.contains("prim"), "got: {}", r);
    }

    #[test]
    fn it_ordinal_second() {
        let r = number_to_words_lang(2, WordCase::Lower, Some("it"), Some("yes"));
        assert!(r.contains("second"), "got: {}", r);
    }
}
