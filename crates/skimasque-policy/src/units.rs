//! Parsing the human-friendly quantities a policy is written with.
//!
//! Policies are edited by hand and reviewed in pull requests, so a duration is
//! `20m` and a cap is `100Mbps`, not a count of seconds or bits. These parsers
//! turn those spellings into machine quantities. They are conservative: an
//! unrecognised unit is an error rather than a silent zero, because a limit that
//! reads as "no limit" by accident is the worst possible failure mode.
//!
//! Conventions follow the fields they serve. Byte sizes accept both decimal
//! (`kB` = 1000) and binary (`KiB` = 1024) prefixes. Bit rates are decimal, as
//! networking always means them: `1Mbps` is 1,000,000 bits per second.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseUnitError {
    #[error("{0:?} is empty")]
    Empty(String),
    #[error("{value:?} is not a {kind}: {reason}")]
    Malformed {
        kind: &'static str,
        value: String,
        reason: &'static str,
    },
    #[error("{0:?} overflows")]
    Overflow(String),
}

fn malformed(kind: &'static str, value: &str, reason: &'static str) -> ParseUnitError {
    ParseUnitError::Malformed {
        kind,
        value: value.to_owned(),
        reason,
    }
}

/// Parse a duration such as `500ms`, `90s`, `20m`, `1h30m` or `7d`.
///
/// Segments are concatenated left to right and summed, so `1h30m` is ninety
/// minutes. A bare number is rejected: the unit is what makes the intent
/// unambiguous in review.
pub fn parse_duration(input: &str) -> Result<Duration, ParseUnitError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(ParseUnitError::Empty(input.to_owned()));
    }

    let mut total = Duration::ZERO;
    let mut rest = text;
    let mut saw_segment = false;

    while !rest.is_empty() {
        let digits_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| malformed("duration", input, "a number with no unit"))?;
        if digits_end == 0 {
            return Err(malformed("duration", input, "expected a number"));
        }
        let value: u64 = rest[..digits_end]
            .parse()
            .map_err(|_| ParseUnitError::Overflow(input.to_owned()))?;
        rest = &rest[digits_end..];

        // The unit is the run of letters that follows.
        let unit_end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
        let unit = &rest[..unit_end];
        rest = &rest[unit_end..];

        let segment = match unit {
            "ms" => Duration::from_millis(value),
            "s" => Duration::from_secs(value),
            "m" => Duration::from_secs(value.checked_mul(60).ok_or_else(overflow(input))?),
            "h" => Duration::from_secs(value.checked_mul(3_600).ok_or_else(overflow(input))?),
            "d" => Duration::from_secs(value.checked_mul(86_400).ok_or_else(overflow(input))?),
            "" => return Err(malformed("duration", input, "a number with no unit")),
            _ => return Err(malformed("duration", input, "unknown unit")),
        };
        total = total.checked_add(segment).ok_or_else(overflow(input))?;
        saw_segment = true;
    }

    if !saw_segment {
        return Err(malformed("duration", input, "expected a number and a unit"));
    }
    Ok(total)
}

/// Parse a byte size such as `1024`, `512KiB`, `10MB` or `2.5GB`.
///
/// Decimal prefixes (`kB`, `MB`, `GB`, `TB`, `PB`) are powers of 1000; binary
/// prefixes (`KiB`, `MiB`, `GiB`, `TiB`, `PiB`) are powers of 1024. A bare
/// number is bytes. The prefix letter is case-insensitive so `10mb` and `10MB`
/// agree.
pub fn parse_bytes(input: &str) -> Result<u64, ParseUnitError> {
    let (value, unit) = split_scalar(input, "byte size")?;
    let multiplier: f64 = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "pb" => 1e15,
        "kib" => 1024.0,
        "mib" => 1024f64.powi(2),
        "gib" => 1024f64.powi(3),
        "tib" => 1024f64.powi(4),
        "pib" => 1024f64.powi(5),
        _ => return Err(malformed("byte size", input, "unknown unit")),
    };
    let bytes = value * multiplier;
    if !bytes.is_finite() || bytes < 0.0 || bytes >= (u64::MAX as f64) {
        return Err(ParseUnitError::Overflow(input.to_owned()));
    }
    Ok(bytes.round() as u64)
}

/// Parse a bit rate such as `1000000`, `100Mbps` or `1Gbps` into bits per
/// second.
///
/// Rates are decimal by networking convention: `1Mbps` is exactly 1,000,000
/// bits per second, never 1,048,576. A bare number is already bits per second.
pub fn parse_bitrate(input: &str) -> Result<u64, ParseUnitError> {
    let (value, unit) = split_scalar(input, "bit rate")?;
    let multiplier: f64 = match unit.to_ascii_lowercase().as_str() {
        "" | "bps" => 1.0,
        "kbps" => 1e3,
        "mbps" => 1e6,
        "gbps" => 1e9,
        "tbps" => 1e12,
        _ => return Err(malformed("bit rate", input, "unknown unit; expected e.g. Mbps")),
    };
    let bits = value * multiplier;
    if !bits.is_finite() || bits < 0.0 || bits >= (u64::MAX as f64) {
        return Err(ParseUnitError::Overflow(input.to_owned()));
    }
    Ok(bits.round() as u64)
}

/// A count permitted per unit of time, such as `10/s` or `600/m`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    /// How many events are permitted in each window.
    pub count: u64,
    /// The length of the window.
    pub per: Duration,
}

impl Rate {
    /// The rate expressed as events per second.
    pub fn per_second(&self) -> f64 {
        let window = self.per.as_secs_f64();
        if window == 0.0 {
            f64::INFINITY
        } else {
            self.count as f64 / window
        }
    }
}

/// Parse a rate such as `10/s`, `100/m` or `5/h`.
pub fn parse_rate(input: &str) -> Result<Rate, ParseUnitError> {
    let text = input.trim();
    let (count, unit) = text
        .split_once('/')
        .ok_or_else(|| malformed("rate", input, "expected <count>/<unit>, e.g. 10/s"))?;
    let count: u64 = count
        .trim()
        .parse()
        .map_err(|_| malformed("rate", input, "the count is not a number"))?;
    let per = match unit.trim() {
        "s" | "sec" | "second" => Duration::from_secs(1),
        "m" | "min" | "minute" => Duration::from_secs(60),
        "h" | "hour" => Duration::from_secs(3_600),
        _ => return Err(malformed("rate", input, "the unit must be s, m or h")),
    };
    Ok(Rate { count, per })
}

/// Split `"2.5GB"` into `(2.5, "GB")`: a leading decimal number and a trailing
/// unit of ASCII letters.
fn split_scalar<'a>(input: &'a str, kind: &'static str) -> Result<(f64, &'a str), ParseUnitError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(ParseUnitError::Empty(input.to_owned()));
    }
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number = number.trim();
    if number.is_empty() {
        return Err(malformed(kind, input, "expected a number"));
    }
    let value: f64 = number
        .parse()
        .map_err(|_| malformed(kind, input, "not a number"))?;
    if value < 0.0 {
        return Err(malformed(kind, input, "must not be negative"));
    }
    Ok((value, unit.trim()))
}

fn overflow(input: &str) -> impl Fn() -> ParseUnitError + '_ {
    move || ParseUnitError::Overflow(input.to_owned())
}

/// Render a duration back the way a policy would write it: `20m`, `1h30m`,
/// `500ms`. The inverse of [`parse_duration`] for the values it produces.
pub fn humanize_duration(duration: std::time::Duration) -> String {
    let mut ms = duration.as_millis();
    if ms == 0 {
        return "0s".to_owned();
    }
    let mut out = String::new();
    for (unit, size) in [("d", 86_400_000u128), ("h", 3_600_000), ("m", 60_000), ("s", 1_000)] {
        if ms >= size {
            let whole = ms / size;
            ms %= size;
            out.push_str(&format!("{whole}{unit}"));
        }
    }
    if ms > 0 {
        out.push_str(&format!("{ms}ms"));
    }
    out
}

/// Render a decimal quantity with the largest unit that leaves a whole number:
/// `100_000_000` with `"bps"` becomes `100Mbps`.
pub fn humanize_decimal(value: u64, unit: &str) -> String {
    for (prefix, size) in [("T", 1_000_000_000_000u64), ("G", 1_000_000_000), ("M", 1_000_000), ("k", 1_000)] {
        if value >= size && value.is_multiple_of(size) {
            return format!("{}{prefix}{unit}", value / size);
        }
    }
    format!("{value}{unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_sum_their_segments() {
        assert_eq!(parse_duration("20m").unwrap(), Duration::from_secs(1_200));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5_400));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7_200));
        assert_eq!(parse_duration(" 7d ").unwrap(), Duration::from_secs(604_800));
    }

    #[test]
    fn a_duration_without_a_unit_is_an_error() {
        assert!(parse_duration("20").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("m").is_err());
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn byte_sizes_distinguish_decimal_and_binary_prefixes() {
        assert_eq!(parse_bytes("1024").unwrap(), 1024);
        assert_eq!(parse_bytes("10MB").unwrap(), 10_000_000);
        assert_eq!(parse_bytes("512KiB").unwrap(), 524_288);
        assert_eq!(parse_bytes("2.5GB").unwrap(), 2_500_000_000);
        assert_eq!(parse_bytes("10mb").unwrap(), 10_000_000);
    }

    #[test]
    fn bit_rates_are_decimal_and_land_in_bits_per_second() {
        assert_eq!(parse_bitrate("100Mbps").unwrap(), 100_000_000);
        assert_eq!(parse_bitrate("1Gbps").unwrap(), 1_000_000_000);
        assert_eq!(parse_bitrate("1000000").unwrap(), 1_000_000);
        assert!(parse_bitrate("100MB").is_err());
    }

    #[test]
    fn rates_convert_to_events_per_second() {
        assert_eq!(parse_rate("10/s").unwrap().per_second(), 10.0);
        assert_eq!(parse_rate("600/m").unwrap().per_second(), 10.0);
        assert_eq!(parse_rate("3600/h").unwrap().per_second(), 1.0);
        assert!(parse_rate("10").is_err());
        assert!(parse_rate("10/decade").is_err());
    }
}
