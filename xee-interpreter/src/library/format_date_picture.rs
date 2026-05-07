// Parses the "variable marker" inside [...] in a format-date/time picture string.
//
// Per XPath 3.1 §9.8.4.1, a variable marker has the form:
//
//   component_specifier  presentation_modifier?  width_modifier?
//
// Where:
//   - component_specifier = single letter (Y, M, D, d, F, W, w, H, h, P, m, s, f, Z, z, C, E)
//   - presentation_modifier = primary_modifier  secondary_modifier?
//     - primary: decimal digit pattern ("1", "01", "001"), name pattern (N/n/Nn),
//       roman (i/I), word (w/W/Ww), alphabetic (a/A)
//     - secondary: 'o' (ordinal), 'c' (cardinal), 'a' (alphabetic), 't' (traditional)
//   - width_modifier = ',' min ('-' max)?   where min/max are integers or '*'
//
// The width modifier is introduced by the *last* comma in the modifier string.

use crate::error;

/// The primary presentation format for a date/time component.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrimaryFormat {
    /// Decimal digit pattern. The char is the zero digit (e.g. '0' for ASCII,
    /// '\u{0E50}' for Thai). The length is the number of digits in the pattern,
    /// which sets the implicit minimum width.
    Decimal { zero_digit: char, pattern_width: u32 },
    /// Name format: N = upper, n = lower, Nn = title case.
    NameUpper,
    NameLower,
    NameTitle,
    /// Roman numerals: I = upper, i = lower.
    RomanUpper,
    RomanLower,
    /// Word format: W = upper, w = lower, Ww = title case.
    WordUpper,
    WordLower,
    WordTitle,
    /// Alphabetic: A = upper, a = lower.
    AlphaUpper,
    AlphaLower,
}

/// The secondary (ordinal/cardinal) modifier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SecondaryModifier {
    Cardinal,
    Ordinal,
}

/// Parsed width modifier: min and max widths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WidthModifier {
    pub min: Option<u32>,
    pub max: Option<u32>,
}

/// A fully parsed variable marker from inside [...].
#[derive(Debug, Clone)]
pub(crate) struct PictureMarker {
    pub component: char,
    pub primary: PrimaryFormat,
    pub secondary: SecondaryModifier,
    pub width: WidthModifier,
    /// True when the user provided an explicit presentation modifier (e.g. `[z0]`
    /// vs bare `[z]`). Used by timezone formatting to distinguish default vs compact.
    pub has_modifier: bool,
}

impl PictureMarker {
    /// Effective minimum width, considering both the digit pattern and explicit width modifier.
    pub fn min_width(&self, default: u32) -> u32 {
        // Explicit width modifier takes priority
        if let Some(min) = self.width.min {
            return min;
        }
        // Digit pattern implies minimum width
        if let PrimaryFormat::Decimal { pattern_width, .. } = &self.primary {
            return *pattern_width;
        }
        default
    }

    /// Effective maximum width. None means unbounded.
    ///
    /// Only uses the explicit width modifier (`,min-max`). The digit pattern
    /// does NOT set a maximum width — per the spec, it only sets the minimum.
    /// (For year truncation, the caller uses `digit_pattern_width()` explicitly.)
    pub fn max_width(&self) -> Option<u32> {
        self.width.max
    }

    /// The number of digits in the decimal pattern, if any.
    /// This is used by the Year component to truncate (e.g., [Y01] → 2-digit year).
    pub fn digit_pattern_width(&self) -> Option<u32> {
        if let PrimaryFormat::Decimal { pattern_width, .. } = &self.primary {
            Some(*pattern_width)
        } else {
            None
        }
    }
}

/// Parse a variable marker string (the content between '[' and ']').
///
/// Returns the parsed marker, or an error if the format is invalid.
pub(crate) fn parse_picture_marker(spec: &str) -> error::Result<PictureMarker> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(error::Error::FOFD1340);
    }

    // First character is the component specifier
    let component = spec.chars().next().unwrap();
    let rest = &spec[component.len_utf8()..];

    // Split off the width modifier (everything after the last comma)
    let (modifier_str, width) = split_width_modifier(rest);

    // Parse the presentation modifier (primary + optional secondary)
    let has_modifier = !modifier_str.trim().is_empty() || width.min.is_some() || width.max.is_some();
    let (primary, secondary) = parse_presentation_modifier(modifier_str, component);

    Ok(PictureMarker {
        component,
        primary,
        secondary,
        width,
        has_modifier,
    })
}

/// Split the modifier string at the last comma to separate presentation from width.
///
/// The width modifier is introduced by the *last* comma. Everything before
/// that comma is the presentation modifier; everything after is "min-max"
/// or "min" (where each part can be a number or '*').
fn split_width_modifier(s: &str) -> (&str, WidthModifier) {
    let no_width = WidthModifier {
        min: None,
        max: None,
    };

    if let Some(comma_pos) = s.rfind(',') {
        let presentation = &s[..comma_pos];
        let width_str = s[comma_pos + 1..].trim();
        let width = parse_width_str(width_str);
        (presentation, width)
    } else {
        (s, no_width)
    }
}

/// Parse a width string like "3-5", "3", "*-4", "2-*", "*".
fn parse_width_str(s: &str) -> WidthModifier {
    if let Some(dash_pos) = s.find('-') {
        let min_str = s[..dash_pos].trim();
        let max_str = s[dash_pos + 1..].trim();
        WidthModifier {
            min: parse_width_value(min_str),
            max: parse_width_value(max_str),
        }
    } else {
        // Just a single value: it's the minimum width, max is unbounded
        let val = parse_width_value(s.trim());
        WidthModifier {
            min: val,
            max: None,
        }
    }
}

/// Parse a single width value: a number or '*' (unbounded).
fn parse_width_value(s: &str) -> Option<u32> {
    if s == "*" || s.is_empty() {
        None
    } else {
        s.parse::<u32>().ok()
    }
}

/// Parse the presentation modifier string into primary format and secondary modifier.
///
/// The modifier is everything between the component letter and the width
/// modifier (i.e., with the comma-delimited width already split off).
fn parse_presentation_modifier(s: &str, component: char) -> (PrimaryFormat, SecondaryModifier) {
    let s = s.trim();
    if s.is_empty() {
        return (default_primary(component), SecondaryModifier::Cardinal);
    }

    // Check for secondary modifier at the end: last char is 'o', 'c', 'a', or 't'
    // But only if there's at least one char before it (single chars are primary modifiers).
    let (primary_str, secondary) = extract_secondary_modifier(s);

    let primary = parse_primary_modifier(primary_str, component);

    (primary, secondary)
}

/// Try to extract a secondary modifier ('o', 'c', 'a', 't') from the end
/// of the modifier string. The secondary modifier is the last character if
/// there are at least 2 characters in the modifier string.
fn extract_secondary_modifier(s: &str) -> (&str, SecondaryModifier) {
    if s.len() >= 2 {
        let last_char = s.chars().last().unwrap();
        match last_char {
            'o' => {
                let rest = &s[..s.len() - last_char.len_utf8()];
                (rest, SecondaryModifier::Ordinal)
            }
            'c' | 'a' | 't' => {
                let rest = &s[..s.len() - last_char.len_utf8()];
                (rest, SecondaryModifier::Cardinal)
            }
            _ => (s, SecondaryModifier::Cardinal),
        }
    } else {
        (s, SecondaryModifier::Cardinal)
    }
}

/// Parse the primary modifier string into a PrimaryFormat.
fn parse_primary_modifier(s: &str, component: char) -> PrimaryFormat {
    if s.is_empty() {
        return default_primary(component);
    }

    // Check for name/word/roman/alpha patterns
    match s {
        "N" => return PrimaryFormat::NameUpper,
        "n" => return PrimaryFormat::NameLower,
        "Nn" => return PrimaryFormat::NameTitle,
        "I" => return PrimaryFormat::RomanUpper,
        "i" => return PrimaryFormat::RomanLower,
        "W" => return PrimaryFormat::WordUpper,
        "w" => return PrimaryFormat::WordLower,
        "Ww" => return PrimaryFormat::WordTitle,
        "A" => return PrimaryFormat::AlphaUpper,
        "a" => return PrimaryFormat::AlphaLower,
        _ => {}
    }

    // Try to parse as a decimal digit pattern (e.g., "1", "01", "001", "0001")
    // The pattern can use any Unicode decimal digit; the zero digit is determined
    // by the first character of the pattern.
    if let Some(result) = parse_decimal_pattern(s) {
        return result;
    }

    // Fallback: treat as default
    default_primary(component)
}

/// Parse a decimal digit pattern like "1", "01", "001", "๐๑" (Thai).
/// Returns the zero digit and the pattern width (number of digits).
fn parse_decimal_pattern(s: &str) -> Option<PrimaryFormat> {
    let mut chars = s.chars();
    let first = chars.next()?;

    // Determine the zero digit for this digit family
    let zero_digit = unicode_zero_digit(first)?;

    // All characters must be digits from the same family
    let mut width = 1u32;
    for ch in chars {
        let ch_zero = unicode_zero_digit(ch)?;
        if ch_zero != zero_digit {
            return None; // Mixed digit families
        }
        width += 1;
    }

    Some(PrimaryFormat::Decimal {
        zero_digit,
        pattern_width: width,
    })
}

/// Find the zero digit for a Unicode decimal digit character.
///
/// For ASCII '0'-'9', returns '0'. For Thai digits, returns '๐', etc.
/// Returns None if the character is not a decimal digit.
fn unicode_zero_digit(ch: char) -> Option<char> {
    // Check if it's a Unicode digit (Nd category)
    if !ch.is_ascii_digit() && !ch.is_numeric() {
        return None;
    }

    let code = ch as u32;

    // ASCII digits
    if (0x30..=0x39).contains(&code) {
        return Some('0');
    }

    // For other digit families, the zero digit is at the start of the block.
    // Unicode digit blocks are always 10 consecutive code points.
    // Known digit families (subset covering XSLT test suite needs):
    let digit_blocks: &[u32] = &[
        0x0030, // ASCII: 0-9
        0x0660, // Arabic-Indic
        0x06F0, // Extended Arabic-Indic
        0x0966, // Devanagari
        0x09E6, // Bengali
        0x0A66, // Gurmukhi
        0x0AE6, // Gujarati
        0x0B66, // Oriya
        0x0BE6, // Tamil
        0x0C66, // Telugu
        0x0CE6, // Kannada
        0x0D66, // Malayalam
        0x0E50, // Thai
        0x0ED0, // Lao
        0x0F20, // Tibetan
        0x1040, // Myanmar
        0x1090, // Myanmar Shan
        0x17E0, // Khmer
        0x1810, // Mongolian
        0x1946, // Limbu
        0x19D0, // New Tai Lue
        0xFF10, // Fullwidth
        0x104A0, // Osmanya
    ];

    for &block_start in digit_blocks {
        if code >= block_start && code < block_start + 10 {
            return char::from_u32(block_start);
        }
    }

    None
}

/// Return the default primary format for a component when no modifier is given.
///
/// Per XPath 3.1 spec, each component has a default presentation modifier:
/// - F (day of week): Nn (title case name)
/// - P (am/pm): n (lower case name)
/// - m (minute), s (second): 01 (2-digit zero-padded)
/// - All other numeric components: 1 (1-digit minimum)
fn default_primary(component: char) -> PrimaryFormat {
    match component {
        // Name-based components
        'F' => PrimaryFormat::NameTitle,
        'P' => PrimaryFormat::NameLower,
        // Minutes and seconds default to 2-digit (01)
        'm' | 's' => PrimaryFormat::Decimal {
            zero_digit: '0',
            pattern_width: 2,
        },
        // All other numeric components default to 1
        _ => PrimaryFormat::Decimal {
            zero_digit: '0',
            pattern_width: 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_year() {
        let m = parse_picture_marker("Y").unwrap();
        assert_eq!(m.component, 'Y');
        assert_eq!(
            m.primary,
            PrimaryFormat::Decimal {
                zero_digit: '0',
                pattern_width: 1
            }
        );
        assert_eq!(m.secondary, SecondaryModifier::Cardinal);
    }

    #[test]
    fn parse_year_with_digit_pattern() {
        let m = parse_picture_marker("Y0001").unwrap();
        assert_eq!(m.component, 'Y');
        assert_eq!(
            m.primary,
            PrimaryFormat::Decimal {
                zero_digit: '0',
                pattern_width: 4
            }
        );
        assert_eq!(m.min_width(1), 4);
        assert_eq!(m.max_width(), None);
        assert_eq!(m.digit_pattern_width(), Some(4));
    }

    #[test]
    fn parse_two_digit_year() {
        let m = parse_picture_marker("Y01").unwrap();
        assert_eq!(
            m.primary,
            PrimaryFormat::Decimal {
                zero_digit: '0',
                pattern_width: 2
            }
        );
        assert_eq!(m.min_width(1), 2);
        assert_eq!(m.max_width(), None);
        assert_eq!(m.digit_pattern_width(), Some(2));
    }

    #[test]
    fn parse_month_with_width() {
        // [MN,3-3] = month name, upper case, width 3-3
        let m = parse_picture_marker("MN,3-3").unwrap();
        assert_eq!(m.component, 'M');
        assert_eq!(m.primary, PrimaryFormat::NameUpper);
        assert_eq!(m.width.min, Some(3));
        assert_eq!(m.width.max, Some(3));
    }

    #[test]
    fn parse_day_ordinal() {
        // [D1o] = day, decimal, ordinal
        let m = parse_picture_marker("D1o").unwrap();
        assert_eq!(m.component, 'D');
        assert_eq!(
            m.primary,
            PrimaryFormat::Decimal {
                zero_digit: '0',
                pattern_width: 1
            }
        );
        assert_eq!(m.secondary, SecondaryModifier::Ordinal);
    }

    #[test]
    fn parse_day_word_upper() {
        let m = parse_picture_marker("DW").unwrap();
        assert_eq!(m.component, 'D');
        assert_eq!(m.primary, PrimaryFormat::WordUpper);
    }

    #[test]
    fn parse_day_word_ordinal() {
        // [DWo] = day, word upper, ordinal
        let m = parse_picture_marker("DWo").unwrap();
        assert_eq!(m.component, 'D');
        assert_eq!(m.primary, PrimaryFormat::WordUpper);
        assert_eq!(m.secondary, SecondaryModifier::Ordinal);
    }

    #[test]
    fn parse_word_title_ordinal() {
        // [DWwo] = day, word title, ordinal
        let m = parse_picture_marker("DWwo").unwrap();
        assert_eq!(m.component, 'D');
        assert_eq!(m.primary, PrimaryFormat::WordTitle);
        assert_eq!(m.secondary, SecondaryModifier::Ordinal);
    }

    #[test]
    fn parse_roman_year() {
        let m = parse_picture_marker("YI").unwrap();
        assert_eq!(m.component, 'Y');
        assert_eq!(m.primary, PrimaryFormat::RomanUpper);
    }

    #[test]
    fn parse_width_star() {
        // [Y,2-*] = year, min 2, max unbounded
        let m = parse_picture_marker("Y,2-*").unwrap();
        assert_eq!(m.width.min, Some(2));
        assert_eq!(m.width.max, None);
    }

    #[test]
    fn parse_width_star_max() {
        // [Y,*-4] = year, min unbounded, max 4
        let m = parse_picture_marker("Y,*-4").unwrap();
        assert_eq!(m.width.min, None);
        assert_eq!(m.width.max, Some(4));
    }

    #[test]
    fn parse_day_of_week_default() {
        // [F] defaults to NameTitle
        let m = parse_picture_marker("F").unwrap();
        assert_eq!(m.component, 'F');
        assert_eq!(m.primary, PrimaryFormat::NameTitle);
    }

    #[test]
    fn parse_day_of_week_numeric() {
        // [F1] or [F01] = numeric day-of-week
        let m = parse_picture_marker("F01").unwrap();
        assert_eq!(m.component, 'F');
        assert_eq!(
            m.primary,
            PrimaryFormat::Decimal {
                zero_digit: '0',
                pattern_width: 2
            }
        );
    }

    #[test]
    fn parse_alpha_upper() {
        let m = parse_picture_marker("mA").unwrap();
        assert_eq!(m.component, 'm');
        assert_eq!(m.primary, PrimaryFormat::AlphaUpper);
    }

    #[test]
    fn parse_alpha_lower() {
        let m = parse_picture_marker("sa").unwrap();
        assert_eq!(m.component, 's');
        assert_eq!(m.primary, PrimaryFormat::AlphaLower);
    }

    #[test]
    fn parse_empty_is_error() {
        assert!(parse_picture_marker("").is_err());
    }

    #[test]
    fn parse_thai_digits() {
        let m = parse_picture_marker("Y๐๐๐๑").unwrap();
        assert_eq!(m.component, 'Y');
        if let PrimaryFormat::Decimal {
            zero_digit,
            pattern_width,
        } = m.primary
        {
            assert_eq!(zero_digit, '๐');
            assert_eq!(pattern_width, 4);
        } else {
            panic!("Expected Decimal");
        }
    }

    #[test]
    fn parse_thai_month() {
        let m = parse_picture_marker("M๐๑").unwrap();
        assert_eq!(m.component, 'M');
        if let PrimaryFormat::Decimal {
            zero_digit,
            pattern_width,
        } = m.primary
        {
            assert_eq!(zero_digit, '๐');
            assert_eq!(pattern_width, 2);
        } else {
            panic!("Expected Decimal for Thai month");
        }
        assert_eq!(m.min_width(1), 2);
    }
}
