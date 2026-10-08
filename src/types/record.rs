//! Record encoding (TS-21 Record encoding): the labels and properties of a
//! node or edge record, the value of its entry in the node or edge tree
//! (TS-06). A record longer than the inline threshold is stored in an
//! overflow chain by the tree (TS-05).
//!
//! **Layout.** The label count (varint), then the label tokens in
//! ascending order, the first as a varint and each later one as the varint
//! delta from the one before; then the properties in ascending key-token
//! order, to the end of the record. Each property is a header byte, the
//! key token as a varint delta from the previous key (the first key
//! itself), then its value's payload.
//!
//! **Header.** Type tag in the high five bits, length class in the low
//! three, Memgraph's one-byte metadata of type and sizes (R-MG @ `847e224`
//! `src/storage/v2/property_store.cpp:63-118`). The length class is the
//! byte width of an integer or a reference (0: zero, 1: 1, 2: 2, 3: 4,
//! 4: 8 bytes), of a float (3: lossless as 4 bytes, 4: 8 bytes), or of the
//! length or count field that precedes a string, byte string, list,
//! record or path (0: empty, 1: 1, 2: 2, 3: 4 bytes). Other types encode
//! their fields as varints and use class 0, except a zoned date-time (0:
//! with an offset; 1: with a named zone, whose name precedes the offset)
//! and a boolean (its value). A zoned date-time's instant is its floor
//! Unix seconds, then nanoseconds in `0..1e9`.
//! Integers are little-endian of minimum width; signed varint fields are
//! zigzag-encoded.
//!
//! **Sizes** (IL001, IL002, IL013, IL015; user decision 2026-10-07):
//! counts and lengths are `u32` fields, so each is at most 2³² − 1; the
//! memory cap bounds them first in practice.
//!
//! **Reserved tags** (user decision 2026-10-07): one per numeric type of
//! features GV11, GV13-GV17, GV20, GV25, GV26 ([`RESERVED`]); none is
//! written, and reading one is a format error.

use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::Offset;

use super::{DayToSecond, Point, Srid, Value, ZonedDateTime, ZonedTime, nanos_of_day};
use crate::sync::Arc;

const NULL: u8 = 0;
const BOOL: u8 = 1;
const INT: u8 = 2;
const FLOAT: u8 = 3;
const STRING: u8 = 4;
const BYTES: u8 = 5;
const LIST: u8 = 6;
const RECORD: u8 = 7;
const PATH: u8 = 8;
const NODE: u8 = 9;
const EDGE: u8 = 10;
const DATE: u8 = 11;
const LOCAL_TIME: u8 = 12;
const LOCAL_DATE_TIME: u8 = 13;
const ZONED_TIME: u8 = 14;
const ZONED_DATE_TIME: u8 = 15;
const YEAR_TO_MONTH: u8 = 16;
const DAY_TO_SECOND: u8 = 17;
const POINT: u8 = 18;

/// Tags reserved for the numeric types not supported (BR-GQL-002): UINT64
/// (GV11), UINT128 (GV13), INT128 (GV14), UINT256 (GV15), INT256 (GV16),
/// DECIMAL (GV17), FLOAT16 (GV20), FLOAT128 (GV25), FLOAT256 (GV26).
pub(crate) const RESERVED: [u8; 9] = [19, 20, 21, 22, 23, 24, 25, 26, 27];

/// Why a record cannot be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TooLarge {
    /// A count or length exceeds 2³² − 1 (IL001, IL002, IL013, IL015).
    Size,
    /// A zoned time's offset is outside ±93 599 s, or a
    /// `DURATION(DAY TO SECOND)`'s nanoseconds are not in `0..1e9`: the
    /// decoder would reject the record.
    Range,
}

fn put_varint(out: &mut Vec<u8>, v: u64) {
    crate::storage::btree::node::put_varint(out, v);
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// The width class of an unsigned integer: 0 for zero, else 1..4 for 1, 2,
/// 4 or 8 bytes.
fn width_class(v: u64) -> u8 {
    match v {
        0 => 0,
        1..=0xFF => 1,
        0x100..=0xFFFF => 2,
        0x1_0000..=0xFFFF_FFFF => 3,
        _ => 4,
    }
}

fn width(class: u8) -> usize {
    [0, 1, 2, 4, 8][usize::from(class)]
}

fn put_uint(out: &mut Vec<u8>, v: u64, class: u8) {
    out.extend_from_slice(&v.to_le_bytes()[..width(class)]);
}

/// The class and bytes of a length or count, at most `u32`.
fn put_len(out: &mut Vec<u8>, header: u8, n: usize) -> Result<(), TooLarge> {
    let n = u32::try_from(n).map_err(|_| TooLarge::Size)?;
    let class = width_class(u64::from(n)).min(3);
    out.push(header << 3 | class);
    put_uint(out, u64::from(n), class);
    Ok(())
}

/// Appends a value as header byte and payload.
fn put_value(out: &mut Vec<u8>, v: &Value) -> Result<(), TooLarge> {
    let head = |out: &mut Vec<u8>, tag: u8, class: u8| out.push(tag << 3 | class);
    match v {
        Value::Null => head(out, NULL, 0),
        Value::Bool(b) => head(out, BOOL, u8::from(*b)),
        Value::Int(i) => {
            // The smallest width that holds the value sign-extended.
            let class = if *i == 0 {
                0
            } else if i8::try_from(*i).is_ok() {
                1
            } else if i16::try_from(*i).is_ok() {
                2
            } else if i32::try_from(*i).is_ok() {
                3
            } else {
                4
            };
            head(out, INT, class);
            put_uint(out, *i as u64, class);
        }
        Value::Float(f) => {
            let narrow = *f as f32;
            if f64::from(narrow).to_bits() == f.to_bits() {
                head(out, FLOAT, 3);
                out.extend_from_slice(&narrow.to_le_bytes());
            } else {
                head(out, FLOAT, 4);
                out.extend_from_slice(&f.to_le_bytes());
            }
        }
        Value::String(s) => {
            put_len(out, STRING, s.len())?;
            out.extend_from_slice(s.as_bytes());
        }
        Value::Bytes(b) => {
            put_len(out, BYTES, b.len())?;
            out.extend_from_slice(b);
        }
        Value::List(items) => {
            put_len(out, LIST, items.len())?;
            for item in items.iter() {
                put_value(out, item)?;
            }
        }
        Value::Record(fields) => {
            put_len(out, RECORD, fields.len())?;
            for (name, value) in fields.iter() {
                u32::try_from(name.len()).map_err(|_| TooLarge::Size)?;
                put_varint(out, name.len() as u64);
                out.extend_from_slice(name.as_bytes());
                put_value(out, value)?;
            }
        }
        Value::Path(ids) => {
            put_len(out, PATH, ids.len())?;
            for &id in ids.iter() {
                put_varint(out, id);
            }
        }
        Value::Node(id) | Value::Edge(id) => {
            let class = width_class(*id);
            head(
                out,
                if matches!(v, Value::Node(_)) {
                    NODE
                } else {
                    EDGE
                },
                class,
            );
            put_uint(out, *id, class);
        }
        Value::Date(d) => {
            head(out, DATE, 0);
            put_date(out, *d);
        }
        Value::LocalTime(t) => {
            head(out, LOCAL_TIME, 0);
            put_varint(out, nanos_of_day(*t) as u64);
        }
        Value::LocalDateTime(dt) => {
            head(out, LOCAL_DATE_TIME, 0);
            put_date(out, dt.date());
            put_varint(out, nanos_of_day(dt.time()) as u64);
        }
        Value::ZonedTime(z) => {
            Offset::from_seconds(z.offset).map_err(|_| TooLarge::Range)?;
            head(out, ZONED_TIME, 0);
            put_varint(out, nanos_of_day(z.time) as u64);
            put_varint(out, zigzag(i64::from(z.offset)));
        }
        Value::ZonedDateTime(z) => {
            let name = z.zone_name();
            head(out, ZONED_DATE_TIME, u8::from(name.is_some()));
            // Floor seconds and nanoseconds in `0..1e9`, as Java's
            // `Instant` and Neo4j's property store keep them: jiff gives
            // the fractional second the instant's sign (TS-21).
            let ts = z.zoned.timestamp();
            let (secs, nanos) = match ts.subsec_nanosecond() {
                n if n < 0 => (ts.as_second() - 1, n + 1_000_000_000),
                n => (ts.as_second(), n),
            };
            put_varint(out, zigzag(secs));
            put_varint(out, nanos as u64);
            // A named zone keeps its offset too, so the value decodes the
            // same on a host without the zone or with other rules (TS-21).
            if let Some(n) = name {
                put_varint(out, n.len() as u64);
                out.extend_from_slice(n.as_bytes());
            }
            put_varint(out, zigzag(i64::from(z.zoned.offset().seconds())));
        }
        Value::YearToMonth(m) => {
            head(out, YEAR_TO_MONTH, 0);
            put_varint(out, zigzag(*m));
        }
        Value::DayToSecond(d) => {
            if d.nanos >= 1_000_000_000 {
                return Err(TooLarge::Range);
            }
            head(out, DAY_TO_SECOND, 0);
            put_varint(out, zigzag(d.seconds));
            put_varint(out, u64::from(d.nanos));
        }
        Value::Point(p) => {
            head(out, POINT, 0);
            put_varint(out, u64::from(p.srid().code()));
            for c in p.coords() {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    Ok(())
}

fn put_date(out: &mut Vec<u8>, d: Date) {
    put_varint(out, zigzag(i64::from(d.year())));
    out.push(d.month() as u8);
    out.push(d.day() as u8);
}

/// Encodes a record with `labels` and properties `props` (key token,
/// value) into `out`. Labels and keys are sorted here; a repeated label is
/// kept once, and a repeated key keeps its last value.
///
/// # Errors
///
/// [`TooLarge::Size`] if a count or length exceeds 2³² − 1;
/// [`TooLarge::Range`] if a value is outside the domain [`decode`] accepts.
pub(crate) fn encode(
    labels: &[u64],
    props: &[(u64, Value)],
    out: &mut Vec<u8>,
) -> Result<(), TooLarge> {
    let mut labels = labels.to_vec();
    labels.sort_unstable();
    labels.dedup();
    u32::try_from(labels.len()).map_err(|_| TooLarge::Size)?;
    put_varint(out, labels.len() as u64);
    let mut prev = 0;
    for (i, &l) in labels.iter().enumerate() {
        put_varint(out, if i == 0 { l } else { l - prev });
        prev = l;
    }
    let mut keys: Vec<usize> = (0..props.len()).collect();
    // Stable: equal keys keep their order, and the last one wins below.
    keys.sort_by_key(|&i| props[i].0);
    let mut last: Option<u64> = None;
    let mut entries: Vec<&(u64, Value)> = Vec::with_capacity(props.len());
    for i in keys {
        if last == Some(props[i].0) {
            entries.pop();
        }
        last = Some(props[i].0);
        entries.push(&props[i]);
    }
    u32::try_from(entries.len()).map_err(|_| TooLarge::Size)?;
    let mut prev: Option<u64> = None;
    for (k, v) in entries {
        let start = out.len();
        put_value(out, v)?;
        // The key delta goes right after the header byte.
        let delta = prev.map_or(*k, |p| k - p);
        let mut d = Vec::with_capacity(4);
        put_varint(&mut d, delta);
        out.splice(start + 1..start + 1, d);
        prev = Some(*k);
    }
    Ok(())
}

/// A cursor over encoded bytes; every read is checked.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= u64::from(b & 0x7F) << shift;
            if b < 0x80 {
                return Some(v);
            }
        }
        None
    }

    fn uint(&mut self, class: u8) -> Option<u64> {
        if class > 4 {
            return None;
        }
        let mut buf = [0u8; 8];
        let w = width(class);
        buf[..w].copy_from_slice(self.take(w)?);
        Some(u64::from_le_bytes(buf))
    }

    fn len(&mut self, class: u8) -> Option<usize> {
        if class > 3 {
            return None;
        }
        usize::try_from(self.uint(class)?).ok()
    }

    fn date(&mut self) -> Option<Date> {
        let y = i16::try_from(unzigzag(self.varint()?)).ok()?;
        let m = self.byte()? as i8;
        let d = self.byte()? as i8;
        Date::new(y, m, d).ok()
    }

    fn time(&mut self) -> Option<Time> {
        let n = self.varint()?;
        if n >= 86_400 * 1_000_000_000 {
            return None;
        }
        let secs = n / 1_000_000_000;
        Time::new(
            (secs / 3600) as i8,
            (secs % 3600 / 60) as i8,
            (secs % 60) as i8,
            (n % 1_000_000_000) as i32,
        )
        .ok()
    }

    fn value(&mut self, depth: u32) -> Option<Value> {
        let h = self.byte()?;
        self.payload(h, depth)
    }

    /// The value whose header byte `h` was just read.
    fn payload(&mut self, h: u8, depth: u32) -> Option<Value> {
        // Nested lists and records are bounded: a format error, not a stack
        // overflow, ends a malformed one.
        if depth > 64 {
            return None;
        }
        let (tag, class) = (h >> 3, h & 7);
        Some(match tag {
            NULL if class == 0 => Value::Null,
            BOOL if class <= 1 => Value::Bool(class == 1),
            INT => {
                let raw = self.uint(class)?;
                let w = width(class) as u32 * 8;
                // Sign-extend from the stored width.
                Value::Int(if w == 0 {
                    0
                } else {
                    ((raw << (64 - w)) as i64) >> (64 - w)
                })
            }
            FLOAT if class == 3 => Value::Float(f64::from(f32::from_le_bytes(
                self.take(4)?.try_into().ok()?,
            ))),
            FLOAT if class == 4 => Value::Float(f64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            STRING => {
                let n = self.len(class)?;
                Value::String(Arc::from(std::str::from_utf8(self.take(n)?).ok()?))
            }
            BYTES => {
                let n = self.len(class)?;
                Value::Bytes(Arc::from(self.take(n)?))
            }
            LIST => {
                let n = self.len(class)?;
                let mut items = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    items.push(self.value(depth + 1)?);
                }
                Value::List(items.into())
            }
            RECORD => {
                let n = self.len(class)?;
                let mut fields = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    let len = usize::try_from(self.varint()?).ok()?;
                    let name = std::str::from_utf8(self.take(len)?).ok()?;
                    fields.push((Arc::from(name), self.value(depth + 1)?));
                }
                Value::Record(fields.into())
            }
            PATH => {
                let n = self.len(class)?;
                let mut ids = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    ids.push(self.varint()?);
                }
                Value::Path(ids.into())
            }
            NODE => Value::Node(self.uint(class)?),
            EDGE => Value::Edge(self.uint(class)?),
            DATE if class == 0 => Value::Date(self.date()?),
            LOCAL_TIME if class == 0 => Value::LocalTime(self.time()?),
            LOCAL_DATE_TIME if class == 0 => {
                let d = self.date()?;
                Value::LocalDateTime(DateTime::from_parts(d, self.time()?))
            }
            ZONED_TIME if class == 0 => {
                let time = self.time()?;
                let offset = i32::try_from(unzigzag(self.varint()?)).ok()?;
                Offset::from_seconds(offset).ok()?;
                Value::ZonedTime(ZonedTime { time, offset })
            }
            ZONED_DATE_TIME if class <= 1 => {
                let secs = unzigzag(self.varint()?);
                let nanos = i32::try_from(self.varint()?).ok()?;
                if !(0..1_000_000_000).contains(&nanos) {
                    return None;
                }
                let ts = Timestamp::new(secs, nanos).ok()?;
                let name = if class == 1 {
                    let len = usize::try_from(self.varint()?).ok()?;
                    Some(std::str::from_utf8(self.take(len)?).ok()?.to_owned())
                } else {
                    None
                };
                let off = i32::try_from(unzigzag(self.varint()?)).ok()?;
                let off = Offset::from_seconds(off).ok()?;
                let z = match name {
                    Some(name) => ZonedDateTime::stored(ts, &name, off),
                    None => ZonedDateTime::with_offset(ts, off),
                };
                Value::ZonedDateTime(Box::new(z))
            }
            YEAR_TO_MONTH if class == 0 => Value::YearToMonth(unzigzag(self.varint()?)),
            DAY_TO_SECOND if class == 0 => {
                let seconds = unzigzag(self.varint()?);
                let nanos = u32::try_from(self.varint()?).ok()?;
                if nanos >= 1_000_000_000 {
                    return None;
                }
                Value::DayToSecond(DayToSecond { seconds, nanos })
            }
            POINT if class == 0 => {
                let srid = Srid::from_code(u32::try_from(self.varint()?).ok()?)?;
                let mut c = Vec::with_capacity(3);
                for _ in 0..srid.dims() {
                    c.push(f64::from_le_bytes(self.take(8)?.try_into().ok()?));
                }
                Value::Point(Box::new(Point::new(srid, &c)?))
            }
            _ => return None,
        })
    }
}

/// The label tokens at the start of a record (module documentation).
fn read_labels(r: &mut Reader<'_>) -> Option<Vec<u64>> {
    let n = usize::try_from(r.varint()?).ok()?;
    let mut labels = Vec::with_capacity(n.min(1024));
    for i in 0..n {
        let d = r.varint()?;
        let l = if i == 0 {
            d
        } else {
            if d == 0 {
                return None;
            }
            labels.last().copied().unwrap_or(0u64).checked_add(d)?
        };
        labels.push(l);
    }
    Some(labels)
}

/// The label tokens of a record, in ascending order, without decoding its
/// properties (the statistics, TS-17). `None` for malformed label bytes.
pub(crate) fn decode_labels(b: &[u8]) -> Option<Vec<u64>> {
    read_labels(&mut Reader { b, at: 0 })
}

/// A decoded record: its label tokens, and its properties as (key token,
/// value) in key order.
pub(crate) type Decoded = (Vec<u64>, Vec<(u64, Value)>);

/// Decodes a record: its labels and its properties in key order. `None`
/// for malformed bytes, a reserved tag included, or a named zone the
/// operating system does not know.
pub(crate) fn decode(b: &[u8]) -> Option<Decoded> {
    let mut r = Reader { b, at: 0 };
    let labels = read_labels(&mut r)?;
    let mut props = Vec::new();
    let mut prev: Option<u64> = None;
    while r.at < b.len() {
        // The header byte, then the key delta, then the payload.
        let h = r.byte()?;
        let d = r.varint()?;
        let k = match prev {
            None => d,
            Some(p) if d > 0 => p.checked_add(d)?,
            Some(_) => return None,
        };
        props.push((k, r.payload(h, 0)?));
        prev = Some(k);
    }
    Some((labels, props))
}
