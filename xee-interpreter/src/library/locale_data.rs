//! Locale data for date/time formatting.
//!
//! Maps XPath language codes (BCP 47 style, e.g. "de", "fr", "sv-SE") to
//! glibc locale data via `pure_rust_locales`. Provides localized month names,
//! day names, and AM/PM strings.

use pure_rust_locales::{locale_match, Locale};

/// Try to resolve an XPath language code to a glibc `Locale`.
///
/// Handles:
/// - Full locale specs: "de_DE", "pt_BR", "en_GB"
/// - BCP 47 with hyphens: "de-DE", "pt-BR", "en-GB"  
/// - Language-only codes: "de" → de_DE, "fr" → fr_FR, etc.
pub(crate) fn resolve_locale(lang: &str) -> Option<Locale> {
    // Normalize hyphens to underscores (BCP 47 → glibc convention)
    let normalized = lang.replace('-', "_");

    // Try exact match first (handles "de_DE", "pt_BR", etc.)
    if let Ok(locale) = normalized.parse::<Locale>() {
        return Some(locale);
    }

    // Extract primary language subtag and try default country mapping
    let primary = normalized.split('_').next().unwrap_or(&normalized);
    default_locale_for_language(primary)
}

/// Map a primary language code to its most common/default locale.
fn default_locale_for_language(lang: &str) -> Option<Locale> {
    let locale = match lang {
        "af" => Locale::af_ZA,
        "am" => Locale::am_ET,
        "an" => Locale::an_ES,
        "ar" => Locale::ar_EG,
        "as" => Locale::as_IN,
        "az" => Locale::az_AZ,
        "be" => Locale::be_BY,
        "bg" => Locale::bg_BG,
        "bn" => Locale::bn_BD,
        "bo" => Locale::bo_CN,
        "br" => Locale::br_FR,
        "bs" => Locale::bs_BA,
        "ca" => Locale::ca_ES,
        "cs" => Locale::cs_CZ,
        "cy" => Locale::cy_GB,
        "da" => Locale::da_DK,
        "de" => Locale::de_DE,
        "el" => Locale::el_GR,
        "en" => Locale::en_US,
        "eo" => Locale::eo,
        "es" => Locale::es_ES,
        "et" => Locale::et_EE,
        "eu" => Locale::eu_ES,
        "fa" => Locale::fa_IR,
        "fi" => Locale::fi_FI,
        "fo" => Locale::fo_FO,
        "fr" => Locale::fr_FR,
        "ga" => Locale::ga_IE,
        "gd" => Locale::gd_GB,
        "gl" => Locale::gl_ES,
        "gu" => Locale::gu_IN,
        "he" => Locale::he_IL,
        "hi" => Locale::hi_IN,
        "hr" => Locale::hr_HR,
        "ht" => Locale::ht_HT,
        "hu" => Locale::hu_HU,
        "hy" => Locale::hy_AM,
        "id" => Locale::id_ID,
        "is" => Locale::is_IS,
        "it" => Locale::it_IT,
        "ja" => Locale::ja_JP,
        "ka" => Locale::ka_GE,
        "kk" => Locale::kk_KZ,
        "km" => Locale::km_KH,
        "kn" => Locale::kn_IN,
        "ko" => Locale::ko_KR,
        "ku" => Locale::ku_TR,
        "ky" => Locale::ky_KG,
        "lb" => Locale::lb_LU,
        "lo" => Locale::lo_LA,
        "lt" => Locale::lt_LT,
        "lv" => Locale::lv_LV,
        "mk" => Locale::mk_MK,
        "ml" => Locale::ml_IN,
        "mn" => Locale::mn_MN,
        "mr" => Locale::mr_IN,
        "ms" => Locale::ms_MY,
        "mt" => Locale::mt_MT,
        "my" => Locale::my_MM,
        "nb" => Locale::nb_NO,
        "ne" => Locale::ne_NP,
        "nl" => Locale::nl_NL,
        "nn" => Locale::nn_NO,
        "no" => Locale::nb_NO,
        "or" => Locale::or_IN,
        "pa" => Locale::pa_IN,
        "pl" => Locale::pl_PL,
        "ps" => Locale::ps_AF,
        "pt" => Locale::pt_PT,
        "ro" => Locale::ro_RO,
        "ru" => Locale::ru_RU,
        "sa" => Locale::sa_IN,
        "se" => Locale::se_NO,
        "si" => Locale::si_LK,
        "sk" => Locale::sk_SK,
        "sl" => Locale::sl_SI,
        "sq" => Locale::sq_AL,
        "sr" => Locale::sr_RS,
        "sv" => Locale::sv_SE,
        "sw" => Locale::sw_TZ,
        "ta" => Locale::ta_IN,
        "te" => Locale::te_IN,
        "tg" => Locale::tg_TJ,
        "th" => Locale::th_TH,
        "tk" => Locale::tk_TM,
        "tl" => Locale::tl_PH,
        "tr" => Locale::tr_TR,
        "uk" => Locale::uk_UA,
        "ur" => Locale::ur_PK,
        "uz" => Locale::uz_UZ,
        "vi" => Locale::vi_VN,
        "wo" => Locale::wo_SN,
        "yi" => Locale::yi_US,
        "yo" => Locale::yo_NG,
        "zh" => Locale::zh_CN,
        "zu" => Locale::zu_ZA,
        _ => return None,
    };
    Some(locale)
}

/// Get a full month name for the given 1-based month number.
pub(crate) fn month_name(month: u32, locale: Locale) -> Option<&'static str> {
    let months: &[&str] = locale_match!(locale => LC_TIME::MON);
    months.get(month.wrapping_sub(1) as usize).copied()
}

/// Get an abbreviated month name for the given 1-based month number.
pub(crate) fn month_name_abbrev(month: u32, locale: Locale) -> Option<&'static str> {
    let months: &[&str] = locale_match!(locale => LC_TIME::ABMON);
    months.get(month.wrapping_sub(1) as usize).copied()
}

/// Get a full day name for the given day index (0=Monday, 6=Sunday, chrono convention).
pub(crate) fn day_name(day_from_monday: usize, locale: Locale) -> Option<&'static str> {
    // glibc DAY array is Sunday-first: [Sun, Mon, Tue, Wed, Thu, Fri, Sat]
    // chrono uses Monday-first: 0=Mon, 1=Tue, ..., 6=Sun
    // Convert: Monday(0) → index 1, Sunday(6) → index 0
    let glibc_idx = (day_from_monday + 1) % 7;
    let days: &[&str] = locale_match!(locale => LC_TIME::DAY);
    days.get(glibc_idx).copied()
}

/// Get an abbreviated day name for the given day index (0=Monday, 6=Sunday).
pub(crate) fn day_name_abbrev(day_from_monday: usize, locale: Locale) -> Option<&'static str> {
    let glibc_idx = (day_from_monday + 1) % 7;
    let days: &[&str] = locale_match!(locale => LC_TIME::ABDAY);
    days.get(glibc_idx).copied()
}

/// Get the AM or PM string for the locale.
/// Returns None if the locale has no AM/PM strings (e.g. German).
pub(crate) fn ampm_str(is_am: bool, locale: Locale) -> Option<&'static str> {
    let ampm: &[&str] = locale_match!(locale => LC_TIME::AM_PM);
    let idx = if is_am { 0 } else { 1 };
    let s = ampm.get(idx)?;
    // Some locales have empty AM/PM strings (24h cultures)
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_locale_language_only() {
        assert_eq!(resolve_locale("de"), Some(Locale::de_DE));
        assert_eq!(resolve_locale("fr"), Some(Locale::fr_FR));
        assert_eq!(resolve_locale("sv"), Some(Locale::sv_SE));
        assert_eq!(resolve_locale("en"), Some(Locale::en_US));
    }

    #[test]
    fn test_resolve_locale_full() {
        assert_eq!(resolve_locale("de_DE"), Some(Locale::de_DE));
        assert_eq!(resolve_locale("pt_BR"), Some(Locale::pt_BR));
        assert_eq!(resolve_locale("en_GB"), Some(Locale::en_GB));
    }

    #[test]
    fn test_resolve_locale_bcp47() {
        assert_eq!(resolve_locale("de-DE"), Some(Locale::de_DE));
        assert_eq!(resolve_locale("pt-BR"), Some(Locale::pt_BR));
        assert_eq!(resolve_locale("en-GB"), Some(Locale::en_GB));
    }

    #[test]
    fn test_resolve_locale_unknown() {
        assert_eq!(resolve_locale("xx"), None);
    }

    #[test]
    fn test_month_names_german() {
        let locale = Locale::de_DE;
        assert_eq!(month_name(1, locale), Some("Januar"));
        assert_eq!(month_name(3, locale), Some("März"));
        assert_eq!(month_name(12, locale), Some("Dezember"));
    }

    #[test]
    fn test_month_names_french() {
        let locale = Locale::fr_FR;
        assert_eq!(month_name(1, locale), Some("janvier"));
        assert_eq!(month_name(7, locale), Some("juillet"));
    }

    #[test]
    fn test_day_names_german() {
        let locale = Locale::de_DE;
        // 0=Monday, 6=Sunday
        assert_eq!(day_name(0, locale), Some("Montag"));
        assert_eq!(day_name(4, locale), Some("Freitag"));
        assert_eq!(day_name(6, locale), Some("Sonntag"));
    }

    #[test]
    fn test_day_names_swedish() {
        let locale = Locale::sv_SE;
        assert_eq!(day_name(0, locale), Some("måndag"));
        assert_eq!(day_name(6, locale), Some("söndag"));
    }

    #[test]
    fn test_ampm_english() {
        let locale = Locale::en_US;
        assert_eq!(ampm_str(true, locale), Some("AM"));
        assert_eq!(ampm_str(false, locale), Some("PM"));
    }

    #[test]
    fn test_ampm_german_empty() {
        let locale = Locale::de_DE;
        // German doesn't use AM/PM
        assert_eq!(ampm_str(true, locale), None);
        assert_eq!(ampm_str(false, locale), None);
    }

    #[test]
    fn test_month_name_abbrev() {
        let locale = Locale::de_DE;
        assert_eq!(month_name_abbrev(1, locale), Some("Jan"));
        assert_eq!(month_name_abbrev(3, locale), Some("Mär"));
    }

    #[test]
    fn test_month_out_of_range() {
        let locale = Locale::en_US;
        assert_eq!(month_name(0, locale), None);
        assert_eq!(month_name(13, locale), None);
    }
}
