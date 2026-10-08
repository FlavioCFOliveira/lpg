//! Index-key encoding (TS-21 Index key encoding, TS-18).
//!
//! [`encode`] writes a value as bytes whose byte-wise order is the approved
//! total order ([`super::total_cmp`]; BR-GQL-002, user decision
//! 2026-10-07): values that the order holds equal, such as 3 and 3.0, get
//! equal bytes, and every encoding is self-delimiting, so the encodings of
//! a composite key concatenate ([`encode_all`]) and compare element by
//! element. Keys are compared only, never decoded: an index entry's value
//! is read from its record.
//!
//! - **Group.** One byte first, in the order of [`Group`]; NULL is 0xF0,
//!   above every other.
//! - **Numbers** (one group for INT64 and FLOAT64): a class byte (−∞,
//!   negative, zero, positive, +∞, NaN), then for a finite non-zero value
//!   its exact binary form: the exponent of its leading one bit (biased,
//!   two bytes) and the bits after it, left-aligned in eight bytes; both
//!   complemented for a negative value. Every `i64` and every finite `f64`
//!   is exact in this form, so equal values have equal bytes, and −0.0 and
//!   +0.0 are both zero.
//! - **Strings, byte strings, zone names**: the bytes with 0x00 escaped as
//!   0x00 0xFF, ended by 0x00 0x00, so a proper prefix sorts first
//!   (`storekey` and `memcomparable` use similar escapes; studied only).
//! - **Signed integers** in fixed width with the sign bit flipped, big
//!   endian; **floats** of point coordinates in the sortable form of their
//!   bits (sign bit flipped, or all bits for a negative), −0.0 as +0.0 and
//!   NaN highest.
//! - **Lists, paths**: each element after a 0x01 marker, then 0x00, so a
//!   proper prefix sorts first; **records**: the field count, the sorted
//!   names, then the values in name order (Neo4j `MapValue`).

use super::{Group, Value, duration_parts, nanos_of_day, sorted_fields, temporal};

/// The group byte.
fn group_byte(g: Group) -> u8 {
    match g {
        Group::Null => 0xF0,
        g => 0x10 + 4 * g as u8,
    }
}

fn put_i64(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&((v as u64) ^ (1 << 63)).to_be_bytes());
}

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&((v as u32) ^ (1 << 31)).to_be_bytes());
}

fn put_i128(out: &mut Vec<u8>, v: i128) {
    out.extend_from_slice(&((v as u128) ^ (1 << 127)).to_be_bytes());
}

/// `b` escaped and terminated (module documentation).
fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    for &x in b {
        out.push(x);
        if x == 0 {
            out.push(0xFF);
        }
    }
    out.extend_from_slice(&[0, 0]);
}

/// The sortable bits of a float coordinate: −0.0 as +0.0, every NaN as
/// one value above +∞.
fn put_f64(out: &mut Vec<u8>, f: f64) {
    let bits = if f.is_nan() {
        u64::MAX
    } else {
        let b = if f == 0.0 { 0 } else { f.to_bits() };
        if b >> 63 == 1 { !b } else { b | 1 << 63 }
    };
    out.extend_from_slice(&bits.to_be_bytes());
}

/// Number classes.
const NEG_INF: u8 = 1;
const NEG: u8 = 2;
const ZERO: u8 = 3;
const POS: u8 = 4;
const POS_INF: u8 = 5;
const NAN: u8 = 6;

/// Exponent bias: the smallest exponent, of the smallest subnormal float,
/// is −1074.
const BIAS: i32 = 1100;

/// The exponent of the leading one bit and the bits after it,
/// left-aligned, of the magnitude `m × 2^e`.
fn normal(m: u64, e: i32) -> (i32, u64) {
    debug_assert!(m != 0);
    let lead = 63 - m.leading_zeros() as i32;
    // Drop the leading one; left-align the rest.
    let rest = if lead == 0 { 0 } else { m << (64 - lead) };
    (e + lead, rest)
}

/// The exact binary form of a number: its class and, for a finite
/// non-zero value, its exponent and mantissa.
fn number_form(v: &Value) -> (u8, i32, u64) {
    match *v {
        Value::Int(0) => (ZERO, 0, 0),
        Value::Int(i) => {
            let (e, m) = normal(i.unsigned_abs(), 0);
            (if i < 0 { NEG } else { POS }, e, m)
        }
        Value::Float(f) => {
            if f.is_nan() {
                (NAN, 0, 0)
            } else if f == f64::INFINITY {
                (POS_INF, 0, 0)
            } else if f == f64::NEG_INFINITY {
                (NEG_INF, 0, 0)
            } else if f == 0.0 {
                (ZERO, 0, 0)
            } else {
                let bits = f.to_bits();
                let exp = ((bits >> 52) & 0x7FF) as i32;
                let frac = bits & ((1 << 52) - 1);
                let (m, e) = if exp == 0 {
                    (frac, -1074)
                } else {
                    (frac | 1 << 52, exp - 1075)
                };
                let (e, m) = normal(m, e);
                (if f < 0.0 { NEG } else { POS }, e, m)
            }
        }
        _ => unreachable!("numbers only"),
    }
}

/// Appends the key encoding of `v` to `out`.
pub(crate) fn encode(v: &Value, out: &mut Vec<u8>) {
    out.push(group_byte(v.group()));
    match v {
        Value::Null => {}
        Value::Bool(b) => out.push(u8::from(*b)),
        Value::Int(_) | Value::Float(_) => {
            let (class, e, m) = number_form(v);
            out.push(class);
            if class == POS || class == NEG {
                let e = u16::try_from(e + BIAS).expect("exponent within the bias");
                let (e, m) = if class == NEG { (!e, !m) } else { (e, m) };
                out.extend_from_slice(&e.to_be_bytes());
                out.extend_from_slice(&m.to_be_bytes());
            }
        }
        Value::Node(id) | Value::Edge(id) => out.extend_from_slice(&id.to_be_bytes()),
        Value::Date(d) => {
            out.extend_from_slice(&((d.year() as u16) ^ 0x8000).to_be_bytes());
            out.push(d.month() as u8);
            out.push(d.day() as u8);
        }
        Value::LocalTime(t) => put_i64(out, nanos_of_day(*t)),
        Value::LocalDateTime(dt) => {
            let d = dt.date();
            out.extend_from_slice(&((d.year() as u16) ^ 0x8000).to_be_bytes());
            out.push(d.month() as u8);
            out.push(d.day() as u8);
            put_i64(out, nanos_of_day(dt.time()));
        }
        Value::ZonedTime(z) => {
            put_i64(out, z.nanos_of_day_utc());
            put_i32(out, z.offset);
        }
        Value::ZonedDateTime(z) => {
            let ts = z.zoned.timestamp();
            put_i64(out, ts.as_second());
            put_i32(out, ts.subsec_nanosecond());
            put_i32(out, z.zoned.offset().seconds());
            match z.zone_name() {
                None => out.push(0),
                Some(name) => {
                    out.push(1);
                    put_bytes(out, temporal::normalize_zone(name).as_bytes());
                }
            }
        }
        Value::YearToMonth(_) | Value::DayToSecond(_) => {
            let (avg, nanos, months, days, seconds) = duration_parts(v).expect("a duration");
            put_i128(out, avg);
            out.extend_from_slice(&nanos.to_be_bytes());
            put_i64(out, months);
            put_i64(out, days);
            put_i64(out, seconds);
        }
        Value::String(s) => put_bytes(out, s.as_bytes()),
        Value::Bytes(b) => put_bytes(out, b),
        Value::List(items) => {
            for item in items.iter() {
                out.push(1);
                encode(item, out);
            }
            out.push(0);
        }
        Value::Record(fields) => {
            let sorted = sorted_fields(fields);
            out.extend_from_slice(&(sorted.len() as u32).to_be_bytes());
            for (name, _) in &sorted {
                put_bytes(out, name.as_bytes());
            }
            for (_, value) in &sorted {
                encode(value, out);
            }
        }
        Value::Path(ids) => {
            out.extend_from_slice(&ids.first().copied().unwrap_or(0).to_be_bytes());
            for edge in ids.iter().skip(1).step_by(2) {
                out.push(1);
                out.extend_from_slice(&edge.to_be_bytes());
            }
            out.push(0);
        }
        Value::Point(p) => {
            out.extend_from_slice(&p.srid().code().to_be_bytes());
            out.push(p.srid().dims() as u8);
            for &c in p.coords() {
                put_f64(out, c);
            }
        }
    }
}

/// The key encoding of the composite key `values` (TS-18): the
/// encodings of its values in order.
pub(crate) fn encode_all(values: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        encode(v, &mut out);
    }
    out
}
