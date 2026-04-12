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
            || duration.num_seconds() % (60 * 60) != 0
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
    format_datetime_picture(&value.date_time, value.offset.as_ref(), picture, DateTimeKind::DateTime)
        .map(Some)
}

#[xpath_fn("fn:format-dateTime($value as xs:dateTime?, $picture as xs:string, $language as xs:string?, $calendar as xs:string?, $place as xs:string?) as xs:string?")]
fn format_date_time5(
    value: Option<NaiveDateTimeWithOffset>,
    picture: &str,
    _language: Option<&str>,
    _calendar: Option<&str>,
    _place: Option<&str>,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    format_datetime_picture(&value.date_time, value.offset.as_ref(), picture, DateTimeKind::DateTime)
        .map(Some)
}

#[xpath_fn("fn:format-date($value as xs:date?, $picture as xs:string) as xs:string?")]
fn format_date2(
    value: Option<NaiveDateWithOffset>,
    picture: &str,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = value.date.and_hms_opt(0, 0, 0).unwrap();
    format_datetime_picture(&dt, value.offset.as_ref(), picture, DateTimeKind::Date).map(Some)
}

#[xpath_fn("fn:format-date($value as xs:date?, $picture as xs:string, $language as xs:string?, $calendar as xs:string?, $place as xs:string?) as xs:string?")]
fn format_date5(
    value: Option<NaiveDateWithOffset>,
    picture: &str,
    _language: Option<&str>,
    _calendar: Option<&str>,
    _place: Option<&str>,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = value.date.and_hms_opt(0, 0, 0).unwrap();
    format_datetime_picture(&dt, value.offset.as_ref(), picture, DateTimeKind::Date).map(Some)
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
    format_datetime_picture(&dt, value.offset.as_ref(), picture, DateTimeKind::Time).map(Some)
}

#[xpath_fn("fn:format-time($value as xs:time?, $picture as xs:string, $language as xs:string?, $calendar as xs:string?, $place as xs:string?) as xs:string?")]
fn format_time5(
    value: Option<NaiveTimeWithOffset>,
    picture: &str,
    _language: Option<&str>,
    _calendar: Option<&str>,
    _place: Option<&str>,
) -> error::Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let dt = chrono::NaiveDate::from_ymd_opt(2000, 1, 1)
        .unwrap()
        .and_time(value.time);
    format_datetime_picture(&dt, value.offset.as_ref(), picture, DateTimeKind::Time).map(Some)
}

#[derive(Clone, Copy)]
enum DateTimeKind {
    DateTime,
    Date,
    Time,
}

fn format_datetime_picture(
    dt: &chrono::NaiveDateTime,
    offset: Option<&chrono::FixedOffset>,
    picture: &str,
    kind: DateTimeKind,
) -> error::Result<String> {
    let mut result = String::new();
    let mut chars = picture.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '[' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    result.push('[');
                } else {
                    let mut spec = String::new();
                    loop {
                        match chars.next() {
                            Some(']') => break,
                            Some(ch) => spec.push(ch),
                            None => return Err(error::Error::FOFD1340),
                        }
                    }
                    format_component(dt, offset, &spec, kind, &mut result)?;
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

fn format_component(
    dt: &chrono::NaiveDateTime,
    offset: Option<&chrono::FixedOffset>,
    spec: &str,
    kind: DateTimeKind,
    result: &mut String,
) -> error::Result<()> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(error::Error::FOFD1340);
    }

    let component = spec.chars().next().unwrap();
    let presentation = &spec[component.len_utf8()..];

    // Parse the presentation modifier to extract minimum width
    let min_width = parse_min_width(presentation);

    match component {
        'Y' => {
            // Year
            check_date_component(kind, component)?;
            let year = dt.year().unsigned_abs();
            format_number_component(year, min_width.unwrap_or(4), result);
        }
        'M' => {
            // Month
            check_date_component(kind, component)?;
            format_number_component(dt.month(), min_width.unwrap_or(1), result);
        }
        'D' => {
            // Day of month
            check_date_component(kind, component)?;
            format_number_component(dt.day(), min_width.unwrap_or(1), result);
        }
        'd' => {
            // Day of year
            check_date_component(kind, component)?;
            format_number_component(dt.ordinal(), min_width.unwrap_or(1), result);
        }
        'F' => {
            // Day of week (name)
            check_date_component(kind, component)?;
            let day_names = [
                "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
            ];
            let day_idx = dt.weekday().num_days_from_monday() as usize;
            result.push_str(day_names[day_idx]);
        }
        'W' => {
            // Week of year
            check_date_component(kind, component)?;
            let week = dt.iso_week().week();
            format_number_component(week, min_width.unwrap_or(1), result);
        }
        'H' => {
            // Hour (0-23)
            check_time_component(kind, component)?;
            format_number_component(dt.hour(), min_width.unwrap_or(1), result);
        }
        'h' => {
            // Hour (1-12)
            check_time_component(kind, component)?;
            let h = dt.hour() % 12;
            let h = if h == 0 { 12 } else { h };
            format_number_component(h, min_width.unwrap_or(1), result);
        }
        'P' => {
            // AM/PM
            check_time_component(kind, component)?;
            if dt.hour() < 12 {
                result.push_str("am");
            } else {
                result.push_str("pm");
            }
        }
        'm' => {
            // Minutes
            check_time_component(kind, component)?;
            format_number_component(dt.minute(), min_width.unwrap_or(2), result);
        }
        's' => {
            // Seconds
            check_time_component(kind, component)?;
            format_number_component(dt.second(), min_width.unwrap_or(2), result);
        }
        'f' => {
            // Fractional seconds
            check_time_component(kind, component)?;
            let nanos = dt.nanosecond() % 1_000_000_000;
            let millis = nanos / 1_000_000;
            format_number_component(millis, min_width.unwrap_or(1), result);
        }
        'Z' | 'z' => {
            // Timezone
            if let Some(offset) = offset {
                use chrono::Offset;
                let total_secs = offset.fix().local_minus_utc();
                if total_secs == 0 {
                    result.push('Z');
                } else {
                    let sign = if total_secs < 0 { '-' } else { '+' };
                    let abs_secs = total_secs.unsigned_abs();
                    let hours = abs_secs / 3600;
                    let minutes = (abs_secs % 3600) / 60;
                    result.push(sign);
                    result.push_str(&format!("{:02}:{:02}", hours, minutes));
                }
            }
        }
        'C' => {
            // Calendar (always ISO by default)
            result.push_str("ISO");
        }
        'E' => {
            // Era
            check_date_component(kind, component)?;
            if dt.year() > 0 {
                result.push_str("AD");
            } else {
                result.push_str("BC");
            }
        }
        _ => {
            return Err(error::Error::FOFD1340);
        }
    }
    Ok(())
}

fn parse_min_width(presentation: &str) -> Option<u32> {
    // Parse presentation like "0001" (min-width = 4), "01" (min-width = 2),
    // "1" (min-width = 1), or "Nn" / "n" / "N" for names
    let presentation = presentation.trim();
    if presentation.is_empty() {
        return None;
    }
    // Count leading zeros + trailing digit to determine minimum width
    let digits: String = presentation.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    Some(digits.len() as u32)
}

fn format_number_component(value: u32, min_width: u32, result: &mut String) {
    let s = value.to_string();
    let padding = if (s.len() as u32) < min_width {
        min_width as usize - s.len()
    } else {
        0
    };
    for _ in 0..padding {
        result.push('0');
    }
    result.push_str(&s);
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
