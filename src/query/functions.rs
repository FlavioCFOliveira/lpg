//! The function table and its kernels (rmp #42): the functions of the
//! approved feature set (character strings of the mandatory surface,
//! `ABS` of a duration, and the temporal functions of GV39, GV40 and
//! GV41), and the temporal arithmetic of [`super::expr::arith`].
//!
//! One row per function ([`TABLE`]): its name, its arity, whether it is
//! deterministic (a deterministic call on literals is folded by the
//! binder, [`super::expr::Expr::fold`]) and its kernel. A NULL argument
//! gives NULL, except for the functions that take none.
//!
//! The table layout (a `Signature` row per function with a determinism
//! flag and a kernel, folding of deterministic calls on literals) is
//! ported from zu @ `a821319` (<https://github.com/tamnd/zu>, Apache-2.0,
//! see `LICENSE-APACHE`), `crates/zu-query/src/functions.rs` (`Signature`,
//! `fold`, `string_kernel`, `trim_kernel`, `cut_kernel`,
//! `temporal_kernel`, `temporal_record`, `duration_between_kernel`).
//! Adapted: kernels take lpg's [`Value`] and compute through `jiff`
//! (TS-21); only the functions of the approved feature set are present;
//! errors are lpg's GQLSTATUS conditions.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{Offset, TimeZone};
use jiff::{SignedDuration, Span, Timestamp, Unit};

use super::ast::NormalForm;
use super::expr::{ArithOp, invalid_type, negate_d2s, type_name};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::{DayToSecond, Value, ZonedDateTime, ZonedTime, nanos_of_day, temporal};

/// A function of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Func {
    CharLength,
    Upper,
    Lower,
    Left,
    Right,
    Trim,
    Normalize(NormalForm),
    Abs,
    CurrentDate,
    CurrentTime,
    CurrentTimestamp,
    CurrentLocalTime,
    CurrentLocalTimestamp,
    Date,
    ZonedTime,
    LocalTime,
    ZonedDatetime,
    LocalDatetime,
    Duration,
    DurationBetween,
    DurationBetweenMonths,
}

/// One row of the function table.
pub(crate) struct FnRow {
    /// The function.
    pub(crate) func: Func,
    /// Its GQL name.
    pub(crate) name: &'static str,
    /// Its number of arguments.
    pub(crate) arity: usize,
    /// Whether equal arguments always give equal results.
    pub(crate) deterministic: bool,
    /// The kernel: arguments and the statement clock to the result.
    pub(crate) kernel: fn(Func, &[Value], Timestamp) -> Result<Value, Error>,
}

macro_rules! rows {
    ($( $f:expr, $name:literal, $arity:literal, $det:literal, $k:ident; )*) => {
        /// The function table.
        pub(crate) static TABLE: &[FnRow] = &[
            $( FnRow { func: $f, name: $name, arity: $arity, deterministic: $det, kernel: $k }, )*
        ];
    };
}

rows! {
    Func::CharLength, "CHAR_LENGTH", 1, true, string_kernel;
    Func::Upper, "UPPER", 1, true, string_kernel;
    Func::Lower, "LOWER", 1, true, string_kernel;
    Func::Left, "LEFT", 2, true, cut_kernel;
    Func::Right, "RIGHT", 2, true, cut_kernel;
    Func::Trim, "TRIM", 1, true, string_kernel;
    Func::Normalize(NormalForm::Nfc), "NORMALIZE(NFC)", 1, true, string_kernel;
    Func::Normalize(NormalForm::Nfd), "NORMALIZE(NFD)", 1, true, string_kernel;
    Func::Normalize(NormalForm::Nfkc), "NORMALIZE(NFKC)", 1, true, string_kernel;
    Func::Normalize(NormalForm::Nfkd), "NORMALIZE(NFKD)", 1, true, string_kernel;
    Func::Abs, "ABS", 1, true, abs_kernel;
    Func::CurrentDate, "CURRENT_DATE", 0, false, now_kernel;
    Func::CurrentTime, "CURRENT_TIME", 0, false, now_kernel;
    Func::CurrentTimestamp, "CURRENT_TIMESTAMP", 0, false, now_kernel;
    Func::CurrentLocalTime, "LOCAL_TIME", 0, false, now_kernel;
    Func::CurrentLocalTimestamp, "LOCAL_TIMESTAMP", 0, false, now_kernel;
    Func::Date, "DATE", 1, true, temporal_kernel;
    Func::ZonedTime, "ZONED_TIME", 1, true, temporal_kernel;
    Func::LocalTime, "LOCAL_TIME", 1, true, temporal_kernel;
    Func::ZonedDatetime, "ZONED_DATETIME", 1, true, temporal_kernel;
    Func::LocalDatetime, "LOCAL_DATETIME", 1, true, temporal_kernel;
    Func::Duration, "DURATION", 1, true, temporal_kernel;
    Func::DurationBetween, "DURATION_BETWEEN", 2, true, between_kernel;
    Func::DurationBetweenMonths, "DURATION_BETWEEN(YEAR TO MONTH)", 2, true, between_kernel;
}

/// The row of `f`.
pub(crate) fn row(f: Func) -> &'static FnRow {
    TABLE
        .iter()
        .find(|r| r.func == f)
        .expect("every function has a row")
}

/// Calls `f` on `args` at the statement clock `now`.
///
/// # Errors
///
/// The data exceptions of the kernel.
pub(crate) fn call(f: Func, args: &[Value], now: Timestamp) -> Result<Value, Error> {
    let r = row(f);
    if r.arity > 0 && args.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    (r.kernel)(f, args, now)
}

fn string_of(f: Func, v: &Value) -> Result<&str, Error> {
    match v {
        Value::String(s) => Ok(s),
        other => Err(invalid_type(&format!(
            "{} of {}",
            row(f).name,
            type_name(other)
        ))),
    }
}

fn string_kernel(f: Func, args: &[Value], _: Timestamp) -> Result<Value, Error> {
    let s = string_of(f, &args[0])?;
    Ok(match f {
        Func::CharLength => Value::Int(s.chars().count() as i64),
        Func::Upper => Value::string(&s.to_uppercase()),
        Func::Lower => Value::string(&s.to_lowercase()),
        // IV023, "Só U+0020": only U+0020 is trimmed.
        Func::Trim => Value::string(s.trim_matches(' ')),
        Func::Normalize(form) => Value::string(&super::unicode::normalize(s, form)),
        _ => {
            return Err(super::expr::internal(
                "a string kernel for another function",
            ));
        }
    })
}

/// `LEFT(s, n)`, `RIGHT(s, n)`: the first or last `n` characters; a
/// negative `n` raises 22011 "substring error".
fn cut_kernel(f: Func, args: &[Value], _: Timestamp) -> Result<Value, Error> {
    let s = string_of(f, &args[0])?;
    let n = match args[1] {
        Value::Int(n) => n,
        ref other => {
            return Err(invalid_type(&format!(
                "the length of {} is {}",
                row(f).name,
                type_name(other)
            )));
        }
    };
    let Ok(n) = usize::try_from(n) else {
        return Err(Error::new(
            codes::C22011,
            "substring error: a negative length",
        ));
    };
    let len = s.chars().count();
    let n = n.min(len);
    let out: String = if f == Func::Left {
        s.chars().take(n).collect()
    } else {
        s.chars().skip(len - n).collect()
    };
    Ok(Value::string(&out))
}

/// `ABS` of a duration (GV41). `ABS` of a number is feature GF01, which is
/// not supported.
fn abs_kernel(_: Func, args: &[Value], _: Timestamp) -> Result<Value, Error> {
    match &args[0] {
        Value::YearToMonth(m) => m
            .checked_abs()
            .map(Value::YearToMonth)
            .ok_or_else(interval_overflow),
        Value::DayToSecond(d) => {
            if d.seconds < 0 {
                negate_d2s(*d).map(Value::DayToSecond)
            } else {
                Ok(Value::DayToSecond(*d))
            }
        }
        Value::Int(_) | Value::Float(_) => Err(super::features::unsupported("GF01")),
        other => Err(invalid_type(&format!("ABS of {}", type_name(other)))),
    }
}

fn interval_overflow() -> Error {
    Error::new(codes::C22015, "interval field overflow")
}

fn datetime_overflow() -> Error {
    Error::new(codes::C22008, "datetime field overflow")
}

/// The session's time zone displacement: UTC (ID048).
fn session_zone() -> TimeZone {
    TimeZone::fixed(Offset::UTC)
}

/// `CURRENT_DATE`, `CURRENT_TIME`, `CURRENT_TIMESTAMP`, `LOCAL_TIME`,
/// `LOCAL_TIMESTAMP`: the statement clock in the session's zone, UTC.
fn now_kernel(f: Func, _: &[Value], now: Timestamp) -> Result<Value, Error> {
    let z = now.to_zoned(session_zone());
    Ok(match f {
        Func::CurrentDate => Value::Date(z.date()),
        Func::CurrentTime => Value::ZonedTime(ZonedTime {
            time: z.time(),
            offset: 0,
        }),
        Func::CurrentTimestamp => {
            Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(now, Offset::UTC)))
        }
        Func::CurrentLocalTime => Value::LocalTime(z.time()),
        Func::CurrentLocalTimestamp => Value::LocalDateTime(z.datetime()),
        _ => return Err(super::expr::internal("a clock kernel for another function")),
    })
}

/// A value of the kind `f` constructs, for [`temporal::parse`].
fn like(f: Func) -> Value {
    match f {
        Func::Date => Value::Date(Date::constant(2000, 1, 1)),
        Func::LocalTime => Value::LocalTime(Time::midnight()),
        Func::ZonedTime => Value::ZonedTime(ZonedTime {
            time: Time::midnight(),
            offset: 0,
        }),
        Func::LocalDatetime => Value::LocalDateTime(DateTime::constant(2000, 1, 1, 0, 0, 0, 0)),
        _ => Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(
            Timestamp::UNIX_EPOCH,
            Offset::UTC,
        ))),
    }
}

/// `DATE`, `ZONED_TIME`, `LOCAL_TIME`, `ZONED_DATETIME`, `LOCAL_DATETIME`,
/// `DURATION` of a string (the lexical forms of TS-21) or of a record of
/// fields.
fn temporal_kernel(f: Func, args: &[Value], _: Timestamp) -> Result<Value, Error> {
    match &args[0] {
        Value::String(s) => {
            if f == Func::Duration {
                return parse_duration(s).ok_or_else(|| {
                    Error::new(codes::C22G0H, format!("invalid duration format: '{s}'"))
                });
            }
            temporal::parse(&like(f), s).ok_or_else(|| {
                Error::new(
                    codes::C22007,
                    format!("invalid date, time, or datetime format: '{s}'"),
                )
            })
        }
        Value::Record(fields) => {
            let fields: Vec<(String, Value)> = fields
                .iter()
                .map(|(n, v)| (n.to_ascii_lowercase(), v.clone()))
                .collect();
            if f == Func::Duration {
                duration_record(&fields)
            } else {
                temporal_record(f, &fields)
            }
        }
        other => Err(invalid_type(&format!(
            "{} of {}",
            row(f).name,
            type_name(other)
        ))),
    }
}

/// A duration from its ISO 8601 text (`P1Y2M`, `PT36H`, `-P3DT4H5M6.5S`),
/// parsed by `jiff` (TS-21): years and months give a `DURATION(YEAR TO
/// MONTH)`, weeks, days and time a `DURATION(DAY TO SECOND)`; a text with
/// both kinds, or none, is `None`.
pub(crate) fn parse_duration(s: &str) -> Option<Value> {
    if let Some(v) = temporal::parse(&Value::YearToMonth(0), s).or_else(|| {
        temporal::parse(
            &Value::DayToSecond(DayToSecond {
                seconds: 0,
                nanos: 0,
            }),
            s,
        )
    }) {
        return Some(v);
    }
    let span: Span = s.parse().ok()?;
    let months = i64::from(span.get_years()) * 12 + i64::from(span.get_months());
    let nanos = i128::from(span.get_weeks()) * 7 * 86_400_000_000_000
        + i128::from(span.get_days()) * 86_400_000_000_000
        + i128::from(span.get_hours()) * 3_600_000_000_000
        + i128::from(span.get_minutes()) * 60_000_000_000
        + i128::from(span.get_seconds()) * 1_000_000_000
        + i128::from(span.get_milliseconds()) * 1_000_000
        + i128::from(span.get_microseconds()) * 1_000
        + i128::from(span.get_nanoseconds());
    match (months != 0, nanos != 0) {
        (true, true) => None,
        (true, false) => Some(Value::YearToMonth(months)),
        (false, _) => d2s_of_nanos(nanos).ok().map(Value::DayToSecond),
    }
}

fn invalid_field(name: &str, duration: bool) -> Error {
    if duration {
        Error::new(
            codes::C22G07,
            format!("invalid duration function field name: '{name}'"),
        )
    } else {
        Error::new(
            codes::C22G05,
            format!("invalid date, time, or datetime function field name: '{name}'"),
        )
    }
}

fn invalid_value(what: &str) -> Error {
    Error::new(
        codes::C22G06,
        format!("invalid datetime function value: {what}"),
    )
}

/// The integer field `name` of a record, `default` if absent.
fn int_field(fields: &[(String, Value)], name: &str, default: i64) -> Result<i64, Error> {
    match fields.iter().find(|(n, _)| n == name) {
        None => Ok(default),
        Some((_, Value::Int(i))) => Ok(*i),
        Some((_, v)) => Err(invalid_value(&format!(
            "the field '{name}' is {}",
            type_name(v)
        ))),
    }
}

fn narrow<T: TryFrom<i64>>(v: i64, name: &str) -> Result<T, Error> {
    T::try_from(v).map_err(|_| invalid_value(&format!("the field '{name}' is out of range")))
}

/// The zone of a `timezone` field: an IANA name or an offset `±hh:mm`.
fn zone_field(fields: &[(String, Value)]) -> Result<Option<(TimeZone, bool)>, Error> {
    match fields.iter().find(|(n, _)| n == "timezone") {
        None => Ok(None),
        Some((_, Value::String(s))) => {
            if let Some(tz) = temporal::zone(s) {
                return Ok(Some((tz, true)));
            }
            let parsed = temporal::parse(
                &Value::ZonedTime(ZonedTime {
                    time: Time::midnight(),
                    offset: 0,
                }),
                &format!("00:00{s}"),
            );
            match parsed {
                Some(Value::ZonedTime(z)) => Ok(Some((
                    TimeZone::fixed(
                        Offset::from_seconds(z.offset)
                            .map_err(|_| invalid_value("an offset out of range"))?,
                    ),
                    false,
                ))),
                _ => Err(invalid_value(&format!("the time zone '{s}'"))),
            }
        }
        Some((_, v)) => Err(invalid_value(&format!(
            "the field 'timezone' is {}",
            type_name(v)
        ))),
    }
}

/// A temporal instant from record fields (Neo4j's component names: year,
/// month, day, hour, minute, second, nanosecond, timezone).
fn temporal_record(f: Func, fields: &[(String, Value)]) -> Result<Value, Error> {
    let date_part = matches!(f, Func::Date | Func::LocalDatetime | Func::ZonedDatetime);
    let time_part = !matches!(f, Func::Date);
    let zoned = matches!(f, Func::ZonedTime | Func::ZonedDatetime);
    for (n, _) in fields {
        let ok = (date_part && matches!(n.as_str(), "year" | "month" | "day"))
            || (time_part && matches!(n.as_str(), "hour" | "minute" | "second" | "nanosecond"))
            || (zoned && n == "timezone");
        if !ok {
            return Err(invalid_field(n, false));
        }
    }
    let date = if date_part {
        let y = int_field(fields, "year", i64::MIN)?;
        if y == i64::MIN {
            return Err(invalid_value("the field 'year' is missing"));
        }
        Date::new(
            narrow(y, "year")?,
            narrow(int_field(fields, "month", 1)?, "month")?,
            narrow(int_field(fields, "day", 1)?, "day")?,
        )
        .map_err(|e| invalid_value(&e.to_string()))?
    } else {
        Date::constant(1970, 1, 1)
    };
    let time = Time::new(
        narrow(int_field(fields, "hour", 0)?, "hour")?,
        narrow(int_field(fields, "minute", 0)?, "minute")?,
        narrow(int_field(fields, "second", 0)?, "second")?,
        narrow(int_field(fields, "nanosecond", 0)?, "nanosecond")?,
    )
    .map_err(|e| invalid_value(&e.to_string()))?;
    let zone = zone_field(fields)?;
    Ok(match f {
        Func::Date => Value::Date(date),
        Func::LocalTime => Value::LocalTime(time),
        Func::LocalDatetime => Value::LocalDateTime(date.to_datetime(time)),
        Func::ZonedTime => {
            let offset = match zone {
                None => 0,
                Some((tz, _)) => tz.to_offset(Timestamp::UNIX_EPOCH).seconds(),
            };
            Value::ZonedTime(ZonedTime { time, offset })
        }
        _ => {
            let (tz, named) = zone.unwrap_or((session_zone(), false));
            let z = date
                .to_datetime(time)
                .to_zoned(tz)
                .map_err(|e| invalid_value(&e.to_string()))?;
            if named {
                Value::ZonedDateTime(Box::new(ZonedDateTime::new(z)))
            } else {
                Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(
                    z.timestamp(),
                    z.offset(),
                )))
            }
        }
    })
}

/// A duration from record fields: years and months give a `DURATION(YEAR
/// TO MONTH)`; days, hours, minutes, seconds, milliseconds, microseconds
/// and nanoseconds a `DURATION(DAY TO SECOND)`; both kinds together raise
/// 22G07.
fn duration_record(fields: &[(String, Value)]) -> Result<Value, Error> {
    const YM: [&str; 2] = ["years", "months"];
    const DS: [(&str, i128); 7] = [
        ("days", 86_400_000_000_000),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ];
    let mut has_ym = false;
    let mut has_ds = false;
    for (n, _) in fields {
        if YM.contains(&n.as_str()) {
            has_ym = true;
        } else if DS.iter().any(|(d, _)| d == n) {
            has_ds = true;
        } else {
            return Err(invalid_field(n, true));
        }
    }
    if has_ym && has_ds {
        return Err(invalid_field(
            "years or months with a day-to-second field",
            true,
        ));
    }
    if has_ym {
        let months = i128::from(int_field(fields, "years", 0)?) * 12
            + i128::from(int_field(fields, "months", 0)?);
        return i64::try_from(months)
            .map(Value::YearToMonth)
            .map_err(|_| interval_overflow());
    }
    let mut nanos: i128 = 0;
    for (n, scale) in DS {
        nanos += i128::from(int_field(fields, n, 0)?) * scale;
    }
    d2s_of_nanos(nanos).map(Value::DayToSecond)
}

fn d2s_of_nanos(n: i128) -> Result<DayToSecond, Error> {
    let s = i64::try_from(n.div_euclid(1_000_000_000)).map_err(|_| interval_overflow())?;
    DayToSecond::new(s, n.rem_euclid(1_000_000_000) as i64).ok_or_else(interval_overflow)
}

fn d2s_nanos(d: DayToSecond) -> i128 {
    i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos)
}

fn signed(d: DayToSecond) -> SignedDuration {
    // `DayToSecond::new` keeps the nanoseconds in 0..1e9; jiff wants the
    // sign of both parts equal, which `new` normalises.
    SignedDuration::new(d.seconds, d.nanos as i32)
}

fn d2s_of(sd: SignedDuration) -> Result<DayToSecond, Error> {
    DayToSecond::new(sd.as_secs(), i64::from(sd.subsec_nanos())).ok_or_else(interval_overflow)
}

/// The instant-like position of a temporal value for `DURATION_BETWEEN`:
/// zoned values as instants, local values as civil date-times.
enum Point {
    Instant(Timestamp),
    Civil(DateTime),
    Clock(i64),
}

fn point_of(v: &Value) -> Option<Point> {
    Some(match v {
        Value::ZonedDateTime(z) => Point::Instant(z.zoned.timestamp()),
        Value::LocalDateTime(d) => Point::Civil(*d),
        Value::Date(d) => Point::Civil(d.to_datetime(Time::midnight())),
        Value::LocalTime(t) => Point::Clock(nanos_of_day(*t)),
        Value::ZonedTime(z) => Point::Clock(z.nanos_of_day_utc()),
        _ => return None,
    })
}

/// `DURATION_BETWEEN(a, b)`: the duration from `a` to `b`, `DAY TO
/// SECOND` or, with the qualifier, `YEAR TO MONTH` (whole months).
fn between_kernel(f: Func, args: &[Value], _: Timestamp) -> Result<Value, Error> {
    let (a, b) = (&args[0], &args[1]);
    let incompatible = || {
        Error::new(
            codes::C22G14,
            format!(
                "incompatible temporal instant unit groups: {} and {}",
                type_name(a),
                type_name(b)
            ),
        )
    };
    let (Some(pa), Some(pb)) = (point_of(a), point_of(b)) else {
        return Err(invalid_type(&format!(
            "DURATION_BETWEEN of {} and {}",
            type_name(a),
            type_name(b)
        )));
    };
    if f == Func::DurationBetweenMonths {
        let (ca, cb) = match (pa, pb) {
            (Point::Civil(x), Point::Civil(y)) => (x, y),
            (Point::Instant(x), Point::Instant(y)) => (
                x.to_zoned(TimeZone::UTC).datetime(),
                y.to_zoned(TimeZone::UTC).datetime(),
            ),
            _ => return Err(incompatible()),
        };
        let span = ca
            .until((Unit::Month, cb))
            .map_err(|_| interval_overflow())?;
        return Ok(Value::YearToMonth(i64::from(span.get_months())));
    }
    let sd = match (pa, pb) {
        (Point::Instant(x), Point::Instant(y)) => y.duration_since(x),
        (Point::Civil(x), Point::Civil(y)) => y.duration_since(x),
        (Point::Clock(x), Point::Clock(y)) => SignedDuration::from_nanos(y - x),
        _ => return Err(incompatible()),
    };
    d2s_of(sd).map(Value::DayToSecond)
}

/// Temporal arithmetic: an instant `±` a duration, a duration `±` a
/// duration of the same kind, a duration `*` an integer and `/` a nonzero
/// integer. A time of day wraps at 24 hours and ignores months. Other
/// operand types raise 22G03.
///
/// # Errors
///
/// As above; 22008 for an instant outside the supported range; 22015 for
/// a duration overflow; 22012 for a zero divisor.
pub(crate) fn temporal_arith(op: ArithOp, a: &Value, b: &Value) -> Result<Value, Error> {
    use Value as V;
    let bad = || {
        invalid_type(&format!(
            "{} {} {}",
            type_name(a),
            match op {
                ArithOp::Add => "+",
                ArithOp::Sub => "-",
                ArithOp::Mul => "*",
                ArithOp::Div => "/",
            },
            type_name(b)
        ))
    };
    match (op, a, b) {
        (ArithOp::Add | ArithOp::Sub, V::YearToMonth(x), V::YearToMonth(y)) => {
            let r = if op == ArithOp::Add {
                x.checked_add(*y)
            } else {
                x.checked_sub(*y)
            };
            r.map(V::YearToMonth).ok_or_else(interval_overflow)
        }
        (ArithOp::Add | ArithOp::Sub, V::DayToSecond(x), V::DayToSecond(y)) => {
            let (x, y) = (d2s_nanos(*x), d2s_nanos(*y));
            d2s_of_nanos(if op == ArithOp::Add { x + y } else { x - y }).map(V::DayToSecond)
        }
        (ArithOp::Mul, V::YearToMonth(m), V::Int(k))
        | (ArithOp::Mul, V::Int(k), V::YearToMonth(m)) => m
            .checked_mul(*k)
            .map(V::YearToMonth)
            .ok_or_else(interval_overflow),
        (ArithOp::Mul, V::DayToSecond(d), V::Int(k))
        | (ArithOp::Mul, V::Int(k), V::DayToSecond(d)) => {
            d2s_of_nanos(d2s_nanos(*d) * i128::from(*k)).map(V::DayToSecond)
        }
        (ArithOp::Div, V::YearToMonth(m), V::Int(k)) => {
            if *k == 0 {
                return Err(super::expr::division_by_zero());
            }
            m.checked_div(*k)
                .map(V::YearToMonth)
                .ok_or_else(interval_overflow)
        }
        (ArithOp::Div, V::DayToSecond(d), V::Int(k)) => {
            if *k == 0 {
                return Err(super::expr::division_by_zero());
            }
            d2s_of_nanos(d2s_nanos(*d) / i128::from(*k)).map(V::DayToSecond)
        }
        (ArithOp::Add, V::YearToMonth(_) | V::DayToSecond(_), _) if is_instant(b) => {
            shift(b, a, false)
        }
        (ArithOp::Add | ArithOp::Sub, _, V::YearToMonth(_) | V::DayToSecond(_))
            if is_instant(a) =>
        {
            shift(a, b, op == ArithOp::Sub)
        }
        _ => Err(bad()),
    }
}

fn is_instant(v: &Value) -> bool {
    matches!(
        v,
        Value::Date(_)
            | Value::LocalTime(_)
            | Value::LocalDateTime(_)
            | Value::ZonedTime(_)
            | Value::ZonedDateTime(_)
    )
}

/// `instant ± duration`.
fn shift(instant: &Value, duration: &Value, minus: bool) -> Result<Value, Error> {
    let (months, d2s) = match *duration {
        Value::YearToMonth(m) => (
            m,
            DayToSecond {
                seconds: 0,
                nanos: 0,
            },
        ),
        Value::DayToSecond(d) => (0, d),
        _ => return Err(super::expr::internal("a shift by a non-duration")),
    };
    let (months, d2s) = if minus {
        (
            months.checked_neg().ok_or_else(interval_overflow)?,
            negate_d2s(d2s)?,
        )
    } else {
        (months, d2s)
    };
    let months: i32 = i32::try_from(months).map_err(|_| datetime_overflow())?;
    let span = Span::new()
        .try_months(months)
        .map_err(|_| datetime_overflow())?;
    let sd = signed(d2s);
    let civil = |dt: DateTime| -> Result<DateTime, Error> {
        dt.checked_add(span)
            .and_then(|d| d.checked_add(sd))
            .map_err(|_| datetime_overflow())
    };
    Ok(match instant {
        Value::Date(d) => Value::Date(civil(d.to_datetime(Time::midnight()))?.date()),
        Value::LocalDateTime(d) => Value::LocalDateTime(civil(*d)?),
        Value::LocalTime(t) => Value::LocalTime(t.wrapping_add(sd)),
        Value::ZonedTime(z) => Value::ZonedTime(ZonedTime {
            time: z.time.wrapping_add(sd),
            offset: z.offset,
        }),
        Value::ZonedDateTime(z) => {
            let shifted = z
                .zoned
                .checked_add(span)
                .and_then(|x| x.checked_add(sd))
                .map_err(|_| datetime_overflow())?;
            let mut out = (**z).clone();
            out.zoned = shifted;
            Value::ZonedDateTime(Box::new(out))
        }
        _ => return Err(super::expr::internal("a shift of a non-instant")),
    })
}
