// https://www.w3.org/TR/xpath-functions-31/#numeric-functions
use std::cmp::Ordering;

use ahash::random_state::RandomState;
use ibig::ops::Abs;
use ibig::IBig;
use icu_properties::{maps, GeneralCategory};
use num_traits::Float;
use rand::prelude::*;
use rand_xoshiro::SplitMix64;

use xee_name::{Name, FN_NAMESPACE};
use xee_xpath_ast::parse_name;
use xee_xpath_macros::xpath_fn;

use crate::atomic::round_atomic;
use crate::atomic::round_half_to_even_atomic;
use crate::atomic::Atomic;
use crate::context;
use crate::error;
use crate::function;
use crate::function::StaticFunctionDescription;
use crate::interpreter::Interpreter;
use crate::sequence;
use crate::wrap_xpath_fn;

#[xpath_fn("fn:abs($arg as xs:numeric?) as xs:numeric?")]
fn abs(arg: Option<Atomic>) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        match arg {
            Atomic::Integer(_, i) => Ok(Some(i.as_ref().abs().into())),
            Atomic::Decimal(d) => Ok(Some(d.abs().into())),
            Atomic::Float(f) => Ok(Some(f.abs().into())),
            Atomic::Double(d) => Ok(Some(d.abs().into())),
            _ => Err(error::Error::XPTY0004),
        }
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:ceiling($arg as xs:numeric?) as xs:numeric?")]
fn ceiling(arg: Option<Atomic>) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        match arg {
            Atomic::Integer(_, _) => Ok(Some(arg.clone())),
            Atomic::Decimal(d) => Ok(Some(d.ceil().into())),
            Atomic::Float(f) => Ok(Some(f.ceil().into())),
            Atomic::Double(d) => Ok(Some(d.ceil().into())),
            _ => Err(error::Error::XPTY0004),
        }
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:floor($arg as xs:numeric?) as xs:numeric?")]
fn floor(arg: Option<Atomic>) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        match arg {
            Atomic::Integer(_, _) => Ok(Some(arg.clone())),
            Atomic::Decimal(d) => Ok(Some(d.floor().into())),
            Atomic::Float(f) => Ok(Some(f.floor().into())),
            Atomic::Double(d) => Ok(Some(d.floor().into())),
            _ => Err(error::Error::XPTY0004),
        }
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:round($arg as xs:numeric?) as xs:numeric?")]
fn round1(arg: Option<Atomic>) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        round_atomic(arg, 0).map(Some)
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:round($arg as xs:numeric?, $precision as xs:integer) as xs:numeric?")]
fn round2(arg: Option<Atomic>, precision: IBig) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        let precision: i32 = precision.try_into().map_err(|_| error::Error::FOAR0002)?;
        round_atomic(arg, precision).map(Some)
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:round-half-to-even($arg as xs:numeric?) as xs:numeric?")]
fn round_half_to_even1(arg: Option<Atomic>) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        round_half_to_even_atomic(arg, 0).map(Some)
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:round-half-to-even($arg as xs:numeric?, $precision as xs:integer) as xs:numeric?")]
fn round_half_to_even2(arg: Option<Atomic>, precision: IBig) -> error::Result<Option<Atomic>> {
    if let Some(arg) = arg {
        let precision: i32 = precision.try_into().map_err(|_| error::Error::FOAR0002)?;
        round_half_to_even_atomic(arg, precision).map(Some)
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:number($arg as xs:anyAtomicType?) as xs:double", context_first)]
fn number(arg: Option<Atomic>) -> error::Result<Atomic> {
    if let Some(arg) = arg {
        match arg.cast_to_double() {
            Ok(d) => Ok(d),
            Err(_) => Ok(f64::NAN.into()),
        }
    } else {
        Ok(f64::NAN.into())
    }
}

#[xpath_fn("fn:format-number($value as xs:anyAtomicType?, $picture as xs:string) as xs:string")]
fn format_number2(
    context: &context::DynamicContext,
    value: Option<Atomic>,
    picture: &str,
) -> error::Result<String> {
    format_number(context, value, picture, None)
}

#[xpath_fn(
    "fn:format-number($value as xs:anyAtomicType?, $picture as xs:string, $decimal_format_name as xs:string) as xs:string"
)]
fn format_number3(
    context: &context::DynamicContext,
    value: Option<Atomic>,
    picture: &str,
    decimal_format_name: &str,
) -> error::Result<String> {
    format_number(context, value, picture, Some(decimal_format_name))
}

#[derive(Debug, Clone)]
struct FormatPicture {
    positive: SubPicture,
    negative: Option<SubPicture>,
}

#[derive(Debug, Clone)]
struct SubPicture {
    prefix: String,
    suffix: String,
    min_integer: usize,
    min_fraction: usize,
    max_fraction: usize,
    min_exponent: usize,
    grouping_positions: Vec<usize>,
    repeat_grouping: Option<usize>,
    multiplier: i32,
}

#[derive(Debug, Clone)]
struct ParsedNumber {
    negative: bool,
    digits: Vec<u8>,
    scale: i32,
}

impl ParsedNumber {
    fn from_atomic(value: &Atomic) -> error::Result<Self> {
        Self::from_lexical(&value.string_value())
    }

    fn from_lexical(lexical: &str) -> error::Result<Self> {
        let (negative, lexical) = if let Some(rest) = lexical.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = lexical.strip_prefix('+') {
            (false, rest)
        } else {
            (false, lexical)
        };

        let (mantissa, exponent) = if let Some((mantissa, exponent)) = lexical.split_once('E') {
            (
                mantissa,
                exponent.parse::<i32>().map_err(|_| error::Error::FODF1310)?,
            )
        } else {
            (lexical, 0)
        };

        let (integer_part, fraction_part) = if let Some((integer_part, fraction_part)) = mantissa.split_once('.') {
            (integer_part, fraction_part)
        } else {
            (mantissa, "")
        };

        let mut digits = integer_part
            .chars()
            .chain(fraction_part.chars())
            .map(|c| c.to_digit(10).ok_or(error::Error::FODF1310).map(|digit| digit as u8))
            .collect::<error::Result<Vec<u8>>>()?;
        let mut scale = fraction_part.chars().count() as i32 - exponent;

        while digits.len() > 1 && digits.first() == Some(&0) {
            digits.remove(0);
        }
        while digits.len() > 1 && digits.last() == Some(&0) && scale > 0 {
            digits.pop();
            scale -= 1;
        }

        if digits.iter().all(|digit| *digit == 0) {
            digits = vec![0];
            scale = 0;
        }

        Ok(Self {
            negative,
            digits,
            scale,
        })
    }

    fn multiply_by_power_of_ten(&mut self, power: i32) {
        self.scale -= power;
    }

    fn rounded_parts(&self, max_fraction: usize) -> (Vec<u8>, usize, Vec<u8>) {
        let (mut digits, mut integer_len) = self.expanded_digits();
        let keep = integer_len + max_fraction;

        match keep.cmp(&digits.len()) {
            Ordering::Greater => {
                digits.resize(keep, 0);
            }
            Ordering::Equal => {}
            Ordering::Less => {
                let round_digit = digits[keep];
                let has_following_non_zero = digits[keep + 1..].iter().any(|digit| *digit != 0);
                let round_up = round_digit > 5
                    || (round_digit == 5
                        && (has_following_non_zero
                            || (keep > 0 && digits[keep - 1] % 2 == 1)));
                digits.truncate(keep);

                if round_up {
                    let mut index = keep;
                    while index > 0 {
                        index -= 1;
                        if digits[index] < 9 {
                            digits[index] += 1;
                            break;
                        }
                        digits[index] = 0;
                    }
                    if index == 0 && digits.first() == Some(&0) {
                        digits.insert(0, 1);
                        integer_len += 1;
                    }
                }
            }
        }

        if digits.is_empty() {
            digits.push(0);
            integer_len = 1;
        }

        let fraction_digits = if integer_len <= digits.len() {
            digits.split_off(integer_len)
        } else {
            Vec::new()
        };
        (digits, integer_len, fraction_digits)
    }

    fn expanded_digits(&self) -> (Vec<u8>, usize) {
        let mut digits = self.digits.clone();
        let integer_len = digits.len() as i32 - self.scale;
        if integer_len < 0 {
            let mut prefixed = vec![0; integer_len.unsigned_abs() as usize];
            prefixed.extend(digits);
            digits = prefixed;
            return (digits, 0);
        }

        let integer_len = integer_len as usize;
        if integer_len > digits.len() {
            digits.resize(integer_len, 0);
        }
        (digits, integer_len)
    }
}

fn format_number(
    context: &context::DynamicContext,
    value: Option<Atomic>,
    picture: &str,
    decimal_format_name: Option<&str>,
) -> error::Result<String> {
    let decimal_format = resolve_decimal_format(context, decimal_format_name)?;
    let picture = parse_picture(picture, decimal_format)
        .map_err(|error| map_format_number_picture_error(context, error))?;

    let value = normalized_format_number_input(value);
    let Some(value) = value else {
        return Ok(decimal_format.nan.clone());
    };

    if value.is_nan() {
        return Ok(decimal_format.nan.clone());
    }

    if value.is_infinite() {
        return Ok(format_infinite(&picture, decimal_format, is_negative_number(&value)));
    }

    let mut number = ParsedNumber::from_atomic(&value)?;
    Ok(format_parsed_number(
        &mut number,
        &picture,
        decimal_format,
    ))
}

pub(crate) fn format_number_from_lexical(
    context: &context::DynamicContext,
    lexical: &str,
    picture: &str,
    decimal_format_name: Option<&str>,
) -> error::Result<String> {
    let decimal_format = resolve_decimal_format(context, decimal_format_name)?;
    let picture = parse_picture(picture, decimal_format)
        .map_err(|error| map_format_number_picture_error(context, error))?;
    let mut number = ParsedNumber::from_lexical(lexical)
        .map_err(|error| map_format_number_picture_error(context, error))?;
    Ok(format_parsed_number(
        &mut number,
        &picture,
        decimal_format,
    ))
}

fn format_parsed_number(
    number: &mut ParsedNumber,
    picture: &FormatPicture,
    decimal_format: &context::DecimalFormatSymbols,
) -> String {
    let negative = number.negative;
    number.negative = false;

    let subpicture = if negative {
        picture.negative.as_ref().unwrap_or(&picture.positive)
    } else {
        &picture.positive
    };
    let mut exponent = 0i32;
    if subpicture.min_exponent > 0 && !number.is_zero() {
        exponent = number.integer_digit_count() - subpicture.min_integer as i32;
    }
    number.multiply_by_power_of_ten(subpicture.multiplier - exponent);

    let min_integer = subpicture.min_integer.max(1);
    let (mut integer_digits, _, mut fraction_digits) = number.rounded_parts(subpicture.max_fraction);
    if subpicture.min_exponent > 0 && integer_digits.len() > min_integer {
        let extra_exponent = (integer_digits.len() - min_integer) as i32;
        exponent += extra_exponent;
        number.multiply_by_power_of_ten(-extra_exponent);
        (integer_digits, _, fraction_digits) = number.rounded_parts(subpicture.max_fraction);
    }
    while fraction_digits.len() > subpicture.min_fraction && fraction_digits.last() == Some(&0) {
        fraction_digits.pop();
    }

    let mut integer = digits_to_ascii_string(&integer_digits);
    integer = integer.trim_start_matches('0').to_string();
    if integer.is_empty() {
        integer = "0".repeat(min_integer);
    } else if integer.len() < min_integer {
        integer = format!("{}{}", "0".repeat(min_integer - integer.len()), integer);
    }
    integer = apply_grouping(
        &integer,
        &subpicture.grouping_positions,
        subpicture.repeat_grouping,
        decimal_format.grouping_separator,
    );

    let mut result = String::new();
    if negative {
        if let Some(negative_subpicture) = &picture.negative {
            result.push_str(&negative_subpicture.prefix);
        } else {
            result.push(decimal_format.minus_sign);
            result.push_str(&picture.positive.prefix);
        }
    } else {
        result.push_str(&picture.positive.prefix);
    }
    result.push_str(&substitute_digits(&integer, decimal_format.zero_digit));
    if !fraction_digits.is_empty() {
        result.push(decimal_format.decimal_separator);
        result.push_str(&substitute_digits(
            &digits_to_ascii_string(&fraction_digits),
            decimal_format.zero_digit,
        ));
    }
    if subpicture.min_exponent > 0 {
        result.push(decimal_format.exponent_separator);
        if exponent < 0 {
            result.push(decimal_format.minus_sign);
        }
        let exponent_digits = exponent.abs().to_string();
        if exponent_digits.len() < subpicture.min_exponent {
            result.push_str(&"0".repeat(subpicture.min_exponent - exponent_digits.len()));
        }
        result.push_str(&substitute_digits(&exponent_digits, decimal_format.zero_digit));
    }
    if negative {
        if let Some(negative_subpicture) = &picture.negative {
            result.push_str(&negative_subpicture.suffix);
        } else {
            result.push_str(&picture.positive.suffix);
        }
    } else {
        result.push_str(&picture.positive.suffix);
    }
    result
}

fn normalized_format_number_input(value: Option<Atomic>) -> Option<Atomic> {
    let value = value?;
    if value.is_numeric() {
        return Some(value);
    }

    Some(match value.cast_to_double() {
        Ok(value) => value,
        Err(_) => f64::NAN.into(),
    })
}

fn resolve_decimal_format<'a>(
    context: &'a context::DynamicContext,
    decimal_format_name: Option<&str>,
) -> error::Result<&'a context::DecimalFormatSymbols> {
    if let Some(decimal_format_name) = decimal_format_name {
        let name = parse_name(decimal_format_name, context.static_context().namespaces())
            .map_err(|_| error::Error::FODF1280)?
            .value;
        context
            .static_context()
            .decimal_format(Some(&name))
            .ok_or(error::Error::FODF1280)
    } else {
        Ok(context.static_context().default_decimal_format())
    }
}

fn format_infinite(
    picture: &FormatPicture,
    decimal_format: &context::DecimalFormatSymbols,
    negative: bool,
) -> String {
    let mut result = String::new();
    if negative {
        if let Some(negative_subpicture) = &picture.negative {
            result.push_str(&negative_subpicture.prefix);
            result.push_str(&decimal_format.infinity);
            result.push_str(&negative_subpicture.suffix);
        } else {
            result.push(decimal_format.minus_sign);
            result.push_str(&picture.positive.prefix);
            result.push_str(&decimal_format.infinity);
            result.push_str(&picture.positive.suffix);
        }
    } else {
        result.push_str(&picture.positive.prefix);
        result.push_str(&decimal_format.infinity);
        result.push_str(&picture.positive.suffix);
    }
    result
}

fn map_format_number_picture_error(
    context: &context::DynamicContext,
    error: error::Error,
) -> error::Error {
    if error == error::Error::FODF1310
        && context.static_context().processor_xslt_version() == Some(2)
    {
        error::Error::XTDE1310
    } else {
        error
    }
}

fn parse_picture(
    picture: &str,
    decimal_format: &context::DecimalFormatSymbols,
) -> error::Result<FormatPicture> {
    let parts = picture
        .split(decimal_format.pattern_separator)
        .collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 2 {
        return Err(error::Error::FODF1310);
    }

    Ok(FormatPicture {
        positive: parse_subpicture(parts[0], decimal_format)?,
        negative: parts
            .get(1)
            .map(|subpicture| parse_subpicture(subpicture, decimal_format))
            .transpose()?,
    })
}

fn parse_subpicture(
    subpicture: &str,
    decimal_format: &context::DecimalFormatSymbols,
) -> error::Result<SubPicture> {
    let chars = subpicture.chars().collect::<Vec<_>>();
    let first_active = chars
        .iter()
        .position(|c| is_active_picture_char(*c, decimal_format))
        .ok_or(error::Error::FODF1310)?;
    let last_active = chars
        .iter()
        .rposition(|c| is_active_picture_char(*c, decimal_format))
        .ok_or(error::Error::FODF1310)?;
    let prefix = chars[..first_active].iter().collect::<String>();
    let suffix = chars[last_active + 1..].iter().collect::<String>();
    let active = &chars[first_active..=last_active];
    if active
        .iter()
        .any(|c| !is_active_picture_char(*c, decimal_format))
    {
        return Err(error::Error::FODF1310);
    }

    let exponent_count = active
        .iter()
        .filter(|c| **c == decimal_format.exponent_separator)
        .count();
    if exponent_count > 1 {
        return Err(error::Error::FODF1310);
    }
    let exponent_index = active
        .iter()
        .position(|c| *c == decimal_format.exponent_separator);
    let (active, exponent_part) = if let Some(exponent_index) = exponent_index {
        (&active[..exponent_index], Some(&active[exponent_index + 1..]))
    } else {
        (active, None)
    };
    let min_exponent = if let Some(exponent_part) = exponent_part {
        if exponent_part.is_empty()
            || exponent_part
                .iter()
                .any(|c| mandatory_digit_value(*c, decimal_format).is_none())
        {
            return Err(error::Error::FODF1310);
        }
        exponent_part.len()
    } else {
        0
    };

    let decimal_count = active
        .iter()
        .filter(|c| **c == decimal_format.decimal_separator)
        .count();
    if decimal_count > 1 {
        return Err(error::Error::FODF1310);
    }
    let split_index = active
        .iter()
        .position(|c| *c == decimal_format.decimal_separator);
    let (integer_part, fraction_part) = if let Some(split_index) = split_index {
        (&active[..split_index], &active[split_index + 1..])
    } else {
        (active, &[][..])
    };
    if integer_part.is_empty() && fraction_part.is_empty() {
        return Err(error::Error::FODF1310);
    }
    if fraction_part
        .iter()
        .any(|c| *c == decimal_format.grouping_separator)
    {
        return Err(error::Error::FODF1310);
    }

    validate_integer_part(integer_part, decimal_format)?;
    validate_fraction_part(fraction_part, decimal_format)?;

    let (grouping_positions, repeat_grouping) = grouping_positions(integer_part, decimal_format)?;
    let min_integer = integer_part
        .iter()
        .filter(|c| mandatory_digit_value(**c, decimal_format).is_some())
        .count();
    let min_fraction = fraction_part
        .iter()
        .filter(|c| mandatory_digit_value(**c, decimal_format).is_some())
        .count();
    let max_fraction = fraction_part
        .iter()
        .filter(|c| is_digit_placeholder(**c, decimal_format))
        .count();
    if min_integer == 0
        && max_fraction == 0
        && !integer_part
            .iter()
            .any(|c| is_digit_placeholder(*c, decimal_format))
    {
        return Err(error::Error::FODF1310);
    }

    let multiplier = scaling_multiplier(&prefix, &suffix, decimal_format)?;
    Ok(SubPicture {
        prefix,
        suffix,
        min_integer,
        min_fraction,
        max_fraction,
        min_exponent,
        grouping_positions,
        repeat_grouping,
        multiplier,
    })
}

fn validate_integer_part(
    integer_part: &[char],
    decimal_format: &context::DecimalFormatSymbols,
) -> error::Result<()> {
    let mut seen_mandatory = false;
    for c in integer_part {
        if *c == decimal_format.grouping_separator {
            continue;
        }
        if mandatory_digit_value(*c, decimal_format).is_some() {
            seen_mandatory = true;
            continue;
        }
        if *c == decimal_format.digit && seen_mandatory {
            return Err(error::Error::FODF1310);
        }
    }
    Ok(())
}

fn validate_fraction_part(
    fraction_part: &[char],
    decimal_format: &context::DecimalFormatSymbols,
) -> error::Result<()> {
    let mut seen_optional = false;
    for c in fraction_part {
        if *c == decimal_format.digit {
            seen_optional = true;
            continue;
        }
        if mandatory_digit_value(*c, decimal_format).is_some() && seen_optional {
            return Err(error::Error::FODF1310);
        }
    }
    Ok(())
}

fn grouping_positions(
    integer_part: &[char],
    decimal_format: &context::DecimalFormatSymbols,
) -> error::Result<(Vec<usize>, Option<usize>)> {
    let mut positions = Vec::new();
    let mut digits_since_group = 0;
    let mut digits_from_right = 0;

    for c in integer_part.iter().rev() {
        if *c == decimal_format.grouping_separator {
            if digits_since_group == 0 {
                return Err(error::Error::FODF1310);
            }
            digits_from_right += digits_since_group;
            positions.push(digits_from_right);
            digits_since_group = 0;
            continue;
        }
        if !is_digit_placeholder(*c, decimal_format) {
            return Err(error::Error::FODF1310);
        }
        digits_since_group += 1;
    }

    let repeat_grouping = match positions.as_slice() {
        [] => None,
        [position] => Some(*position),
        _ => {
            let mut previous = 0;
            let mut intervals = positions
                .iter()
                .map(|position| {
                    let interval = *position - previous;
                    previous = *position;
                    interval
                });
            let first_interval = intervals.next().unwrap_or_default();
            if intervals.all(|interval| interval == first_interval) {
                Some(first_interval)
            } else {
                None
            }
        }
    };

    Ok((positions, repeat_grouping))
}

fn scaling_multiplier(
    prefix: &str,
    suffix: &str,
    decimal_format: &context::DecimalFormatSymbols,
) -> error::Result<i32> {
    let mut percent_count = 0;
    let mut per_mille_count = 0;
    for c in prefix.chars().chain(suffix.chars()) {
        if c == decimal_format.percent {
            percent_count += 1;
        } else if c == decimal_format.per_mille {
            per_mille_count += 1;
        }
    }

    if percent_count + per_mille_count > 1 {
        return Err(error::Error::FODF1310);
    }
    if percent_count == 1 {
        Ok(2)
    } else if per_mille_count == 1 {
        Ok(3)
    } else {
        Ok(0)
    }
}

fn is_active_picture_char(c: char, decimal_format: &context::DecimalFormatSymbols) -> bool {
    is_digit_placeholder(c, decimal_format)
        || c == decimal_format.decimal_separator
        || c == decimal_format.grouping_separator
        || c == decimal_format.exponent_separator
}

fn is_digit_placeholder(c: char, decimal_format: &context::DecimalFormatSymbols) -> bool {
    c == decimal_format.digit || mandatory_digit_value(c, decimal_format).is_some()
}

fn mandatory_digit_value(c: char, decimal_format: &context::DecimalFormatSymbols) -> Option<u32> {
    let zero = decimal_format.zero_digit as u32;
    let value = c as u32;
    let offset = value.checked_sub(zero)?;
    if is_valid_zero_digit(decimal_format.zero_digit) && offset <= 9 && is_decimal_digit(c) {
        Some(offset)
    } else {
        None
    }
}

fn is_valid_zero_digit(zero_digit: char) -> bool {
    if !is_decimal_digit(zero_digit) {
        return false;
    }

    let zero = zero_digit as u32;
    let has_all_following_digits = (1..=9).all(|offset| {
        char::from_u32(zero + offset)
            .map(is_decimal_digit)
            .unwrap_or(false)
    });
    if !has_all_following_digits {
        return false;
    }

    zero.checked_sub(1)
        .and_then(char::from_u32)
        .map(|previous| !is_decimal_digit(previous))
        .unwrap_or(true)
}

fn is_decimal_digit(c: char) -> bool {
    maps::general_category()
        .get_set_for_value(GeneralCategory::DecimalNumber)
    .as_borrowed()
    .contains32(c as u32)
}

fn is_negative_number(value: &Atomic) -> bool {
    match value {
        Atomic::Integer(_, integer) => integer.as_ref() < &0.into(),
        Atomic::Decimal(decimal) => decimal.is_sign_negative(),
        Atomic::Float(f) => f.is_sign_negative(),
        Atomic::Double(d) => d.is_sign_negative(),
        _ => false,
    }
}

fn digits_to_ascii_string(digits: &[u8]) -> String {
    digits
        .iter()
        .map(|digit| char::from(b'0' + *digit))
        .collect()
}

fn apply_grouping(
    digits: &str,
    grouping_positions: &[usize],
    repeat_grouping: Option<usize>,
    separator: char,
) -> String {
    if grouping_positions.is_empty() {
        return digits.to_string();
    }

    let chars = digits.chars().collect::<Vec<_>>();
    let mut all_positions = grouping_positions.to_vec();
    if let Some(interval) = repeat_grouping {
        let mut next_position = all_positions.last().copied().unwrap_or(0) + interval;
        while next_position < chars.len() {
            all_positions.push(next_position);
            next_position += interval;
        }
    }

    let mut insertion_points = all_positions
        .iter()
        .copied()
        .filter(|position| *position < chars.len())
        .map(|position| chars.len() - position)
        .collect::<Vec<_>>();
    if insertion_points.is_empty() {
        return digits.to_string();
    }
    insertion_points.sort_unstable();

    let mut result = String::new();
    let mut insertion_index = 0;
    for (index, c) in chars.into_iter().enumerate() {
        if insertion_index < insertion_points.len() && insertion_points[insertion_index] == index {
            result.push(separator);
            insertion_index += 1;
        }
        result.push(c);
    }
    result
}

fn substitute_digits(digits: &str, zero_digit: char) -> String {
    if zero_digit == '0' {
        return digits.to_string();
    }

    let base = zero_digit as u32;
    digits
        .chars()
        .map(|c| match c.to_digit(10) {
            Some(value) => char::from_u32(base + value).unwrap_or(c),
            None => c,
        })
        .collect()
}

impl ParsedNumber {
    fn integer_digit_count(&self) -> i32 {
        self.digits.len() as i32 - self.scale
    }

    fn is_zero(&self) -> bool {
        self.digits.len() == 1 && self.digits[0] == 0
    }
}

#[xpath_fn("fn:random-number-generator() as map(xs:string, item())")]
fn random_number_generator0(context: &context::DynamicContext) -> error::Result<function::Map> {
    random_number_generator1(context, None)
}

#[xpath_fn("fn:random-number-generator($seed as xs:anyAtomicType?) as map(xs:string, item())")]
fn random_number_generator1(
    context: &context::DynamicContext,
    seed: Option<Atomic>,
) -> error::Result<function::Map> {
    // use a hash function with a fixed seed
    let random_state = RandomState::with_seeds(0, 0, 0, 0);
    let seed = if let Some(seed) = seed {
        random_state.hash_one(seed)
    } else {
        random_state.hash_one(context.current_datetime())
    };
    rng_object(context, seed)
}

fn rng_object(context: &context::DynamicContext, seed: u64) -> error::Result<function::Map> {
    // XPath 3.1 states that all xs:double values in [0.0, 1.0) SHOULD be
    // equally likely, but a mathematically uniform distribution is clearly
    // the intent.
    let number = SplitMix64::seed_from_u64(seed).gen_range(0.0..1.0);
    let static_context = context.static_context();
    let next_name = Name::new(
        "_rng-next".to_string(),
        FN_NAMESPACE.to_string(),
        String::new(),
    );
    let next_id = static_context
        .function_id_by_internal_name(&next_name, 0)
        .unwrap();
    let next = Interpreter::create_static_closure(context, next_id, || Some(seed.into()))?;
    let permute_name = Name::new(
        "_rng-permute".to_string(),
        FN_NAMESPACE.to_string(),
        String::new(),
    );
    let permute_id = static_context
        .function_id_by_internal_name(&permute_name, 1)
        .unwrap();
    let permute = Interpreter::create_static_closure(context, permute_id, || Some(seed.into()))?;
    function::Map::new(vec![
        ("number".into(), number.into()),
        ("next".into(), sequence::Item::from(next).into()),
        ("permute".into(), sequence::Item::from(permute).into()),
    ])
}

#[xpath_fn(
    "fn:_rng-next($seed as xs:unsignedLong) as map(xs:string, item())",
    anonymous_closure
)]
fn rng_next(context: &context::DynamicContext, seed: u64) -> error::Result<function::Map> {
    // this code has the same effect as calling next_u64() on a SplitMix64
    // generator then extracting its state afterward. See the original
    // implementation at:
    // https://github.com/rust-random/rngs/blob/rand_xoshiro-0.6.0/rand_xoshiro/src/splitmix64.rs#L47-L53
    const PHI: u64 = 0x9e3779b97f4a7c15;
    rng_object(context, seed.wrapping_add(PHI))
}

#[xpath_fn(
    "fn:_rng-permute($arg as item()*, $seed as xs:unsignedLong) as item()*",
    anonymous_closure
)]
fn rng_permute(arg: &sequence::Sequence, seed: u64) -> sequence::Sequence {
    // don't use the seed directly, since rejection sampling can cause
    // consecutive seeds in a next() sequence to produce the same shuffle.
    // Adding a level of indirection breaks up this correlation.
    // TODO: mix an argument-based hash into the seed for better randomness.
    let shuffle_seed = SplitMix64::seed_from_u64(seed).next_u64();
    let mut items = arg.iter().collect::<Vec<_>>();
    items.shuffle(&mut SplitMix64::seed_from_u64(shuffle_seed));
    items.into()
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(abs),
        wrap_xpath_fn!(ceiling),
        wrap_xpath_fn!(floor),
        wrap_xpath_fn!(round1),
        wrap_xpath_fn!(round2),
        wrap_xpath_fn!(round_half_to_even1),
        wrap_xpath_fn!(round_half_to_even2),
        wrap_xpath_fn!(number),
        wrap_xpath_fn!(format_number2),
        wrap_xpath_fn!(format_number3),
        wrap_xpath_fn!(random_number_generator0),
        wrap_xpath_fn!(random_number_generator1),
        wrap_xpath_fn!(rng_next),
        wrap_xpath_fn!(rng_permute),
    ]
}
