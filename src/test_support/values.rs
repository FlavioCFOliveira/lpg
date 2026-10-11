//! Seeded generators of values (rmp #54): every type of TS-21's runtime
//! value, with the edge cases the import and export round trips need, and
//! property values (IV011) of one kind or of mixed kinds.
//!
//! Moved from `src/types/tests.rs` (own code) and extended: NaN, ±infinity,
//! −0.0, subnormals, `i64::MIN` and `i64::MAX`, empty strings, U+0000 to
//! U+001F, U+FFFE, U+FFFF, `\r`, named and offset-only zoned values, both
//! durations, the four SRIDs, nested lists and records. Each domain is
//! small, so that equal and adjacent values are frequent, as the key-order
//! tests need. Seeds come from [`super::seed`] (`LPG_SEED`).

use jiff::Zoned;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{Offset, TimeZone};
use rand::RngExt;
use rand::rngs::StdRng;

use crate::sync::Arc;
use crate::types::{DayToSecond, Point, Srid, Value, ZonedDateTime, ZonedTime};

/// A zoned date-time with the named zone of `text`.
pub(crate) fn zoned(text: &str) -> Value {
    Value::ZonedDateTime(Box::new(ZonedDateTime::new(text.parse::<Zoned>().unwrap())))
}

/// An offset-only zoned date-time of the local date-time `dt` at `offset`
/// seconds.
pub(crate) fn offset_zoned(dt: &str, offset: i32) -> Value {
    let dt: DateTime = dt.parse().unwrap();
    let offset = Offset::from_seconds(offset).unwrap();
    let ts = dt.to_zoned(TimeZone::fixed(offset)).unwrap().timestamp();
    Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(ts, offset)))
}

/// A point.
pub(crate) fn point(srid: Srid, c: &[f64]) -> Value {
    Value::Point(Box::new(Point::new(srid, c).unwrap()))
}

fn small(rng: &mut StdRng) -> i64 {
    rng.random_range(-3i64..=3)
}

fn int(rng: &mut StdRng) -> i64 {
    match rng.random_range(0..6) {
        0 => small(rng),
        1 => [i64::MIN, i64::MAX, i64::MIN + 1, i64::MAX - 1][rng.random_range(0..4)],
        2 => (1i64 << 53) + small(rng),
        3 => -(1i64 << 53) + small(rng),
        4 => rng.random(),
        _ => small(rng) << rng.random_range(0..62),
    }
}

fn float(rng: &mut StdRng) -> f64 {
    match rng.random_range(0..9) {
        0 => small(rng) as f64,
        1 => small(rng) as f64 + 0.5,
        2 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 0.0][rng.random_range(0..5)],
        3 => 9_223_372_036_854_775_808.0 * [1.0, -1.0, 2.0][rng.random_range(0..3)],
        4 => (1u64 << 53) as f64 * if rng.random() { 1.0 } else { -1.0 },
        // Subnormals.
        5 => {
            f64::from_bits(rng.random::<u64>() & 0x000F_FFFF_FFFF_FFFF)
                * if rng.random() { 1.0 } else { -1.0 }
        }
        6 => small(rng) as f64 * 1e-300,
        7 => [f64::MAX, f64::MIN, f64::MIN_POSITIVE, 1e16, 1.25e-7, 0.1][rng.random_range(0..6)],
        _ => rng.random::<f64>() * 1e10 - 5e9,
    }
}

fn string(rng: &mut StdRng) -> Value {
    match rng.random_range(0..3) {
        0 => {
            // One control character, U+0000 to U+001F.
            let c = char::from(rng.random_range(0u8..0x20));
            Value::String(Arc::from(format!("a{c}").as_str()))
        }
        _ => Value::string(
            [
                "", "a", "a\0", "ab", "b", "é", "\0", "\r", "a\r\nb", "\u{FFFE}", "\u{FFFF}", "'",
                "\\", "it's", "a b", "<&>\"", "𝄞",
            ][rng.random_range(0..17)],
        ),
    }
}

fn bytes(rng: &mut StdRng) -> Value {
    Value::Bytes(Arc::from(
        [&b""[..], b"\0", b"\0\0", b"\x01", b"\xff", b"a\0"][rng.random_range(0..6)],
    ))
}

fn date(rng: &mut StdRng) -> Value {
    Value::Date(
        Date::new(
            rng.random_range(-2..=2) * 3000 + 2000,
            rng.random_range(1..=2),
            rng.random_range(1..=2),
        )
        .unwrap(),
    )
}

fn local_time(rng: &mut StdRng) -> Value {
    Value::LocalTime(Time::new(rng.random_range(0..3), 0, 0, rng.random_range(0..2)).unwrap())
}

fn local_date_time(rng: &mut StdRng) -> Value {
    Value::LocalDateTime(DateTime::from_parts(
        Date::new(2026, 1, rng.random_range(1..=2)).unwrap(),
        Time::new(rng.random_range(0..2), 0, 0, rng.random_range(0..2)).unwrap(),
    ))
}

fn zoned_time(rng: &mut StdRng) -> Value {
    Value::ZonedTime(ZonedTime {
        time: Time::new(rng.random_range(0..3), 0, 0, 0).unwrap(),
        offset: rng.random_range(-1..=1) * 3600 + [0, 1][rng.random_range(0..2)],
    })
}

fn zoned_date_time(rng: &mut StdRng) -> Value {
    // Instants with and without a fractional second on both sides of the
    // Unix epoch: jiff gives the fraction the instant's sign.
    let near_epoch = [
        "1969-12-31T23:59:58.5",
        "1969-12-31T23:59:59",
        "1969-12-31T23:59:59.5",
        "1970-01-01T00:00:00",
        "1970-01-01T00:00:00.5",
    ][rng.random_range(0..5)];
    match rng.random_range(0..8) {
        0 => offset_zoned("2026-03-28T01:00:00", rng.random_range(-1..=1) * 3600),
        1 => zoned("2026-03-28T01:00:00+00:00[UTC]"),
        2 => zoned("2026-03-28T01:00:00+00:00[Europe/Lisbon]"),
        3 => zoned("2026-03-28T01:00:00+00:00[Europe/London]"),
        4 => offset_zoned(near_epoch, rng.random_range(-1..=1) * 3600),
        5 => zoned("2015-07-21T21:40:32.142+02:00[Europe/Berlin]"),
        6 => zoned("2026-07-01T12:00:00-04:00[America/New_York]"),
        _ => zoned(&format!("{near_epoch}+00:00[UTC]")),
    }
}

fn year_to_month(rng: &mut StdRng) -> Value {
    Value::YearToMonth(small(rng) * [1, 13][rng.random_range(0..2)])
}

fn day_to_second(rng: &mut StdRng) -> Value {
    Value::DayToSecond(DayToSecond::new(small(rng) * 1_314_873, small(rng) * 500_000_000).unwrap())
}

fn point_value(rng: &mut StdRng) -> Value {
    let srid = [
        Srid::Cartesian2D,
        Srid::Wgs84_2D,
        Srid::Cartesian3D,
        Srid::Wgs84_3D,
    ][rng.random_range(0..4)];
    let c: Vec<f64> = (0..srid.dims())
        .map(|_| [0.0, -0.0, 1.0, -1.0, 2.5, f64::NAN][rng.random_range(0..6)])
        .collect();
    point(srid, &c)
}

/// A random value of every runtime type, of a small domain per group, so
/// that equal and adjacent values are frequent; lists and records nest
/// below `depth` 2.
pub(crate) fn random_value(rng: &mut StdRng, depth: u32) -> Value {
    let pick = rng.random_range(0..if depth > 1 { 17 } else { 20 });
    match pick {
        0 => Value::Null,
        1 => Value::Bool(rng.random()),
        2 => Value::Int(int(rng)),
        3 => Value::Float(float(rng)),
        4 => Value::Node(rng.random_range(0..4)),
        5 => Value::Edge(rng.random_range(0..4)),
        6 => date(rng),
        7 => local_time(rng),
        8 => local_date_time(rng),
        9 => zoned_time(rng),
        10 => zoned_date_time(rng),
        11 => year_to_month(rng),
        12 => day_to_second(rng),
        13 => string(rng),
        14 => bytes(rng),
        15 => Value::Path(Arc::from(
            [
                &[1u64][..],
                &[1, 5, 2],
                &[1, 5, 2, 6, 3],
                &[2, 5, 3],
                &[1, 4, 2],
            ][rng.random_range(0..5)],
        )),
        16 => point_value(rng),
        17 | 18 => {
            let n = rng.random_range(0..3);
            Value::List(
                (0..n)
                    .map(|_| random_value(rng, depth + 1))
                    .collect::<Vec<_>>()
                    .into(),
            )
        }
        _ => {
            let names = ["a", "b", "b c"];
            let n = rng.random_range(0..3);
            let mut fields: Vec<(Arc<str>, Value)> = Vec::new();
            for name in names.iter().take(n) {
                if rng.random() {
                    fields.push((Arc::from(*name), random_value(rng, depth + 1)));
                }
            }
            Value::Record(fields.into())
        }
    }
}

/// The kinds of property value (IV011) [`property_value`] makes: the
/// thirteen scalar types, a list of one scalar type, and the empty list.
pub(crate) const PROPERTY_KINDS: usize = 15;

/// A property value (IV011) of kind `kind` (below [`PROPERTY_KINDS`]): a
/// boolean, integer, float, string, byte string, the five temporal
/// instants, both durations, a point, a non-empty list of one scalar kind
/// (the kind drawn per value, so lists of one property may differ in
/// element type), or the empty list.
pub(crate) fn property_value(rng: &mut StdRng, kind: usize) -> Value {
    match kind {
        0 => Value::Bool(rng.random()),
        1 => Value::Int(int(rng)),
        2 => Value::Float(float(rng)),
        3 => string(rng),
        4 => bytes(rng),
        5 => date(rng),
        6 => local_time(rng),
        7 => local_date_time(rng),
        8 => zoned_time(rng),
        9 => zoned_date_time(rng),
        10 => year_to_month(rng),
        11 => day_to_second(rng),
        12 => point_value(rng),
        13 => {
            let k = rng.random_range(0..13);
            let n = rng.random_range(1..4);
            Value::List(
                (0..n)
                    .map(|_| property_value(rng, k))
                    .collect::<Vec<_>>()
                    .into(),
            )
        }
        _ => Value::List(Arc::from(Vec::new())),
    }
}

/// A property value of a kind drawn per call: a property whose values
/// come from it is mixed-type.
pub(crate) fn mixed_property_value(rng: &mut StdRng) -> Value {
    let k = rng.random_range(0..PROPERTY_KINDS);
    property_value(rng, k)
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;
    use crate::types::lexical::{TypeName, bit_eq, is_property_value};

    /// rmp #54 acceptance: with one fixed seed, 10 000 values hold every
    /// runtime type, NaN, ±infinity and −0.0; a property drawn in mixed
    /// mode has two types; two runs with the seed are equal.
    #[test]
    fn the_generator_covers_every_type_and_is_deterministic() {
        let run = |seed: u64| -> Vec<Value> {
            let mut rng = StdRng::seed_from_u64(seed);
            (0..10_000).map(|_| random_value(&mut rng, 0)).collect()
        };
        let a = run(1);
        let b = run(1);
        assert!(a.iter().zip(&b).all(|(x, y)| bit_eq(x, y)));
        let mut kinds = std::collections::BTreeSet::new();
        let (mut nan, mut inf, mut ninf, mut nzero) = (false, false, false, false);
        let mut walk = vec![];
        walk.extend(a.iter().cloned());
        while let Some(v) = walk.pop() {
            let t = TypeName::of(&v);
            kinds.insert(match t {
                TypeName::List(_) => "LIST".to_owned(),
                t => t.to_string(),
            });
            match &v {
                Value::Float(x) => {
                    nan |= x.is_nan();
                    inf |= *x == f64::INFINITY;
                    ninf |= *x == f64::NEG_INFINITY;
                    nzero |= x.to_bits() == (-0.0f64).to_bits();
                }
                Value::List(items) => walk.extend(items.iter().cloned()),
                Value::Record(fields) => walk.extend(fields.iter().map(|(_, v)| v.clone())),
                _ => {}
            }
        }
        assert_eq!(kinds.len(), 19, "{kinds:?}");
        assert!(nan && inf && ninf && nzero);
        let mut rng = StdRng::seed_from_u64(1);
        let mixed: Vec<Value> = (0..10).map(|_| mixed_property_value(&mut rng)).collect();
        assert!(mixed.iter().all(is_property_value));
        let types: std::collections::BTreeSet<String> =
            mixed.iter().map(|v| TypeName::of(v).to_string()).collect();
        assert!(types.len() >= 2, "{types:?}");
        for k in 0..PROPERTY_KINDS {
            assert!(is_property_value(&property_value(&mut rng, k)), "kind {k}");
        }
    }
}
