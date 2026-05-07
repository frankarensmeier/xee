// https://www.w3.org/TR/xpath-functions-31/#dates-times

use chrono::{Datelike, Offset, SubsecRound, Timelike};
use ibig::IBig;
use rust_decimal::Decimal;
use xee_xpath_macros::xpath_fn;

use crate::atomic::ToDateTimeStamp;
use crate::function::StaticFunctionDescription;
use crate::{
    atomic::NaiveDateTimeWithOffset, atomic::NaiveDateWithOffset, atomic::NaiveTimeWithOffset,
    context::DynamicContext, error, wrap_xpath_fn,
};

use super::locale_data;

#[xpath_fn("fn:dateTime($arg1 as xs:date?, $arg2 as xs:time?) as xs:dateTime?")]
fn date_time(
    arg1: Option<NaiveDateWithOffset>,
    arg2: Option<NaiveTimeWithOffset>,
) -> error::Result<Option<NaiveDateTimeWithOffset>> {
    match (arg1, arg2) {
        (Some(arg1), Some(arg2)) => {
            let offset = match (arg1.offset, arg2.offset) {
                (Some(arg1), Some(arg2)) => {
                    if arg1 == arg2 {
                        Some(arg1)
                    } else {
                        return Err(error::Error::FORG0008);
                    }
                }
                (Some(arg1), None) => Some(arg1),
                (None, Some(arg2)) => Some(arg2),
                (None, None) => None,
            };
            Ok(Some(NaiveDateTimeWithOffset::new(
                arg1.date.and_time(arg2.time),
                offset,
            )))
        }
        (Some(_), None) => Ok(None),
        (None, Some(_)) => Ok(None),
        (None, None) => Ok(None),
    }
}

#[xpath_fn("fn:year-from-dateTime($arg as xs:dateTime?) as xs:integer?")]
fn year_from_date_time(arg: Option<NaiveDateTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date_time.year().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:month-from-dateTime($arg as xs:dateTime?) as xs:integer?")]
fn month_from_date_time(arg: Option<NaiveDateTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date_time.month().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:day-from-dateTime($arg as xs:dateTime?) as xs:integer?")]
fn day_from_date_time(arg: Option<NaiveDateTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date_time.day().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:hours-from-dateTime($arg as xs:dateTime?) as xs:integer?")]
fn hours_from_date_time(arg: Option<NaiveDateTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date_time.hour().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:minutes-from-dateTime($arg as xs:dateTime?) as xs:integer?")]
fn minutes_from_date_time(arg: Option<NaiveDateTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date_time.minute().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:seconds-from-dateTime($arg as xs:dateTime?) as xs:decimal?")]
fn seconds_from_date_time(arg: Option<NaiveDateTimeWithOffset>) -> error::Result<Option<Decimal>> {
    match arg {
        Some(arg) => Ok(Some(seconds(arg.date_time))),
        None => Ok(None),
    }
}

#[xpath_fn("fn:timezone-from-dateTime($arg as xs:dateTime?) as xs:dayTimeDuration?")]
fn timezone_from_date_time(
    arg: Option<NaiveDateTimeWithOffset>,
) -> error::Result<Option<chrono::Duration>> {
    match arg {
        Some(arg) => Ok(offset_to_duration_option(arg.offset)),
        None => Ok(None),
    }
}

#[xpath_fn("fn:year-from-date($arg as xs:date?) as xs:integer?")]
fn year_from_date(arg: Option<NaiveDateWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date.year().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:month-from-date($arg as xs:date?) as xs:integer?")]
fn month_from_date(arg: Option<NaiveDateWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date.month().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:day-from-date($arg as xs:date?) as xs:integer?")]
fn day_from_date(arg: Option<NaiveDateWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.date.day().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:timezone-from-date($arg as xs:date?) as xs:time?")]
fn timezone_from_date(arg: Option<NaiveDateWithOffset>) -> error::Result<Option<chrono::Duration>> {
    match arg {
        Some(arg) => Ok(offset_to_duration_option(arg.offset)),
        None => Ok(None),
    }
}

#[xpath_fn("fn:hours-from-time($arg as xs:time?) as xs:integer?")]
fn hours_from_time(arg: Option<NaiveTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.time.hour().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:minutes-from-time($arg as xs:time?) as xs:integer?")]
fn minutes_from_time(arg: Option<NaiveTimeWithOffset>) -> error::Result<Option<IBig>> {
    match arg {
        Some(arg) => Ok(Some(arg.time.minute().into())),
        None => Ok(None),
    }
}

#[xpath_fn("fn:seconds-from-time($arg as xs:time?) as xs:decimal?")]
fn seconds_from_time(arg: Option<NaiveTimeWithOffset>) -> error::Result<Option<Decimal>> {
    match arg {
        Some(arg) => Ok(Some(seconds(arg.time))),
        None => Ok(None),
    }
}

#[xpath_fn("fn:timezone-from-time($arg as xs:time?) as xs:dayTimeDuration?")]
fn timezone_from_time(arg: Option<NaiveTimeWithOffset>) -> error::Result<Option<chrono::Duration>> {
    match arg {
        Some(arg) => Ok(offset_to_duration_option(arg.offset)),
        None => Ok(None),
    }
}

#[xpath_fn("fn:adjust-dateTime-to-timezone($arg as xs:dateTime?) as xs:dateTime?")]
fn adjust_date_time_to_timezone1(
    context: &DynamicContext,
    arg: Option<NaiveDateTimeWithOffset>,
) -> error::Result<Option<NaiveDateTimeWithOffset>> {
    adjust_date_time_to_timezone(arg, Some(context.implicit_timezone()))
}

#[xpath_fn("fn:adjust-dateTime-to-timezone($arg as xs:dateTime?, $timezone as xs:dayTimeDuration?) as xs:dateTime?")]
fn adjust_date_time_to_timezone2(
    arg: Option<NaiveDateTimeWithOffset>,
    timezone: Option<chrono::Duration>,
) -> error::Result<Option<NaiveDateTimeWithOffset>> {
    adjust_date_time_to_timezone(arg, duration_to_offset(timezone)?)
}

fn adjust_date_time_to_timezone(
    arg: Option<NaiveDateTimeWithOffset>,
    offset: Option<chrono::FixedOffset>,
) -> error::Result<Option<NaiveDateTimeWithOffset>> {
    match (arg, offset) {
        (Some(arg), Some(offset)) => {
            let date_time = if arg.offset.is_some() {
                // we need to first turn this into a date time stamp;
                // the default offset will be ignored as we know we have
                // an arg.offset
                let stamp = arg.to_date_time_stamp(chrono::offset::Utc.fix());
                // now we need to take the same time in this offset
                let stamp = stamp.with_timezone(&offset);
                // now we get the naive datetime local again
                stamp.naive_local()
            } else {
                arg.date_time
            };
            Ok(Some(NaiveDateTimeWithOffset::new(date_time, Some(offset))))
        }
        (Some(arg), None) => Ok(Some(NaiveDateTimeWithOffset::new(arg.date_time, None))),
        (None, _) => Ok(None),
    }
}

#[xpath_fn("fn:adjust-date-to-timezone($arg as xs:date?) as xs:date?")]
fn adjust_date_to_timezone1(
    context: &crate::context::DynamicContext,
    arg: Option<NaiveDateWithOffset>,
) -> error::Result<Option<NaiveDateWithOffset>> {
    adjust_date_to_timezone(arg, Some(context.implicit_timezone()))
}

#[xpath_fn(
    "fn:adjust-date-to-timezone($arg as xs:date?, $timezone as xs:dayTimeDuration?) as xs:date?"
)]
fn adjust_date_to_timezone2(
    arg: Option<NaiveDateWithOffset>,
    timezone: Option<chrono::Duration>,
) -> error::Result<Option<NaiveDateWithOffset>> {
    adjust_date_to_timezone(arg, duration_to_offset(timezone)?)
}

fn adjust_date_to_timezone(
    arg: Option<NaiveDateWithOffset>,
    offset: Option<chrono::FixedOffset>,
) -> error::Result<Option<NaiveDateWithOffset>> {
    match (arg, offset) {
        (Some(arg), Some(offset)) => {
            let stamp = arg.to_date_time_stamp(chrono::offset::Utc.fix());
            let stamp = if arg.offset.is_some() {
                stamp + offset
            } else {
                stamp
            };
            Ok(Some(NaiveDateWithOffset::new(
                stamp.naive_utc().date(),
                Some(offset),
            )))
        }
        (Some(arg), None) => Ok(Some(NaiveDateWithOffset::new(arg.date, None))),
        (None, _) => Ok(None),
    }
}

#[xpath_fn("fn:adjust-time-to-timezone($arg as xs:time?) as xs:time?")]
fn adjust_time_to_timezone1(
    context: &crate::context::DynamicContext,
    arg: Option<NaiveTimeWithOffset>,
) -> error::Result<Option<NaiveTimeWithOffset>> {
    adjust_time_to_timezone(arg, Some(context.implicit_timezone()))
}

#[xpath_fn(
    "fn:adjust-time-to-timezone($arg as xs:time?, $timezone as xs:dayTimeDuration?) as xs:time?"
)]
fn adjust_time_to_timezone2(
    arg: Option<NaiveTimeWithOffset>,
    timezone: Option<chrono::Duration>,
) -> error::Result<Option<NaiveTimeWithOffset>> {
    adjust_time_to_timezone(arg, duration_to_offset(timezone)?)
}

fn adjust_time_to_timezone(
    arg: Option<NaiveTimeWithOffset>,
    offset: Option<chrono::FixedOffset>,
) -> error::Result<Option<NaiveTimeWithOffset>> {
    match (arg, offset) {
        (Some(arg), Some(offset)) => {
            let stamp = arg.to_date_time_stamp(chrono::offset::Utc.fix());
            let stamp = if let Some(_arg_offset) = arg.offset {
                // the arg offset is already processed when we do
                // to_date_time_stamp, but the offset still needs to be
                // added in this case
                stamp + offset
            } else {
                stamp
            };
            Ok(Some(NaiveTimeWithOffset::new(
                stamp.naive_utc().time(),
                Some(offset),
            )))
        }
        (Some(arg), None) => Ok(Some(NaiveTimeWithOffset::new(arg.time, None))),
        (None, _) => Ok(None),
    }
}

fn seconds(time: impl Timelike + SubsecRound + Copy) -> Decimal {
    let nanoseconds: Decimal = time.round_subsecs(3).nanosecond().into();
    let seconds: Decimal = time.second().into();
    seconds + (nanoseconds / Decimal::from(1_000_000_000))
}

fn offset_to_duration_option(offset: Option<chrono::FixedOffset>) -> Option<chrono::Duration> {
    offset.map(offset_to_duration)
}

pub(crate) fn offset_to_duration(offset: chrono::FixedOffset) -> chrono::Duration {
    chrono::Duration::seconds(offset.local_minus_utc() as i64)
}

fn duration_to_offset(
    duration: Option<chrono::Duration>,
) -> error::Result<Option<chrono::FixedOffset>> {
    if let Some(duration) = duration {
        if duration > chrono::Duration::hours(14)
            || duration < chrono::Duration::hours(-14)
            || duration.num_seconds() % 60 != 0
        {
            return Err(error::Error::FODT0003);
        }
        Ok(Some(
            chrono::FixedOffset::east_opt(
                duration
                    .num_seconds()
                    .try_into()
                    .expect("too many seconds to convert"),
            )
            .unwrap(),
        ))
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:parse-ietf-date($value as xs:string?) as xs:dateTime?")]
fn parse_ietf_date(value: Option<&str>) -> error::Result<Option<NaiveDateTimeWithOffset>> {
    if let Some(value) = value {
        match chrono::DateTime::parse_from_rfc2822(value.trim()) {
            Ok(date_time) => Ok(Some(date_time.into())),
            Err(_) => Err(error::Error::FORG0010),
        }
    } else {
        Ok(None)
    }
}

// format-dateTime, format-date, format-time
// https://www.w3.org/TR/xpath-functions-31/#func-format-dateTime

#[xpath_fn("fn:format-dateTime($value as xs:dateTime?, $picture as xs:string) as xs:string?")]
fn format_date_time2(
    value: Option<NaiveDateTimeWithOffset>,
    picture: &str,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    format_datetime_picture(
        &value.date_time,
        value.offset.as_ref(),
        picture,
        DateTimeKind::DateTime,
        None,
        None,
    )
    .map(Some)
}

#[xpath_fn("fn:format-dateTime($value as xs:dateTime?, $picture as xs:string, $language as xs:string?, $calendar as xs:string?, $place as xs:string?) as xs:string?")]
fn format_date_time5(
    value: Option<NaiveDateTimeWithOffset>,
    picture: &str,
    language: Option<&str>,
    calendar: Option<&str>,
    _place: Option<&str>,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    format_datetime_picture(
        &value.date_time,
        value.offset.as_ref(),
        picture,
        DateTimeKind::DateTime,
        language,
        calendar,
    )
    .map(Some)
}

#[xpath_fn("fn:format-date($value as xs:date?, $picture as xs:string) as xs:string?")]
fn format_date2(
    value: Option<NaiveDateWithOffset>,
    picture: &str,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = value.date.and_hms_opt(0, 0, 0).unwrap();
    format_datetime_picture(&dt, value.offset.as_ref(), picture, DateTimeKind::Date, None, None)
        .map(Some)
}

#[xpath_fn("fn:format-date($value as xs:date?, $picture as xs:string, $language as xs:string?, $calendar as xs:string?, $place as xs:string?) as xs:string?")]
fn format_date5(
    value: Option<NaiveDateWithOffset>,
    picture: &str,
    language: Option<&str>,
    calendar: Option<&str>,
    _place: Option<&str>,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = value.date.and_hms_opt(0, 0, 0).unwrap();
    format_datetime_picture(
        &dt,
        value.offset.as_ref(),
        picture,
        DateTimeKind::Date,
        language,
        calendar,
    )
    .map(Some)
}

#[xpath_fn("fn:format-time($value as xs:time?, $picture as xs:string) as xs:string?")]
fn format_time2(
    value: Option<NaiveTimeWithOffset>,
    picture: &str,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = chrono::NaiveDate::from_ymd_opt(2000, 1, 1)
        .unwrap()
        .and_time(value.time);
    format_datetime_picture(&dt, value.offset.as_ref(), picture, DateTimeKind::Time, None, None)
        .map(Some)
}

#[xpath_fn("fn:format-time($value as xs:time?, $picture as xs:string, $language as xs:string?, $calendar as xs:string?, $place as xs:string?) as xs:string?")]
fn format_time5(
    value: Option<NaiveTimeWithOffset>,
    picture: &str,
    language: Option<&str>,
    calendar: Option<&str>,
    _place: Option<&str>,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = chrono::NaiveDate::from_ymd_opt(2000, 1, 1)
        .unwrap()
        .and_time(value.time);
    format_datetime_picture(
        &dt,
        value.offset.as_ref(),
        picture,
        DateTimeKind::Time,
        language,
        calendar,
    )
    .map(Some)
}

#[derive(Clone, Copy)]
enum DateTimeKind {
    DateTime,
    Date,
    Time,
}

use super::format_date_picture::{
    parse_picture_marker, PictureMarker, PrimaryFormat, SecondaryModifier,
};
use super::number_words;

/// Format a dateTime/date/time value using a picture string.
///
/// Implements the algorithm from XPath 3.1 §9.8.4.
fn format_datetime_picture(
    dt: &chrono::NaiveDateTime,
    offset: Option<&chrono::FixedOffset>,
    picture: &str,
    kind: DateTimeKind,
    language: Option<&str>,
    calendar: Option<&str>,
) -> error::Result<String> {
    let mut result = String::new();

    // Language/calendar fallback prefixes (per spec §9.8.4.7)
    let lang = resolve_language(language, &mut result);
    resolve_calendar(calendar, &mut result);

    let mut chars = picture.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '[' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    result.push('[');
                } else {
                    // Collect the variable marker content between [ and ]
                    let mut spec = String::new();
                    loop {
                        match chars.next() {
                            Some(']') => break,
                            Some(ch) => spec.push(ch),
                            None => return Err(error::Error::FOFD1340),
                        }
                    }
                    let marker = parse_picture_marker(&spec)?;
                    format_component(dt, offset, &marker, kind, lang, &mut result)?;
                }
            }
            ']' => {
                if chars.peek() == Some(&']') {
                    chars.next();
                    result.push(']');
                } else {
                    return Err(error::Error::FOFD1340);
                }
            }
            _ => result.push(c),
        }
    }

    Ok(result)
}

/// Check if the requested language is supported and emit a fallback prefix if not.
/// Returns the effective language to use for formatting.
fn resolve_language<'a>(language: Option<&'a str>, result: &mut String) -> Option<&'a str> {
    let Some(lang) = language else {
        return None;
    };
    if lang.is_empty() {
        return Some(lang);
    }
    // Check if we can resolve this language to a locale
    if locale_data::resolve_locale(lang).is_some() {
        Some(lang)
    } else {
        result.push_str("[Language: en]");
        Some("en")
    }
}

/// Check if the requested calendar is supported and emit a fallback prefix if not.
fn resolve_calendar(calendar: Option<&str>, result: &mut String) {
    let Some(cal) = calendar else { return };
    // We only support the ISO/AD/CE calendar (the default)
    let cal_upper = cal.to_uppercase();
    if cal_upper != "AD" && cal_upper != "ISO" && cal_upper != "CE" && !cal.is_empty() {
        result.push_str("[Calendar: AD]");
    }
}

/// Format a single component from the picture string.
fn format_component(
    dt: &chrono::NaiveDateTime,
    offset: Option<&chrono::FixedOffset>,
    marker: &PictureMarker,
    kind: DateTimeKind,
    language: Option<&str>,
    result: &mut String,
) -> error::Result<()> {
    match marker.component {
        'Y' => {
            check_date_component(kind, marker.component)?;
            let year = dt.year().unsigned_abs();
            format_numeric_or_name(year, marker, result)
        }
        'M' => {
            check_date_component(kind, marker.component)?;
            let month = dt.month();
            format_month(month, marker, language, result)
        }
        'D' => {
            check_date_component(kind, marker.component)?;
            format_numeric_or_name(dt.day(), marker, result)
        }
        'd' => {
            check_date_component(kind, marker.component)?;
            format_numeric_or_name(dt.ordinal(), marker, result)
        }
        'F' => {
            check_date_component(kind, marker.component)?;
            format_day_of_week(dt, marker, language, result)
        }
        'W' => {
            check_date_component(kind, marker.component)?;
            let week = dt.iso_week().week();
            format_numeric_or_name(week, marker, result)
        }
        'w' => {
            // Week of month (ISO): week number that the day falls in
            check_date_component(kind, marker.component)?;
            let week_of_month = iso_week_of_month(dt);
            format_numeric_or_name(week_of_month, marker, result)
        }
        'H' => {
            check_time_component(kind, marker.component)?;
            format_numeric_or_name(dt.hour(), marker, result)
        }
        'h' => {
            check_time_component(kind, marker.component)?;
            let h = dt.hour() % 12;
            let h = if h == 0 { 12 } else { h };
            format_numeric_or_name(h, marker, result)
        }
        'P' => {
            check_time_component(kind, marker.component)?;
            format_ampm(dt, marker, language, result)
        }
        'm' => {
            check_time_component(kind, marker.component)?;
            format_numeric_or_name(dt.minute(), marker, result)
        }
        's' => {
            check_time_component(kind, marker.component)?;
            format_numeric_or_name(dt.second(), marker, result)
        }
        'f' => {
            check_time_component(kind, marker.component)?;
            format_fractional_seconds(dt, marker, result)
        }
        'Z' | 'z' => {
            format_timezone(offset, marker, result)
        }
        'C' => {
            // Calendar: always ISO
            result.push_str("ISO");
            Ok(())
        }
        'E' => {
            check_date_component(kind, marker.component)?;
            format_era(dt, marker, result)
        }
        _ => Err(error::Error::FOFD1340),
    }
}

// ── Numeric formatting ──────────────────────────────────────────────────────

/// Core formatting for numeric components. Handles decimal, roman, word,
/// alphabetic, and ordinal presentation modifiers.
fn format_numeric_or_name(
    value: u32,
    marker: &PictureMarker,
    result: &mut String,
) -> error::Result<()> {
    // Determine default min-width based on component
    let default_min = default_min_width(marker.component);

    match &marker.primary {
        PrimaryFormat::Decimal {
            zero_digit,
            pattern_width,
        } => {
            let min_w = marker.min_width(default_min);
            let max_w = marker.max_width();

            // Apply max-width truncation.
            // For Year, the digit pattern width implies truncation when explicitly
            // multi-digit (e.g. [Y01] → 2-digit year). The default [Y] = [Y1] does
            // NOT truncate — pattern_width=1 is just the default, not a request for
            // a single-digit year. For other components, only an explicit width
            // modifier causes truncation.
            let effective_max = match max_w {
                Some(max) => Some(max),
                None if marker.component == 'Y' && *pattern_width > 1 => Some(*pattern_width),
                None => None,
            };

            let truncated = if let Some(max) = effective_max {
                truncate_value(value, max)
            } else {
                value
            };

            let formatted = format_decimal(truncated, *zero_digit, min_w);
            result.push_str(&formatted);

            // Append ordinal suffix if requested
            if marker.secondary == SecondaryModifier::Ordinal {
                result.push_str(number_words::ordinal_suffix(truncated as i64));
            }
            Ok(())
        }
        PrimaryFormat::RomanUpper => {
            let roman = format_roman_number(value, true)?;
            let min_w = marker.min_width(1) as usize;
            if roman.len() < min_w {
                result.push_str(&roman);
                result.extend(std::iter::repeat(' ').take(min_w - roman.len()));
            } else {
                result.push_str(&roman);
            }
            Ok(())
        }
        PrimaryFormat::RomanLower => {
            let roman = format_roman_number(value, false)?;
            let min_w = marker.min_width(1) as usize;
            if roman.len() < min_w {
                result.push_str(&roman);
                result.extend(std::iter::repeat(' ').take(min_w - roman.len()));
            } else {
                result.push_str(&roman);
            }
            Ok(())
        }
        PrimaryFormat::WordUpper | PrimaryFormat::WordLower | PrimaryFormat::WordTitle => {
            let case = match &marker.primary {
                PrimaryFormat::WordUpper => number_words::WordCase::Upper,
                PrimaryFormat::WordLower => number_words::WordCase::Lower,
                _ => number_words::WordCase::Title,
            };
            let ordinal = if marker.secondary == SecondaryModifier::Ordinal {
                Some("yes")
            } else {
                None
            };
            let words = number_words::number_to_words_lang(value as i64, case, Some("en"), ordinal);
            result.push_str(&words);
            Ok(())
        }
        PrimaryFormat::AlphaUpper => {
            result.push_str(&format_alpha(value, true));
            Ok(())
        }
        PrimaryFormat::AlphaLower => {
            result.push_str(&format_alpha(value, false));
            Ok(())
        }
        // Name formats don't make sense for pure numeric components; fall back to decimal
        PrimaryFormat::NameUpper | PrimaryFormat::NameLower | PrimaryFormat::NameTitle => {
            let min_w = marker.min_width(default_min);
            let formatted = format_decimal(value, '0', min_w);
            result.push_str(&formatted);
            Ok(())
        }
    }
}

/// Default minimum width for numeric components when no modifier is given.
fn default_min_width(component: char) -> u32 {
    match component {
        'Y' => 4,  // Year defaults to 4 digits
        'm' => 2,  // Minutes default to 2 digits
        's' => 2,  // Seconds default to 2 digits
        _ => 1,    // Everything else defaults to 1
    }
}

/// Truncate a value to fit within max_width digits.
/// E.g., truncate_value(2003, 2) = 3 (2003 % 100).
fn truncate_value(value: u32, max_width: u32) -> u32 {
    if max_width == 0 {
        return value;
    }
    let modulus = 10u32.saturating_pow(max_width);
    value % modulus
}

/// Format a number using the given zero digit, with at least `min_width` digits.
fn format_decimal(value: u32, zero_digit: char, min_width: u32) -> String {
    let digits = decimal_digits(value, zero_digit);
    let char_count = digits.chars().count() as u32;
    let padding = if char_count < min_width {
        (min_width - char_count) as usize
    } else {
        0
    };
    let zero_str: String = std::iter::repeat(zero_digit).take(padding).collect();
    format!("{}{}", zero_str, digits)
}

/// Convert a number to a string using the specified Unicode zero digit.
fn decimal_digits(value: u32, zero_digit: char) -> String {
    if zero_digit == '0' {
        return value.to_string();
    }
    // Map each ASCII digit to the corresponding digit in the target family
    let zero_code = zero_digit as u32;
    value
        .to_string()
        .chars()
        .map(|c| {
            let digit_val = c as u32 - '0' as u32;
            char::from_u32(zero_code + digit_val).unwrap_or(c)
        })
        .collect()
}

/// Format a number as alphabetic: 1=A, 2=B, ..., 26=Z, 27=AA, etc.
fn format_alpha(value: u32, uppercase: bool) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let base_char = if uppercase { 'A' } else { 'a' };
    let mut result = String::new();
    let mut n = value;
    while n > 0 {
        n -= 1;
        let ch = char::from_u32(base_char as u32 + (n % 26)).unwrap_or('?');
        result.insert(0, ch);
        n /= 26;
    }
    result
}

// ── Month formatting ────────────────────────────────────────────────────────

/// Format a month component — handles both numeric and name-based formats.
fn format_month(
    month: u32,
    marker: &PictureMarker,
    language: Option<&str>,
    result: &mut String,
) -> error::Result<()> {
    match &marker.primary {
        PrimaryFormat::NameUpper | PrimaryFormat::NameLower | PrimaryFormat::NameTitle => {
            let name = localized_month_name(month, language)?;
            let cased = apply_name_case(&name, &marker.primary);
            let truncated = apply_name_width(&cased, marker);
            result.push_str(&truncated);
            Ok(())
        }
        // For non-name formats, delegate to the generic numeric formatter
        _ => format_numeric_or_name(month, marker, result),
    }
}

// ── Day-of-week formatting ──────────────────────────────────────────────────

/// Format day-of-week — can be a name or a number (1=Monday..7=Sunday).
fn format_day_of_week(
    dt: &chrono::NaiveDateTime,
    marker: &PictureMarker,
    language: Option<&str>,
    result: &mut String,
) -> error::Result<()> {
    let day_idx = dt.weekday().num_days_from_monday() as usize;

    match &marker.primary {
        PrimaryFormat::NameUpper | PrimaryFormat::NameLower | PrimaryFormat::NameTitle => {
            let name = localized_day_name(day_idx, language);
            let cased = apply_name_case(&name, &marker.primary);
            let truncated = apply_name_width(&cased, marker);
            result.push_str(&truncated);
            Ok(())
        }
        // Numeric: 1=Monday, 7=Sunday (ISO numbering)
        _ => {
            let day_num = (day_idx as u32) + 1;
            format_numeric_or_name(day_num, marker, result)
        }
    }
}

// ── AM/PM formatting ────────────────────────────────────────────────────────

/// Format the P (AM/PM) component with case support.
fn format_ampm(
    dt: &chrono::NaiveDateTime,
    marker: &PictureMarker,
    language: Option<&str>,
    result: &mut String,
) -> error::Result<()> {
    let is_am = dt.hour() < 12;
    let localized = localized_ampm(is_am, language);
    let text = match &marker.primary {
        PrimaryFormat::NameUpper => localized.to_uppercase(),
        PrimaryFormat::NameTitle => {
            let mut chars = localized.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.extend(chars.flat_map(|c| c.to_lowercase()));
                    s
                }
                None => String::new(),
            }
        }
        // Default and NameLower: lowercase
        _ => localized.to_lowercase(),
    };
    result.push_str(&text);
    Ok(())
}

// ── Era formatting ──────────────────────────────────────────────────────────

/// Format the E (era) component with case support.
fn format_era(
    dt: &chrono::NaiveDateTime,
    marker: &PictureMarker,
    result: &mut String,
) -> error::Result<()> {
    let is_ad = dt.year() > 0;
    let text = match &marker.primary {
        PrimaryFormat::NameLower => {
            if is_ad { "ad" } else { "bc" }
        }
        PrimaryFormat::NameTitle => {
            if is_ad { "Ad" } else { "Bc" }
        }
        // Default and NameUpper
        _ => {
            if is_ad { "AD" } else { "BC" }
        }
    };
    result.push_str(text);
    Ok(())
}

// ── Fractional seconds ──────────────────────────────────────────────────────

/// Format fractional seconds. Unlike other components, fractional seconds
/// are formatted left-to-right (most significant digit first) and max-width
/// truncates from the right. Trailing zeros are stripped down to min-width.
fn format_fractional_seconds(
    dt: &chrono::NaiveDateTime,
    marker: &PictureMarker,
    result: &mut String,
) -> error::Result<()> {
    let nanos = dt.nanosecond() % 1_000_000_000;

    // Determine precision (number of decimal places).
    // For fractional seconds, the digit pattern width directly determines the
    // number of decimal places: [f01] = 2 places, [f001] = 3 places.
    // An explicit width modifier overrides the digit pattern.
    let min_w = marker.min_width(1);
    let max_w = marker.max_width().unwrap_or_else(|| {
        // No explicit width modifier: use digit pattern as max precision.
        // If the digit pattern is just "1" (the default), use full available
        // precision (up to 9 digits for nanoseconds).
        let pattern = marker.digit_pattern_width().unwrap_or(1);
        if pattern <= 1 {
            // Default: use enough precision to show all significant digits
            // We use 9 (nanosecond precision) as the maximum, then strip trailing zeros
            9
        } else {
            pattern
        }
    });
    let precision = max_w.max(min_w);

    // Convert nanos to the desired precision
    // nanos is in 10^-9; we want 10^-precision digits
    let divisor = 10u64.pow(9u32.saturating_sub(precision));
    let value = (nanos as u64 / divisor) as u32;

    // For fractional seconds, strip trailing zeros down to min_width.
    // E.g., [f,1-4] with value 0.456 → precision=4 → raw "4560" → strip to "456".
    let zero_digit = match &marker.primary {
        PrimaryFormat::Decimal { zero_digit, .. } => *zero_digit,
        _ => '0',
    };
    let raw = format_decimal(value, zero_digit, precision);
    let min_chars = min_w as usize;

    // Strip trailing zero-digits (in the appropriate digit family)
    let stripped: String = {
        let chars: Vec<char> = raw.chars().collect();
        let mut end = chars.len();
        while end > min_chars && chars[end - 1] == zero_digit {
            end -= 1;
        }
        chars[..end].iter().collect()
    };

    result.push_str(&stripped);

    // Append ordinal suffix if requested
    if marker.secondary == SecondaryModifier::Ordinal {
        result.push_str(number_words::ordinal_suffix(value as i64));
    }

    Ok(())
}

// ── Timezone formatting ─────────────────────────────────────────────────────

/// Format the Z or z (timezone) component.
///
/// Per the spec, the presentation modifier determines the format:
/// - Default/numeric: ±HH:MM (or Z for UTC)
/// - "0" pattern variations for different separator styles
/// - "N"/"n" for timezone name (not widely available, falls back to offset)
fn format_timezone(
    offset: Option<&chrono::FixedOffset>,
    marker: &PictureMarker,
    result: &mut String,
) -> error::Result<()> {
    let Some(offset) = offset else {
        // No timezone info available — output nothing (per spec)
        return Ok(());
    };

    let total_secs = offset.fix().local_minus_utc();

    // For 'z' component, prefix with "GMT"
    if marker.component == 'z' {
        result.push_str("GMT");
    }

    // For 'Z' with UTC and default format, output "+00:00" (not "Z").
    // The literal "Z" form is only used when explicitly requested.
    format_tz_numeric(total_secs, marker, result);
    Ok(())
}

/// Format a timezone offset as a numeric string (the part after "GMT" for `z`,
/// or the full value for `Z`).
///
/// The format depends on the width modifier and pattern width:
///
/// | Picture      | pattern_width | min_width | Example         |
/// |--------------|---------------|-----------|-----------------|
/// | `[Z]`        | 1 (default)   | —         | `+05:30`        |
/// | `[Z0000]`    | 4             | —         | `+0530`         |
/// | `[z]`        | 1 (default)   | —         | `GMT+05:30`     |
/// | `[z,2-2]`    | 1 (default)   | 2         | `GMT+5:30`      |
/// | `[z0]`       | 1             | —         | `GMT+5:30`      |
fn format_tz_numeric(total_secs: i32, marker: &PictureMarker, result: &mut String) {
    let sign = if total_secs < 0 { '-' } else { '+' };
    let abs_secs = total_secs.unsigned_abs();
    let hours = abs_secs / 3600;
    let minutes = (abs_secs % 3600) / 60;

    result.push(sign);

    let pattern_width = if let PrimaryFormat::Decimal { pattern_width, .. } = &marker.primary {
        *pattern_width
    } else {
        1
    };

    // Determine whether to zero-pad hours.
    // [z,2-2] → min_width=2 → pad to 2; [z0] → pattern_width=1 → no pad.
    // Default (no explicit width or pattern) → pad to 2.
    let min_hour_digits = if marker.has_modifier {
        marker.min_width(1) as usize
    } else {
        2
    };

    // Determine whether to always include minutes or only when non-zero.
    // Default (no modifier) → always include `:MM` (effective max is 6).
    // [z,6-6] → max 6 → always include.
    // [z,2-2] → max 2 → omit when zero.
    // [z0] → has_modifier but no explicit max → compact (omit when zero).
    let effective_max = marker.max_width().unwrap_or(if marker.has_modifier { 2 } else { 6 });
    let always_show_minutes = effective_max >= 5;

    let use_colon = pattern_width != 4;

    // Hours
    if min_hour_digits >= 2 {
        result.push_str(&format!("{:02}", hours));
    } else {
        result.push_str(&format!("{}", hours));
    }

    // Minutes
    if minutes != 0 || always_show_minutes {
        if use_colon {
            result.push(':');
        }
        result.push_str(&format!("{:02}", minutes));
    }
}

// ── Week of month ───────────────────────────────────────────────────────────

/// Calculate ISO week-of-month per XPath 3.1 §9.8.4.8.
///
/// Each Monday-to-Sunday week belongs to the month in which its Thursday
/// falls.  The weeks belonging to a month are numbered starting from 1.
/// For dates whose Mon–Sun block straddles a month boundary, the week
/// number is relative to the date's own calendar month.
fn iso_week_of_month(dt: &chrono::NaiveDateTime) -> u32 {
    let day = dt.day() as i32;
    let weekday = dt.weekday().num_days_from_monday() as i32; // 0=Mon, 6=Sun

    // Weekday of the 1st of this month (0=Mon, 6=Sun)
    let first_weekday = ((weekday - (day - 1) % 7) + 7) % 7;

    // Monday of the first ISO week whose Thursday falls in this month.
    // If the month starts Mon–Thu (0–3), the week containing the 1st qualifies.
    // If the month starts Fri–Sun (4–6), the next Monday starts the first
    // qualifying week.
    let first_monday = if first_weekday <= 3 {
        1 - first_weekday
    } else {
        8 - first_weekday
    };

    // Monday of the date's Mon–Sun week
    let date_monday = day - weekday;

    ((date_monday - first_monday) / 7 + 1) as u32
}

// ── Name helpers ────────────────────────────────────────────────────────────

/// Apply case transformation to a name string.
fn apply_name_case(name: &str, primary: &PrimaryFormat) -> String {
    match primary {
        PrimaryFormat::NameUpper => name.to_uppercase(),
        PrimaryFormat::NameLower => name.to_lowercase(),
        PrimaryFormat::NameTitle => {
            // Title case: first letter uppercase, rest lowercase.
            // Month/day names are already title-cased, but be explicit.
            let mut chars = name.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    for ch in chars {
                        s.extend(ch.to_lowercase());
                    }
                    s
                }
            }
        }
        _ => name.to_string(),
    }
}

/// Truncate a name to fit within the width constraints.
/// Apply width constraints to a name-format value (month/day name).
///
/// When the full name exceeds max_width, we prefer conventional 3-letter
/// abbreviations (Mon, Tue, Jan, Feb, etc.) if they fit within the min-max
/// range, per the spec's recommendation to use conventional abbreviations.
/// Falls back to truncation to min_width if 3-letter abbreviation is too long.
fn apply_name_width(name: &str, marker: &PictureMarker) -> String {
    let name_len = name.chars().count() as u32;
    let max = marker.width.max.unwrap_or(u32::MAX);
    let min = marker.width.min.unwrap_or(0);

    if name_len <= max {
        // Full name fits — use it, possibly right-padded to min
        return name.to_string();
    }

    // Name exceeds max_width. Use conventional 3-letter abbreviation if it fits.
    const CONVENTIONAL_ABBREV_LEN: u32 = 3;
    if CONVENTIONAL_ABBREV_LEN >= min && CONVENTIONAL_ABBREV_LEN <= max {
        return name.chars().take(CONVENTIONAL_ABBREV_LEN as usize).collect();
    }

    // Otherwise truncate to min_width (capped at max)
    let target = min.min(max);
    name.chars().take(target as usize).collect()
}

/// Get a localized month name, falling back to English if the locale doesn't resolve.
fn localized_month_name(month: u32, language: Option<&str>) -> error::Result<String> {
    let locale = language.and_then(locale_data::resolve_locale);
    if let Some(locale) = locale {
        if let Some(name) = locale_data::month_name(month, locale) {
            return Ok(name.to_string());
        }
    }
    // Fallback to English
    let en = pure_rust_locales::Locale::en_US;
    locale_data::month_name(month, en)
        .map(|s| s.to_string())
        .ok_or(error::Error::FOFD1340)
}

/// Get a localized day name, falling back to English.
fn localized_day_name(day_from_monday: usize, language: Option<&str>) -> String {
    let locale = language.and_then(locale_data::resolve_locale);
    if let Some(locale) = locale {
        if let Some(name) = locale_data::day_name(day_from_monday, locale) {
            return name.to_string();
        }
    }
    // Fallback to English. unwrap_or is safe: day_from_monday always comes from
    // chrono::Weekday (0..6), so the index is always valid for en_US.
    locale_data::day_name(day_from_monday, pure_rust_locales::Locale::en_US)
        .unwrap_or("Monday")
        .to_string()
}

/// Get a localized AM/PM string.
///
/// Returns an empty string when the locale has no AM/PM concept (e.g. 24-hour
/// cultures like German). This allows stylesheet authors to detect 24h locales
/// by probing `format-time($t, '[P]', $lang, (), ())` and adapting the picture
/// string accordingly. Per spec §9.8.4.7, the `[P]` output is entirely
/// implementation-defined.
fn localized_ampm(is_am: bool, language: Option<&str>) -> String {
    let locale = language.and_then(locale_data::resolve_locale);
    if let Some(locale) = locale {
        if let Some(s) = locale_data::ampm_str(is_am, locale) {
            return s.to_string();
        }
        // Locale exists but has no AM/PM strings (24h culture) — return empty.
        return String::new();
    }
    // No locale resolved — fall back to English AM/PM.
    if is_am { "AM".to_string() } else { "PM".to_string() }
}

fn format_roman_number(number: u32, uppercase: bool) -> error::Result<String> {
    if number == 0 {
        return Ok("0".to_string());
    }

    let mut value = u64::from(number);
    let numerals = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    for (magnitude, numeral) in numerals {
        while value >= magnitude {
            result.push_str(numeral);
            value -= magnitude;
        }
    }

    if uppercase {
        Ok(result)
    } else {
        Ok(result.to_ascii_lowercase())
    }
}

fn check_date_component(kind: DateTimeKind, _component: char) -> error::Result<()> {
    match kind {
        DateTimeKind::Time => Err(error::Error::FOFD1350),
        _ => Ok(()),
    }
}

fn check_time_component(kind: DateTimeKind, _component: char) -> error::Result<()> {
    match kind {
        DateTimeKind::Date => Err(error::Error::FOFD1350),
        _ => Ok(()),
    }
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(date_time),
        wrap_xpath_fn!(year_from_date_time),
        wrap_xpath_fn!(month_from_date_time),
        wrap_xpath_fn!(day_from_date_time),
        wrap_xpath_fn!(hours_from_date_time),
        wrap_xpath_fn!(minutes_from_date_time),
        wrap_xpath_fn!(seconds_from_date_time),
        wrap_xpath_fn!(timezone_from_date_time),
        wrap_xpath_fn!(year_from_date),
        wrap_xpath_fn!(month_from_date),
        wrap_xpath_fn!(day_from_date),
        wrap_xpath_fn!(timezone_from_date),
        wrap_xpath_fn!(hours_from_time),
        wrap_xpath_fn!(minutes_from_time),
        wrap_xpath_fn!(seconds_from_time),
        wrap_xpath_fn!(timezone_from_time),
        wrap_xpath_fn!(adjust_date_time_to_timezone1),
        wrap_xpath_fn!(adjust_date_time_to_timezone2),
        wrap_xpath_fn!(adjust_date_to_timezone1),
        wrap_xpath_fn!(adjust_date_to_timezone2),
        wrap_xpath_fn!(adjust_time_to_timezone1),
        wrap_xpath_fn!(adjust_time_to_timezone2),
        wrap_xpath_fn!(parse_ietf_date),
        wrap_xpath_fn!(format_date_time2),
        wrap_xpath_fn!(format_date_time5),
        wrap_xpath_fn!(format_date2),
        wrap_xpath_fn!(format_date5),
        wrap_xpath_fn!(format_time2),
        wrap_xpath_fn!(format_time5),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt_dt(dt_str: &str, picture: &str) -> String {
        let dt = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%dT%H:%M:%S%.f")
            .unwrap();
        format_datetime_picture(&dt, None, picture, DateTimeKind::DateTime, None, None)
            .unwrap()
    }

    fn fmt_dt_lang(dt_str: &str, picture: &str, lang: &str) -> String {
        let dt = chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%dT%H:%M:%S%.f")
            .unwrap();
        format_datetime_picture(&dt, None, picture, DateTimeKind::DateTime, Some(lang), None)
            .unwrap()
    }

    #[test]
    fn test_year_width_specifiers() {
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,4-4]"), "0985");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,3-4]"), "985");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,2-5]"), "985");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,2-2]"), "85");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,2-*]"), "985");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,*-4]"), "985");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[Y,3]"), "985");
    }

    #[test]
    fn test_month_width_specifiers() {
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,4-4]"), "0003");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,1-4]"), "3");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,2-5]"), "03");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,2-2]"), "03");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,1-*]"), "3");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,*-2]"), "3");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[M,3]"), "003");
    }

    #[test]
    fn test_fractional_seconds_width() {
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,4-4]"), "4560");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,1-4]"), "456");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,2-5]"), "456");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,2-2]"), "45");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,1-*]"), "456");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,*-2]"), "45");
        assert_eq!(fmt_dt("0985-03-01T09:15:06.456", "[f,3]"), "456");
    }

    // ── Localized formatting tests ──────────────────────────────────────────

    #[test]
    fn test_localized_month_name() {
        // German March — NameTitle
        assert_eq!(
            fmt_dt_lang("2024-03-15T10:00:00.0", "[MNn]", "de"),
            "März"
        );
        // French July — NameTitle capitalizes first letter
        assert_eq!(
            fmt_dt_lang("2024-07-15T10:00:00.0", "[MNn]", "fr"),
            "Juillet"
        );
        // French July — NameLower
        assert_eq!(
            fmt_dt_lang("2024-07-15T10:00:00.0", "[Mn]", "fr"),
            "juillet"
        );
        // English (explicit) January
        assert_eq!(
            fmt_dt_lang("2024-01-15T10:00:00.0", "[MNn]", "en"),
            "January"
        );
    }

    #[test]
    fn test_localized_day_name() {
        // 2024-03-15 is a Friday
        // [FNn] is NameTitle (default for F)
        assert_eq!(
            fmt_dt_lang("2024-03-15T10:00:00.0", "[FNn]", "de"),
            "Freitag"
        );
        // Swedish: NameTitle capitalizes first letter
        assert_eq!(
            fmt_dt_lang("2024-03-15T10:00:00.0", "[FNn]", "sv"),
            "Fredag"
        );
        // Swedish lowercase
        assert_eq!(
            fmt_dt_lang("2024-03-15T10:00:00.0", "[Fn]", "sv"),
            "fredag"
        );
    }

    #[test]
    fn test_localized_ampm() {
        // English AM (morning)
        assert_eq!(
            fmt_dt_lang("2024-03-15T09:00:00.0", "[PN]", "en"),
            "AM"
        );
        // English PM (afternoon)
        assert_eq!(
            fmt_dt_lang("2024-03-15T15:00:00.0", "[Pn]", "en"),
            "pm"
        );
        // German locale has no AM/PM strings (24h culture);
        // returns empty string so stylesheet authors can detect and adapt.
        // Per spec §9.8.4.7, P output is entirely implementation-defined.
        assert_eq!(
            fmt_dt_lang("2024-03-15T09:00:00.0", "[PN]", "de"),
            ""
        );
        assert_eq!(
            fmt_dt_lang("2024-03-15T15:00:00.0", "[Pn]", "de"),
            ""
        );
    }

    #[test]
    fn test_ampm_case_variants() {
        // NameLower: [Pn]
        assert_eq!(
            fmt_dt_lang("2024-03-15T09:00:00.0", "[Pn]", "en"),
            "am"
        );
        // NameUpper: [PN]
        assert_eq!(
            fmt_dt_lang("2024-03-15T09:00:00.0", "[PN]", "en"),
            "AM"
        );
        // NameTitle: [PNn]
        assert_eq!(
            fmt_dt_lang("2024-03-15T09:00:00.0", "[PNn]", "en"),
            "Am"
        );
        assert_eq!(
            fmt_dt_lang("2024-03-15T15:00:00.0", "[PNn]", "en"),
            "Pm"
        );
    }

    #[test]
    fn test_unknown_language_fallback() {
        // Unknown language should prefix with [Language: en] and use English
        assert_eq!(
            fmt_dt_lang("2024-03-15T10:00:00.0", "[MNn]", "xx"),
            "[Language: en]March"
        );
    }
}
