//! Temporal values (TS-21, FR-TYPE-001, FR-TYPE-006, FR-TYPE-007): zones,
//! the system clock and lexical forms.
//!
//! - **Zones** come from the operating system's IANA database through
//!   `jiff` (`/usr/share/zoneinfo`, or `TZDIR`; feature `tzdb-zoneinfo`,
//!   no bundled data): [`zone`]. New sessions use UTC (TS-21; ID048).
//! - **Precision** is nanoseconds and there are no leap seconds, as in
//!   `jiff` (IL024, IA026; user decision 2026-10-07).
//! - **The current time** is read from the system clock: [`now`]
//!   (`jiff::Timestamp::now`, which reads `SystemTime`).
//! - **Zone names in the order.** Zoned date-times with the same instant and
//!   offset are ordered by normalised zone name ([`normalize_zone`]).
//!   TS-21 names Neo4j's `TimeZones.map`, which maps the old names of
//!   renamed zones to their new names from a data file shipped with Neo4j
//!   (R-NEO `storable/TimeZones.java`, resource `TZIDS`). That file is GPL
//!   and is not copied: the same normalisation comes from the IANA link
//!   lines, the database's own record of renamed and merged zones, fixed in
//!   the crate for the format version ([`super::zone_links`]). It is not
//!   read from the host: Debian's `tzdata.zi` (2026c) is built with
//!   `backzone`, where `Europe/Oslo` is a zone, while IANA's default build
//!   links it to `Europe/Berlin`, so a host table would make index keys
//!   depend on the host. `jiff` offers no link resolution: `iana_name`
//!   only canonicalises case (`jiff` 0.2.38 `src/tz/timezone.rs:652-666`).
//! - **Stored zones.** A stored named zone keeps its name and its offset
//!   ([`ZonedDateTime::stored`]): the value reads back the same on a host
//!   that lacks the zone or whose rules give another offset.
//! - **Lexical forms** (TS-21): ISO 8601 text; a named zone in brackets
//!   after the offset (RFC 9557, `jiff`'s format); durations as ISO 8601
//!   durations (`P1Y2M`, `P3DT4H5M6.5S`), negative ones with a leading
//!   minus sign (ISO 8601-2).

use std::fmt::Write as _;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, Zoned};

use super::{DayToSecond, Value, ZonedDateTime, ZonedTime};

/// The zone named `name` from the operating system's IANA database
/// (FR-TYPE-007), or `None` if it has no such zone.
pub(crate) fn zone(name: &str) -> Option<TimeZone> {
    TimeZone::get(name).ok()
}

/// The current instant from the system clock (FR-TYPE-006).
pub(crate) fn now() -> Timestamp {
    Timestamp::now()
}

/// The canonical name of zone `name`: the target of its IANA link in the
/// format's fixed table ([`super::zone_links::LINKS`]), or `name` itself.
pub(crate) fn normalize_zone(name: &str) -> &str {
    let links = super::zone_links::LINKS;
    match links.binary_search_by(|(alias, _)| alias.as_bytes().cmp(name.as_bytes())) {
        Ok(i) => links[i].1,
        Err(_) => name,
    }
}

/// The lexical form of a temporal value (TS-21), `None` for any other.
pub(crate) fn format(v: &Value) -> Option<String> {
    Some(match v {
        Value::Date(d) => d.to_string(),
        Value::LocalTime(t) => t.to_string(),
        Value::LocalDateTime(dt) => dt.to_string(),
        Value::ZonedTime(z) => format!("{}{}", z.time, offset_text(z.offset)),
        Value::ZonedDateTime(z) => match z.zone_name() {
            Some(name) if z.kept.is_some() => format!(
                "{}{}[{name}]",
                z.zoned.datetime(),
                offset_text(z.zoned.offset().seconds())
            ),
            Some(_) => z.zoned.to_string(),
            None => format!(
                "{}{}",
                z.zoned.datetime(),
                offset_text(z.zoned.offset().seconds())
            ),
        },
        Value::YearToMonth(m) => {
            let sign = if *m < 0 { "-" } else { "" };
            let m = m.unsigned_abs();
            format!("{sign}P{}Y{}M", m / 12, m % 12)
        }
        Value::DayToSecond(d) => {
            let total = i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos);
            let sign = if total < 0 { "-" } else { "" };
            let total = total.unsigned_abs();
            let (secs, nanos) = (total / 1_000_000_000, total % 1_000_000_000);
            let (days, rest) = (secs / 86_400, secs % 86_400);
            let (h, m, s) = (rest / 3600, rest % 3600 / 60, rest % 60);
            let mut out = format!("{sign}P{days}DT{h}H{m}M{s}");
            if nanos != 0 {
                let frac = format!("{nanos:09}");
                let _ = write!(out, ".{}", frac.trim_end_matches('0'));
            }
            out.push('S');
            out
        }
        _ => return None,
    })
}

/// `+hh:mm` or `+hh:mm:ss` for an offset of `seconds`.
fn offset_text(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let s = seconds.unsigned_abs();
    let (h, m, sec) = (s / 3600, s % 3600 / 60, s % 60);
    if sec == 0 {
        format!("{sign}{h:02}:{m:02}")
    } else {
        format!("{sign}{h:02}:{m:02}:{sec:02}")
    }
}

/// Splits `text` into a local part and a trailing offset `±hh:mm[:ss]`.
/// As jiff parses a numeric offset, each field is exactly two digits, the
/// hours at most 25 and the minutes and seconds at most 59 (jiff 0.2.38
/// `src/fmt/offset.rs:591-623`; jiff-core 0.1.1 `src/bounds.rs:277-281`),
/// so the offset is within ±93 599 s, the range the record decoder
/// accepts. `None` otherwise.
fn split_offset(text: &str) -> Option<(&str, Offset)> {
    let i = text.rfind(['+', '-'])?;
    let (local, off) = text.split_at(i);
    let sign = if off.starts_with('-') { -1 } else { 1 };
    let parts: Vec<i32> = off[1..]
        .split(':')
        .map(|p| {
            let b = p.as_bytes();
            (b.len() == 2 && b.iter().all(u8::is_ascii_digit))
                .then(|| i32::from(b[0] - b'0') * 10 + i32::from(b[1] - b'0'))
        })
        .collect::<Option<_>>()?;
    let (h, m, s) = match parts.as_slice() {
        [h, m] => (*h, *m, 0),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if h > 25 || m > 59 || s > 59 {
        return None;
    }
    Some((
        local,
        Offset::from_seconds(sign * (h * 3600 + m * 60 + s)).ok()?,
    ))
}

/// Parses the lexical form of a value of the same kind as `like` (TS-21).
/// `None` if `text` is not one.
pub(crate) fn parse(like: &Value, text: &str) -> Option<Value> {
    Some(match like {
        Value::Date(_) => Value::Date(text.parse::<Date>().ok()?),
        Value::LocalTime(_) => Value::LocalTime(text.parse::<Time>().ok()?),
        Value::LocalDateTime(_) => Value::LocalDateTime(text.parse::<DateTime>().ok()?),
        Value::ZonedTime(_) => {
            let (local, offset) = split_offset(text)?;
            Value::ZonedTime(ZonedTime {
                time: local.parse().ok()?,
                offset: offset.seconds(),
            })
        }
        Value::ZonedDateTime(_) => {
            if text.ends_with(']') {
                let z = text.parse::<Zoned>().ok()?;
                z.time_zone().iana_name()?;
                Value::ZonedDateTime(Box::new(ZonedDateTime::new(z)))
            } else {
                let (local, offset) = split_offset(text)?;
                let dt: DateTime = local.parse().ok()?;
                let ts = dt.to_zoned(TimeZone::fixed(offset)).ok()?.timestamp();
                Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(ts, offset)))
            }
        }
        Value::YearToMonth(_) => {
            let (neg, body) = text.strip_prefix('-').map_or((false, text), |b| (true, b));
            let body = body.strip_prefix('P')?.strip_suffix('M')?;
            let (y, m) = body.split_once('Y')?;
            let months = y
                .parse::<i64>()
                .ok()?
                .checked_mul(12)?
                .checked_add(m.parse().ok()?)?;
            Value::YearToMonth(if neg { months.checked_neg()? } else { months })
        }
        Value::DayToSecond(_) => {
            let (neg, body) = text.strip_prefix('-').map_or((false, text), |b| (true, b));
            let body = body.strip_prefix('P')?.strip_suffix('S')?;
            let (d, rest) = body.split_once("DT")?;
            let (h, rest) = rest.split_once('H')?;
            let (m, s) = rest.split_once('M')?;
            let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
            if frac.len() > 9 {
                return None;
            }
            let nanos: i64 = if frac.is_empty() {
                0
            } else {
                format!("{frac:0<9}").parse().ok()?
            };
            let secs = d
                .parse::<i64>()
                .ok()?
                .checked_mul(86_400)?
                .checked_add(h.parse::<i64>().ok()?.checked_mul(3600)?)?
                .checked_add(m.parse::<i64>().ok()?.checked_mul(60)?)?
                .checked_add(whole.parse().ok()?)?;
            let d = if neg {
                DayToSecond::new(secs.checked_neg()?, -nanos)?
            } else {
                DayToSecond::new(secs, nanos)?
            };
            Value::DayToSecond(d)
        }
        _ => return None,
    })
}
