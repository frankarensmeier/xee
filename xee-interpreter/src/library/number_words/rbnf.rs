//! CLDR Rule-Based Number Format (RBNF) interpreter.
//!
//! Parses and evaluates RBNF rules from CLDR XML data files to produce
//! spelled-out number words in any supported language. Adding a new language
//! requires only dropping the CLDR XML file into `data/rbnf/`.
//!
//! Reference: <https://unicode.org/reports/tr35/tr35-numbers.html#Rule-Based_Number_Formatting>

use std::collections::HashMap;
use std::sync::OnceLock;

// Embed CLDR RBNF data at compile time.
// To add a language, download the XML from
//   https://github.com/unicode-org/cldr/tree/main/common/rbnf
// and add an entry here.
const RBNF_DATA: &[(&str, &str)] = &[
    ("en", include_str!("../../../data/rbnf/en.xml")),
    ("de", include_str!("../../../data/rbnf/de.xml")),
    ("fr", include_str!("../../../data/rbnf/fr.xml")),
    ("it", include_str!("../../../data/rbnf/it.xml")),
];

/// Word case (re-exported from parent module).
#[derive(Clone, Copy)]
pub(crate) enum WordCase {
    Lower,
    Upper,
    Title,
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Rule {
    /// The threshold value for this rule (e.g. 0, 1, 20, 100, 1000).
    value: i64,
    /// Optional radix override (from "N/R:" syntax). If None, computed automatically.
    radix: Option<i64>,
    /// The rule body text.
    body: String,
}

#[derive(Debug, Clone)]
struct RuleSet {
    name: String,
    negative_rule: Option<String>,
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct LangData {
    rulesets: HashMap<String, RuleSet>,
}

// ---------------------------------------------------------------------------
// Global cache
// ---------------------------------------------------------------------------

fn global_lang_data() -> &'static HashMap<String, LangData> {
    static CACHE: OnceLock<HashMap<String, LangData>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        for &(lang, xml) in RBNF_DATA {
            if let Some(data) = parse_rbnf_xml(xml) {
                map.insert(lang.to_string(), data);
            }
        }
        map
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Format a number using RBNF rules for the given language and ruleset.
/// Falls back to English if the language is not available.
/// Falls back to decimal if the ruleset is not found.
pub(super) fn format_number(lang: &str, ruleset: &str, number: i64) -> String {
    let langs = global_lang_data();
    let data = langs.get(lang).or_else(|| langs.get("en"));
    let data = match data {
        Some(d) => d,
        None => return number.to_string(),
    };

    // Try exact ruleset name, with fallbacks
    let rs = find_ruleset(data, ruleset);
    let rs = match rs {
        Some(r) => r,
        None => return number.to_string(),
    };

    let mut buf = String::new();
    eval_ruleset(data, rs, number, &mut buf, 0);
    // Replace soft hyphens (U+00AD) with nothing (they're optional joiners in CLDR)
    buf.replace('\u{00AD}', "")
}

// ---------------------------------------------------------------------------
// Ruleset lookup
// ---------------------------------------------------------------------------

fn find_ruleset<'a>(data: &'a LangData, name: &str) -> Option<&'a RuleSet> {
    // Direct match
    if let Some(rs) = data.rulesets.get(name) {
        return Some(rs);
    }
    // Fallback chains
    match name {
        "%spellout-cardinal-verbose" => data
            .rulesets
            .get("%spellout-cardinal")
            .or_else(|| data.rulesets.get("%spellout-numbering")),
        "%spellout-cardinal" => data.rulesets.get("%spellout-numbering"),
        "%spellout-ordinal" => data.rulesets.get("%spellout-ordinal-masculine"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Rule matching
// ---------------------------------------------------------------------------

/// Find the rule that applies for a given number within a ruleset.
fn find_rule(rs: &RuleSet, number: i64) -> &Rule {
    // Rules are sorted by value; find the last one where value <= number
    let mut best = &rs.rules[0];
    for rule in &rs.rules {
        if rule.value <= number {
            best = rule;
        } else {
            break;
        }
    }
    best
}

/// Compute the effective radix/divisor for a rule.
fn rule_divisor(rule: &Rule) -> i64 {
    if let Some(r) = rule.radix {
        return r;
    }
    if rule.value == 0 {
        return 1;
    }
    // Find the largest power of 10 that is <= rule.value
    let mut d: i64 = 1;
    while d * 10 <= rule.value {
        d *= 10;
    }
    d
}

// ---------------------------------------------------------------------------
// Evaluation engine
// ---------------------------------------------------------------------------

const MAX_DEPTH: usize = 50;

fn eval_ruleset(data: &LangData, rs: &RuleSet, number: i64, buf: &mut String, depth: usize) {
    if depth > MAX_DEPTH {
        buf.push_str(&number.to_string());
        return;
    }

    // Handle negative numbers
    if number < 0 {
        if let Some(ref neg) = rs.negative_rule {
            eval_body(data, rs, neg, -number, -number, 1, buf, depth);
        } else {
            buf.push('-');
            eval_ruleset(data, rs, -number, buf, depth);
        }
        return;
    }

    let rule = find_rule(rs, number);
    let divisor = rule_divisor(rule);

    let quotient = if divisor > 0 { number / divisor } else { number };
    let remainder = if divisor > 0 { number % divisor } else { 0 };

    eval_body(data, rs, &rule.body, quotient, remainder, divisor, buf, depth);
}

fn eval_body(
    data: &LangData,
    current_rs: &RuleSet,
    body: &str,
    quotient: i64,
    remainder: i64,
    divisor: i64,
    buf: &mut String,
    depth: usize,
) {
    let chars: Vec<char> = body.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];
        match ch {
            '\'' => {
                // RBNF quoting: '' → literal apostrophe, single ' → skip (escape marker)
                if i + 1 < len && chars[i + 1] == '\'' {
                    buf.push('\'');
                    i += 2;
                } else {
                    // Skip the quote marker — it just marks the following text as literal
                    i += 1;
                }
            }
            '>' => {
                // >> — substitute remainder
                if i + 1 < len && chars[i + 1] == '>' {
                    // >> — format remainder using current ruleset
                    eval_ruleset(data, current_rs, remainder, buf, depth + 1);
                    i += 2;
                } else if i + 1 < len && chars[i + 1] == '%' {
                    // >%ruleset> — format remainder using named ruleset
                    let end = find_char(&chars, '>', i + 2);
                    let name: String = chars[i + 1..end].iter().collect();
                    if let Some(rs) = data.rulesets.get(&name) {
                        eval_ruleset(data, rs, remainder, buf, depth + 1);
                    } else {
                        buf.push_str(&remainder.to_string());
                    }
                    i = end + 1;
                } else {
                    buf.push(ch);
                    i += 1;
                }
            }
            '<' => {
                // << — substitute quotient
                if i + 1 < len && chars[i + 1] == '<' {
                    eval_ruleset(data, current_rs, quotient, buf, depth + 1);
                    i += 2;
                } else if i + 1 < len && chars[i + 1] == '%' {
                    // <%ruleset< — format quotient using named ruleset
                    let end = find_char(&chars, '<', i + 2);
                    let name: String = chars[i + 1..end].iter().collect();
                    if let Some(rs) = data.rulesets.get(&name) {
                        eval_ruleset(data, rs, quotient, buf, depth + 1);
                    } else {
                        buf.push_str(&quotient.to_string());
                    }
                    i = end + 1;
                } else {
                    buf.push(ch);
                    i += 1;
                }
            }
            '=' => {
                // =%ruleset= or =0.0= or =#,##0=
                let end = find_char(&chars, '=', i + 1);
                let inner: String = chars[i + 1..end].iter().collect();
                if inner.starts_with('%') {
                    // Delegate to another ruleset with the original number
                    let orig_number = quotient * divisor + remainder;
                    if let Some(rs) = data.rulesets.get(&inner) {
                        eval_ruleset(data, rs, orig_number, buf, depth + 1);
                    } else {
                        buf.push_str(&orig_number.to_string());
                    }
                } else if inner.starts_with('#') || inner.starts_with('0') {
                    // Decimal format — just output the number
                    let orig_number = quotient * divisor + remainder;
                    buf.push_str(&orig_number.to_string());
                } else {
                    let orig_number = quotient * divisor + remainder;
                    buf.push_str(&orig_number.to_string());
                }
                i = end + 1;
            }
            '[' => {
                // Optional section: [...] or [...|...]
                let end = find_matching_bracket(&chars, i);
                let inner: String = chars[i + 1..end].iter().collect();

                if remainder > 0 {
                    // Use the part before '|' (or the whole thing if no '|')
                    let part = if let Some(pipe) = inner.find('|') {
                        &inner[..pipe]
                    } else {
                        &inner
                    };
                    eval_body(data, current_rs, part, quotient, remainder, divisor, buf, depth);
                } else {
                    // Use the part after '|' (or nothing if no '|')
                    if let Some(pipe) = inner.find('|') {
                        let part = &inner[pipe + 1..];
                        eval_body(
                            data, current_rs, part, quotient, remainder, divisor, buf, depth,
                        );
                    }
                    // If no '|', emit nothing
                }
                i = end + 1;
            }
            '$' => {
                // Plural selection: $(cardinal,one{...}other{...})$
                if i + 1 < len && chars[i + 1] == '(' {
                    let end = find_plural_end(&chars, i);
                    let inner: String = chars[i + 2..end].iter().collect();
                    let result = eval_plural(&inner, quotient);
                    buf.push_str(&result);
                    i = end + 2; // skip )$
                } else {
                    buf.push(ch);
                    i += 1;
                }
            }
            _ => {
                buf.push(ch);
                i += 1;
            }
        }
    }
}

fn find_char(chars: &[char], target: char, start: usize) -> usize {
    for i in start..chars.len() {
        if chars[i] == target {
            return i;
        }
    }
    chars.len()
}

fn find_matching_bracket(chars: &[char], start: usize) -> usize {
    let mut depth = 0;
    for i in start..chars.len() {
        match chars[i] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    chars.len()
}

fn find_plural_end(chars: &[char], start: usize) -> usize {
    // Find )$ starting from after $(
    for i in start + 2..chars.len().saturating_sub(1) {
        if chars[i] == ')' && chars[i + 1] == '$' {
            return i;
        }
    }
    chars.len()
}

/// Evaluate a plural selection like "cardinal,one{Million}other{Millionen}"
fn eval_plural(inner: &str, number: i64) -> String {
    // Format: "cardinal,one{X}other{Y}" or "ordinal,one{st}two{nd}few{rd}other{th}"
    let comma = match inner.find(',') {
        Some(i) => i,
        None => return String::new(),
    };
    let _plural_type = &inner[..comma]; // "cardinal" or "ordinal"
    let categories = &inner[comma + 1..];

    // Determine which category applies (simplified CLDR plural rules)
    let category = select_plural_category(number);

    // Parse {value} pairs and find matching category
    let mut fallback = None;
    let mut pos = 0;
    let cat_bytes = categories.as_bytes();
    while pos < cat_bytes.len() {
        // Find the next '{'
        let brace_start = match categories[pos..].find('{') {
            Some(i) => pos + i,
            None => break,
        };
        let cat_name = categories[pos..brace_start].trim();
        let brace_end = match categories[brace_start..].find('}') {
            Some(i) => brace_start + i,
            None => break,
        };
        let value = &categories[brace_start + 1..brace_end];

        if cat_name == category {
            return value.to_string();
        }
        if cat_name == "other" {
            fallback = Some(value.to_string());
        }

        pos = brace_end + 1;
    }

    fallback.unwrap_or_default()
}

/// Simplified CLDR plural category selection for cardinal numbers.
/// Covers English-like languages (one vs other) and a few more.
fn select_plural_category(number: i64) -> &'static str {
    // English/German/Italian/French: "one" if n=1, else "other"
    // This is sufficient for RBNF where the quotient passed to plural
    // is typically 1 vs >1.
    if number == 1 {
        "one"
    } else {
        "other"
    }
}

// ---------------------------------------------------------------------------
// XML + RBNF parsing
// ---------------------------------------------------------------------------

fn parse_rbnf_xml(xml: &str) -> Option<LangData> {
    // Extract CDATA content from SpelloutRules
    let spellout_cdata = extract_spellout_cdata(xml)?;
    let rulesets = parse_rulesets(&spellout_cdata);
    Some(LangData { rulesets })
}

fn extract_spellout_cdata(xml: &str) -> Option<String> {
    // Find <rulesetGrouping type="SpelloutRules"> ... <![CDATA[...]]>
    let marker = "type=\"SpelloutRules\"";
    let start = xml.find(marker)?;
    let cdata_start = xml[start..].find("<![CDATA[")?;
    let cdata_content_start = start + cdata_start + "<![CDATA[".len();
    let cdata_end = xml[cdata_content_start..].find("]]>")?;
    Some(xml[cdata_content_start..cdata_content_start + cdata_end].to_string())
}

fn parse_rulesets(text: &str) -> HashMap<String, RuleSet> {
    let mut rulesets = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_rules: Vec<Rule> = Vec::new();
    let mut current_negative: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Check if this is a ruleset header (starts with % or %%)
        if (line.starts_with('%') || line.starts_with("%%")) && line.ends_with(':') && !line.contains(' ') {
            // Save previous ruleset
            if let Some(name) = current_name.take() {
                rulesets.insert(
                    name.clone(),
                    RuleSet {
                        name,
                        negative_rule: current_negative.take(),
                        rules: std::mem::take(&mut current_rules),
                    },
                );
            }
            current_name = Some(line[..line.len() - 1].to_string());
            current_negative = None;
            continue;
        }

        if current_name.is_none() {
            continue;
        }

        // Parse rule line: "N: body" or "N/R: body" or "-x: body" or "x.x: body"
        let colon = match line.find(':') {
            Some(i) => i,
            None => continue,
        };
        let key = line[..colon].trim();
        let body = line[colon + 1..].trim().to_string();

        // Strip trailing semicolons from body
        let body = body.strip_suffix(';').unwrap_or(&body).trim().to_string();

        if key == "-x" {
            current_negative = Some(body);
            continue;
        }

        if key == "x.x" || key == "Inf" || key == "NaN" {
            // Skip fraction, infinity, NaN rules — we only handle integers
            continue;
        }

        // Parse "N" or "N/R"
        let (value, radix) = if let Some(slash) = key.find('/') {
            let v: i64 = match key[..slash].parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let r: i64 = match key[slash + 1..].parse() {
                Ok(r) => r,
                Err(_) => continue,
            };
            (v, Some(r))
        } else {
            let v: i64 = match key.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            (v, None)
        };

        current_rules.push(Rule { value, radix, body });
    }

    // Save last ruleset
    if let Some(name) = current_name {
        rulesets.insert(
            name.clone(),
            RuleSet {
                name,
                negative_rule: current_negative,
                rules: std::mem::take(&mut current_rules),
            },
        );
    }

    rulesets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_en_has_spellout_cardinal() {
        let data = global_lang_data();
        let en = data.get("en").expect("en should be loaded");
        assert!(en.rulesets.contains_key("%spellout-cardinal"));
        assert!(en.rulesets.contains_key("%spellout-ordinal"));
    }

    #[test]
    fn parse_de_has_spellout_numbering() {
        let data = global_lang_data();
        let de = data.get("de").expect("de should be loaded");
        assert!(de.rulesets.contains_key("%spellout-numbering"));
        assert!(de.rulesets.contains_key("%spellout-ordinal"));
        assert!(
            de.rulesets.contains_key("%spellout-ordinal-r"),
            "missing %spellout-ordinal-r, available: {:?}",
            de.rulesets.keys().collect::<Vec<_>>()
        );
        assert!(de.rulesets.contains_key("%spellout-ordinal-s"));
        assert!(de.rulesets.contains_key("%spellout-ordinal-n"));
    }

    #[test]
    fn en_cardinal_basic() {
        assert_eq!(format_number("en", "%spellout-cardinal", 0), "zero");
        assert_eq!(format_number("en", "%spellout-cardinal", 1), "one");
        assert_eq!(format_number("en", "%spellout-cardinal", 5), "five");
        assert_eq!(format_number("en", "%spellout-cardinal", 12), "twelve");
        assert_eq!(format_number("en", "%spellout-cardinal", 19), "nineteen");
    }

    #[test]
    fn en_cardinal_tens() {
        assert_eq!(format_number("en", "%spellout-cardinal", 20), "twenty");
        assert_eq!(format_number("en", "%spellout-cardinal", 21), "twenty-one");
        assert_eq!(
            format_number("en", "%spellout-cardinal", 42),
            "forty-two"
        );
        assert_eq!(format_number("en", "%spellout-cardinal", 99), "ninety-nine");
    }

    #[test]
    fn en_cardinal_hundreds() {
        assert_eq!(
            format_number("en", "%spellout-cardinal", 100),
            "one hundred"
        );
        assert_eq!(
            format_number("en", "%spellout-cardinal", 115),
            "one hundred fifteen"
        );
        assert_eq!(
            format_number("en", "%spellout-cardinal", 999),
            "nine hundred ninety-nine"
        );
    }

    #[test]
    fn en_cardinal_thousands() {
        assert_eq!(
            format_number("en", "%spellout-cardinal", 1000),
            "one thousand"
        );
        assert_eq!(
            format_number("en", "%spellout-cardinal", 1001),
            "one thousand one"
        );
        assert_eq!(
            format_number("en", "%spellout-cardinal", 2500),
            "two thousand five hundred"
        );
    }

    #[test]
    fn en_cardinal_million() {
        assert_eq!(
            format_number("en", "%spellout-cardinal", 1000000),
            "one million"
        );
        assert_eq!(
            format_number("en", "%spellout-cardinal", 2134816),
            "two million one hundred thirty-four thousand eight hundred sixteen"
        );
    }

    #[test]
    fn en_ordinal_basic() {
        assert_eq!(format_number("en", "%spellout-ordinal", 1), "first");
        assert_eq!(format_number("en", "%spellout-ordinal", 2), "second");
        assert_eq!(format_number("en", "%spellout-ordinal", 3), "third");
        assert_eq!(format_number("en", "%spellout-ordinal", 12), "twelfth");
    }

    #[test]
    fn en_ordinal_tens() {
        assert_eq!(format_number("en", "%spellout-ordinal", 20), "twentieth");
        assert_eq!(
            format_number("en", "%spellout-ordinal", 21),
            "twenty-first"
        );
        assert_eq!(
            format_number("en", "%spellout-ordinal", 30),
            "thirtieth"
        );
    }

    #[test]
    fn en_ordinal_hundreds() {
        assert_eq!(
            format_number("en", "%spellout-ordinal", 100),
            "one hundredth"
        );
        assert_eq!(
            format_number("en", "%spellout-ordinal", 115),
            "one hundred fifteenth"
        );
    }

    #[test]
    fn de_cardinal_basic() {
        assert_eq!(format_number("de", "%spellout-numbering", 0), "null");
        assert_eq!(format_number("de", "%spellout-numbering", 1), "eins");
        assert_eq!(format_number("de", "%spellout-numbering", 3), "drei");
        assert_eq!(format_number("de", "%spellout-numbering", 10), "zehn");
    }

    #[test]
    fn de_cardinal_million() {
        let r = format_number("de", "%spellout-numbering", 2000000);
        assert!(r.contains("Millionen"), "got: {}", r);
    }

    #[test]
    fn de_cardinal_compound() {
        let r = format_number("de", "%spellout-numbering", 21);
        // Should contain "ein" and "zwanzig"
        assert!(r.contains("ein") && r.contains("zwanzig"), "got: {}", r);
    }

    #[test]
    fn de_cardinal_hundred() {
        let r = format_number("de", "%spellout-numbering", 100);
        assert!(r.contains("hundert"), "got: {}", r);
    }

    #[test]
    fn de_ordinal_basic() {
        let r = format_number("de", "%spellout-ordinal", 1);
        assert!(r.contains("erst"), "got: {}", r);
    }

    #[test]
    fn unknown_lang_fallback() {
        assert_eq!(format_number("xx", "%spellout-cardinal", 3), "three");
    }

    #[test]
    fn negative_number() {
        let r = format_number("en", "%spellout-cardinal", -5);
        assert_eq!(r, "minus five");
    }
}
