//! Values and types (TS-21).
//!
//! - [`Value`]: the 24-byte runtime value. Inline: null, boolean, `i64`,
//!   `f64`, node and edge identifiers, date, local time, local date-time,
//!   zoned time and both duration types. Reference-counted slices: strings,
//!   byte strings, lists, records, paths. Boxed: zoned date-times and
//!   points.
//! - Numbers are INT64 and FLOAT64 only (BR-GQL-002, user decision
//!   2026-10-07): the other numeric types of features GV11, GV13-GV17,
//!   GV20, GV25 and GV26 are not supported; the record encoding reserves a
//!   type tag for each ([`record`]).
//! - [`total_cmp`]: the approved total order of `ORDER BY` and index keys
//!   (TS-21 Implementation-defined choices); [`key`] encodes values as byte
//!   strings in that order; [`record`] encodes the labels and properties of
//!   node and edge records.
//! - [`temporal`]: zones from the operating system through `jiff`, the
//!   system clock, lexical forms; [`spatial`]: points, distance and Hilbert
//!   keys.

pub(crate) mod key;
pub(crate) mod lexical;
pub(crate) mod record;
pub(crate) mod spatial;
pub(crate) mod temporal;
pub(crate) mod zone_links;

#[cfg(test)]
mod tests;

use std::cmp::Ordering;

use jiff::Zoned;
use jiff::civil::{Date, DateTime, Time};

use crate::sync::Arc;

pub use spatial::{Point, Srid};

/// A runtime value (TS-21): what parameters, properties and result
/// columns hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// The null value.
    Null,
    /// A boolean.
    Bool(bool),
    /// A 64-bit signed integer (INT64).
    Int(i64),
    /// A 64-bit floating point number (FLOAT64).
    Float(f64),
    /// A node reference: its element identifier (TS-07).
    Node(u64),
    /// An edge reference: its element identifier.
    Edge(u64),
    /// A date (GV39).
    Date(Date),
    /// A local time (GV39).
    LocalTime(Time),
    /// A local date-time (GV39).
    LocalDateTime(DateTime),
    /// A zoned time: a local time and its offset from UTC in seconds
    /// (GV40). A time has no date, so it has no named zone.
    ZonedTime(ZonedTime),
    /// A zoned date-time with a named zone or an offset (GV40).
    ZonedDateTime(Box<ZonedDateTime>),
    /// A duration of years and months, `DURATION(YEAR TO MONTH)` (GV41).
    YearToMonth(i64),
    /// A duration of days to fractions of seconds, `DURATION(DAY TO
    /// SECOND)` (GV41), as seconds and nanoseconds: a day is 86 400 s.
    DayToSecond(DayToSecond),
    /// A character string.
    String(Arc<str>),
    /// A byte string (GV31).
    Bytes(Arc<[u8]>),
    /// A list (GV50).
    List(Arc<[Value]>),
    /// A record: fields with distinct names, in any order (GV45).
    Record(Arc<[(Arc<str>, Value)]>),
    /// A path: its start node, then each edge and the node after it (GV55).
    Path(Arc<[u64]>),
    /// A point (FR-TYPE-002).
    Point(Box<Point>),
}

/// A zoned time: local time and offset (GV40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZonedTime {
    /// The local time.
    pub(crate) time: Time,
    /// The offset from UTC in seconds.
    pub(crate) offset: i32,
}

impl ZonedTime {
    /// The local time.
    #[must_use]
    pub fn time(&self) -> Time {
        self.time
    }

    /// The offset from UTC, in seconds.
    #[must_use]
    pub fn offset_seconds(&self) -> i32 {
        self.offset
    }

    /// Nanoseconds of the day in UTC: the local nanoseconds of the day less
    /// the offset. Neo4j orders zoned times by it (R-NEO
    /// `storable/TimeValue.java:277-285`, studied only).
    pub(crate) fn nanos_of_day_utc(self) -> i64 {
        nanos_of_day(self.time) - i64::from(self.offset) * 1_000_000_000
    }
}

/// A zoned date-time (GV40): an instant with a named zone or an offset.
#[derive(Debug, Clone, PartialEq)]
pub struct ZonedDateTime {
    /// The instant and its zone.
    pub(crate) zoned: Zoned,
    /// Whether the zone is named. `jiff` turns a zero offset into its UTC
    /// zone, which has a name (`jiff` 0.2.38 `src/tz/timezone.rs:457-464,
    /// 666-678`), so an offset-only value needs this flag to stay one.
    pub(crate) named: bool,
    /// A stored zone name that this host's zone database cannot apply: it
    /// lacks the zone, or the zone's rules give another offset at this
    /// instant than the stored one. `zoned` then holds the stored offset
    /// as a fixed zone, so the value keeps its name and offset (TS-21).
    pub(crate) kept: Option<Box<str>>,
}

impl ZonedDateTime {
    /// A zoned date-time in the named zone of `zoned`, or with only its
    /// offset if the zone has no name.
    pub(crate) fn new(zoned: Zoned) -> ZonedDateTime {
        let named = zoned.time_zone().iana_name().is_some();
        ZonedDateTime {
            zoned,
            named,
            kept: None,
        }
    }

    /// The stored zoned date-time at `ts` in the zone named `name` with
    /// offset `offset` (TS-21 record format). The host's zone is used when
    /// it has that name and gives that offset at `ts`; otherwise the value
    /// keeps `name` and `offset` without the zone's rules.
    pub(crate) fn stored(
        ts: jiff::Timestamp,
        name: &str,
        offset: jiff::tz::Offset,
    ) -> ZonedDateTime {
        if let Some(tz) = temporal::zone(name) {
            let z = ts.to_zoned(tz);
            if z.offset() == offset && z.time_zone().iana_name() == Some(name) {
                return ZonedDateTime::new(z);
            }
        }
        ZonedDateTime {
            zoned: ts.to_zoned(jiff::tz::TimeZone::fixed(offset)),
            named: true,
            kept: Some(name.into()),
        }
    }

    /// A zoned date-time at `ts` with only the offset `offset` (seconds).
    pub(crate) fn with_offset(ts: jiff::Timestamp, offset: jiff::tz::Offset) -> ZonedDateTime {
        ZonedDateTime {
            zoned: ts.to_zoned(jiff::tz::TimeZone::fixed(offset)),
            named: false,
            kept: None,
        }
    }

    /// The instant and its zone (a fixed offset for an offset-only value).
    #[must_use]
    pub fn zoned(&self) -> &Zoned {
        &self.zoned
    }

    /// The zone name, `None` for an offset-only value.
    #[must_use]
    pub fn zone_name(&self) -> Option<&str> {
        if let Some(k) = &self.kept {
            Some(k)
        } else if self.named {
            self.zoned.time_zone().iana_name()
        } else {
            None
        }
    }
}

/// A `DURATION(DAY TO SECOND)`: whole seconds and nanoseconds `0..1e9`,
/// the nanoseconds of the same sign as the total or zero after
/// normalisation by [`DayToSecond::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayToSecond {
    /// Whole seconds.
    pub(crate) seconds: i64,
    /// Nanoseconds, `0..1_000_000_000`.
    pub(crate) nanos: u32,
}

impl DayToSecond {
    /// Whole seconds.
    #[must_use]
    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    /// Nanoseconds, `0..1_000_000_000`.
    #[must_use]
    pub fn nanos(&self) -> u32 {
        self.nanos
    }

    /// The duration of `seconds` and `nanos` nanoseconds, normalised so
    /// that the nanoseconds are in `0..1e9` (Neo4j `DurationValue` keeps
    /// them so). `None` if the normalised seconds overflow `i64`.
    pub(crate) fn new(seconds: i64, nanos: i64) -> Option<DayToSecond> {
        let total = i128::from(seconds) * 1_000_000_000 + i128::from(nanos);
        Some(DayToSecond {
            seconds: i64::try_from(total.div_euclid(1_000_000_000)).ok()?,
            nanos: total.rem_euclid(1_000_000_000) as u32,
        })
    }
}

/// Nanoseconds since midnight of `t`.
pub(crate) fn nanos_of_day(t: Time) -> i64 {
    ((i64::from(t.hour()) * 60 + i64::from(t.minute())) * 60 + i64::from(t.second()))
        * 1_000_000_000
        + i64::from(t.subsec_nanosecond())
}

/// The type groups of the total order, in ascending order (TS-21: Neo4j's
/// group order, with byte strings after character strings and NULL last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    Record,
    Node,
    Edge,
    List,
    Path,
    Point,
    ZonedDateTime,
    LocalDateTime,
    Date,
    ZonedTime,
    LocalTime,
    Duration,
    String,
    Bytes,
    Boolean,
    Number,
    Null,
}

impl Value {
    /// The value's group in the total order.
    pub(crate) fn group(&self) -> Group {
        match self {
            Value::Null => Group::Null,
            Value::Bool(_) => Group::Boolean,
            Value::Int(_) | Value::Float(_) => Group::Number,
            Value::Node(_) => Group::Node,
            Value::Edge(_) => Group::Edge,
            Value::Date(_) => Group::Date,
            Value::LocalTime(_) => Group::LocalTime,
            Value::LocalDateTime(_) => Group::LocalDateTime,
            Value::ZonedTime(_) => Group::ZonedTime,
            Value::ZonedDateTime(_) => Group::ZonedDateTime,
            Value::YearToMonth(_) | Value::DayToSecond(_) => Group::Duration,
            Value::String(_) => Group::String,
            Value::Bytes(_) => Group::Bytes,
            Value::List(_) => Group::List,
            Value::Record(_) => Group::Record,
            Value::Path(_) => Group::Path,
            Value::Point(_) => Group::Point,
        }
    }

    /// A string value.
    #[must_use]
    pub fn string(s: &str) -> Value {
        Value::String(Arc::from(s))
    }
}

/// The parts Neo4j compares durations by (R-NEO
/// `storable/DurationValue.java:86-93`, studied only): the average length
/// in seconds (months × 2 629 746 + days × 86 400 + seconds), then
/// nanoseconds, months, days and seconds. A `DAY TO SECOND` duration keeps
/// its days in its seconds.
pub(crate) fn duration_parts(v: &Value) -> Option<(i128, u32, i64, i64, i64)> {
    const AVG_SECONDS_PER_MONTH: i128 = 2_629_746;
    match *v {
        Value::YearToMonth(months) => {
            Some((i128::from(months) * AVG_SECONDS_PER_MONTH, 0, months, 0, 0))
        }
        Value::DayToSecond(d) => Some((i128::from(d.seconds), d.nanos, 0, 0, d.seconds)),
        _ => None,
    }
}

/// Compares two numbers by exact value: integers and floats are one group
/// (TS-21). NaN is above every number and equal to itself; −0.0 equals
/// +0.0 (user decision 2026-10-07).
fn number_cmp(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => float_cmp(*x, *y),
        (Value::Int(x), Value::Float(y)) => int_float_cmp(*x, *y),
        (Value::Float(x), Value::Int(y)) => int_float_cmp(*y, *x).reverse(),
        _ => unreachable!("numbers only"),
    }
}

/// Float order: NaN above everything and equal to NaN; −0.0 = +0.0.
pub(crate) fn float_cmp(x: f64, y: f64) -> Ordering {
    match (x.is_nan(), y.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => x.partial_cmp(&y).expect("not NaN"),
    }
}

/// Compares integer `i` with float `f` by exact value.
fn int_float_cmp(i: i64, f: f64) -> Ordering {
    if f.is_nan() {
        return Ordering::Less;
    }
    // 2^63 as a float, exactly.
    const TWO_63: f64 = 9_223_372_036_854_775_808.0;
    if f >= TWO_63 {
        return Ordering::Less;
    }
    if f < -TWO_63 {
        return Ordering::Greater;
    }
    // |f| < 2^63 (or f == -2^63): its integer part fits an i64 exactly.
    let t = f.trunc();
    let ti = t as i64;
    match i.cmp(&ti) {
        Ordering::Equal => {
            let frac = f - t;
            if frac > 0.0 {
                Ordering::Less
            } else if frac < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        o => o,
    }
}

/// The approved total order of values (TS-21 Implementation-defined
/// choices; BR-GQL-002, user decision 2026-10-07): groups in [`Group`]
/// order, NULL last, then within a group:
/// - numbers by exact value, NaN highest, −0.0 = +0.0;
/// - strings by code point (their UTF-8 bytes), no padding; byte strings
///   by unsigned byte, a proper prefix first, trailing X'00' significant;
/// - lists lexicographically, a proper prefix first; records by size, then
///   sorted field names, then the values in name order; paths by start
///   node, then edges in order, then length (Neo4j `MapValue`,
///   `VirtualPathValue`);
/// - nodes and edges by identifier; points by CRS, then dimension, then
///   coordinates in axis order;
/// - temporal values chronologically; zoned date-times by instant, then
///   offset, then offset-only before named zones, then normalised zone
///   names ([`temporal::normalize_zone`]); zoned times by UTC time of day,
///   then offset; durations as [`duration_parts`].
pub(crate) fn total_cmp(a: &Value, b: &Value) -> Ordering {
    let g = a.group().cmp(&b.group());
    if g != Ordering::Equal {
        return g;
    }
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Int(_) | Value::Float(_), _) => number_cmp(a, b),
        (Value::Node(x), Value::Node(y)) | (Value::Edge(x), Value::Edge(y)) => x.cmp(y),
        (Value::Date(x), Value::Date(y)) => x.cmp(y),
        (Value::LocalTime(x), Value::LocalTime(y)) => x.cmp(y),
        (Value::LocalDateTime(x), Value::LocalDateTime(y)) => x.cmp(y),
        (Value::ZonedTime(x), Value::ZonedTime(y)) => x
            .nanos_of_day_utc()
            .cmp(&y.nanos_of_day_utc())
            .then(x.offset.cmp(&y.offset)),
        (Value::ZonedDateTime(x), Value::ZonedDateTime(y)) => zoned_cmp(x, y),
        (Value::YearToMonth(_) | Value::DayToSecond(_), _) => {
            duration_parts(a).cmp(&duration_parts(b))
        }
        (Value::String(x), Value::String(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Bytes(x), Value::Bytes(y)) => x.cmp(y),
        (Value::List(x), Value::List(y)) => {
            for (p, q) in x.iter().zip(y.iter()) {
                let o = total_cmp(p, q);
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Record(x), Value::Record(y)) => {
            let o = x.len().cmp(&y.len());
            if o != Ordering::Equal {
                return o;
            }
            let (sx, sy) = (sorted_fields(x), sorted_fields(y));
            for (p, q) in sx.iter().zip(&sy) {
                let o = p.0.as_bytes().cmp(q.0.as_bytes());
                if o != Ordering::Equal {
                    return o;
                }
            }
            for (p, q) in sx.iter().zip(&sy) {
                let o = total_cmp(&p.1, &q.1);
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        }
        (Value::Path(x), Value::Path(y)) => {
            let o = x.first().cmp(&y.first());
            if o != Ordering::Equal {
                return o;
            }
            // Edges are at the odd positions.
            for (p, q) in x.iter().skip(1).step_by(2).zip(y.iter().skip(1).step_by(2)) {
                let o = p.cmp(q);
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Point(x), Value::Point(y)) => x.total_cmp(y),
        _ => unreachable!("same group"),
    }
}

/// The fields of a record sorted by name.
pub(crate) fn sorted_fields(r: &[(Arc<str>, Value)]) -> Vec<&(Arc<str>, Value)> {
    let mut v: Vec<_> = r.iter().collect();
    v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    v
}

/// The zoned date-time order (R-NEO `storable/DateTimeValue.java:477-513`,
/// studied only): instant, then offset, then offset-only zones before
/// named ones, then normalised zone names. The chronology is ISO for every
/// value.
fn zoned_cmp(x: &ZonedDateTime, y: &ZonedDateTime) -> Ordering {
    let (tx, ty) = (x.zoned.timestamp(), y.zoned.timestamp());
    tx.as_second()
        .cmp(&ty.as_second())
        .then(tx.subsec_nanosecond().cmp(&ty.subsec_nanosecond()))
        .then(x.zoned.offset().seconds().cmp(&y.zoned.offset().seconds()))
        .then_with(|| match (x.zone_name(), y.zone_name()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(a), Some(b)) => temporal::normalize_zone(a)
                .as_bytes()
                .cmp(temporal::normalize_zone(b).as_bytes()),
        })
}
