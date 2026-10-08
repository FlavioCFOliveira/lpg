//! Tests of values and types (TS-21, rmp #30).

use std::cmp::Ordering;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{Offset, TimeZone};
use jiff::{Timestamp, Zoned};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::spatial::{self, Point, Srid, distance};
use super::{DayToSecond, Value, ZonedDateTime, ZonedTime, key, record, temporal, total_cmp};
use crate::sync::Arc;
use crate::test_support::seed;

fn zoned(text: &str) -> Value {
    Value::ZonedDateTime(Box::new(ZonedDateTime::new(text.parse::<Zoned>().unwrap())))
}

fn offset_zoned(dt: &str, offset: i32) -> Value {
    let dt: DateTime = dt.parse().unwrap();
    let offset = Offset::from_seconds(offset).unwrap();
    let ts = dt.to_zoned(TimeZone::fixed(offset)).unwrap().timestamp();
    Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(ts, offset)))
}

fn point(srid: Srid, c: &[f64]) -> Value {
    Value::Point(Box::new(Point::new(srid, c).unwrap()))
}

/// One value of every approved type (BR-GQL-002, user decision
/// 2026-10-07), nested ones included.
fn every_type() -> Vec<Value> {
    vec![
        Value::Null,
        Value::Bool(false),
        Value::Bool(true),
        Value::Int(0),
        Value::Int(-1),
        Value::Int(127),
        Value::Int(-129),
        Value::Int(40_000),
        Value::Int(i64::from(i32::MIN) - 1),
        Value::Int(i64::MAX),
        Value::Int(i64::MIN),
        Value::Float(0.5),
        Value::Float(-0.0),
        Value::Float(0.1),
        Value::Float(f64::INFINITY),
        Value::Float(f64::MIN_POSITIVE / 8.0),
        Value::Node(0),
        Value::Node(u64::MAX),
        Value::Edge(77_777),
        Value::Date(Date::new(-9999, 1, 1).unwrap()),
        Value::Date(Date::new(2026, 10, 8).unwrap()),
        Value::LocalTime(Time::new(23, 59, 59, 999_999_999).unwrap()),
        Value::LocalDateTime("2026-10-08T12:34:56.789".parse().unwrap()),
        Value::ZonedTime(ZonedTime {
            time: Time::new(8, 0, 0, 1).unwrap(),
            offset: -(3 * 3600 + 30 * 60),
        }),
        zoned("2026-07-01T12:00:00+01:00[Europe/Lisbon]"),
        zoned("2026-01-01T00:00:00+00:00[Europe/Lisbon]"),
        offset_zoned("2026-01-01T00:00:00", 5 * 3600 + 45 * 60),
        offset_zoned("2026-01-01T00:00:00", 0),
        Value::YearToMonth(-27),
        Value::DayToSecond(DayToSecond::new(3 * 86_400 + 5, 500).unwrap()),
        Value::DayToSecond(DayToSecond::new(-1, 999_999_999).unwrap()),
        Value::string(""),
        Value::string("ação 🚀\0x"),
        Value::Bytes(Arc::from(&b"\0\xff\0"[..])),
        Value::Bytes(Arc::from(&[][..])),
        Value::List(Arc::from(vec![
            Value::Int(1),
            Value::string("a"),
            Value::List(Arc::from(vec![Value::Null])),
        ])),
        Value::Record(Arc::from(vec![
            (Arc::from("b"), Value::Int(2)),
            (Arc::from("a"), Value::Float(1.5)),
        ])),
        Value::Path(Arc::from(vec![1u64, 10, 2, 11, 3])),
        point(Srid::Cartesian2D, &[3.0, -4.5]),
        point(Srid::Cartesian3D, &[1.0, 2.0, 3.0]),
        point(Srid::Wgs84_2D, &[-9.1393, 38.7223]),
        point(Srid::Wgs84_3D, &[-9.1393, 38.7223, 120.5]),
    ]
}

#[test]
fn a_value_is_24_bytes() {
    assert_eq!(std::mem::size_of::<Value>(), 24);
}

/// Every approved type round-trips through the record encoding, as a
/// property value and inside lists and records (FR-TYPE-001, FR-TYPE-002).
#[test]
fn every_type_round_trips_through_the_record_encoding() {
    let values = every_type();
    let props: Vec<(u64, Value)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (i as u64 * 3 + 1, v.clone()))
        .collect();
    let labels = [9u64, 2, 300_000, 2];
    let mut b = Vec::new();
    record::encode(&labels, &props, &mut b).unwrap();
    let (l, p) = record::decode(&b).unwrap();
    assert_eq!(l, vec![2, 9, 300_000]);
    assert_eq!(p, props);
    // Nested as one list.
    let mut b = Vec::new();
    record::encode(&[], &[(5, Value::List(values.clone().into()))], &mut b).unwrap();
    assert_eq!(
        record::decode(&b).unwrap().1,
        vec![(5, Value::List(values.into()))]
    );
    // The zoned date-time keeps its instant and zone name.
    let Value::ZonedDateTime(z) = &p[24].1 else {
        panic!("{:?}", p[24]);
    };
    assert_eq!(z.zone_name(), Some("Europe/Lisbon"));
    assert_eq!(
        z.zoned.timestamp(),
        "2026-07-01T11:00:00Z".parse::<Timestamp>().unwrap()
    );
    // Points keep their coordinates and SRID.
    for (srid, c) in [
        (Srid::Cartesian2D, vec![3.0, -4.5]),
        (Srid::Cartesian3D, vec![1.0, 2.0, 3.0]),
        (Srid::Wgs84_2D, vec![-9.1393, 38.7223]),
        (Srid::Wgs84_3D, vec![-9.1393, 38.7223, 120.5]),
    ] {
        let found = p.iter().find_map(|(_, v)| match v {
            Value::Point(q) if q.srid() == srid => Some(q.coords().to_vec()),
            _ => None,
        });
        assert_eq!(found, Some(c), "{}", srid.code());
    }
    // A repeated key keeps its last value; a NaN keeps its bits.
    let mut b = Vec::new();
    record::encode(
        &[],
        &[(1, Value::Int(1)), (1, Value::Float(f64::NAN))],
        &mut b,
    )
    .unwrap();
    let (_, p) = record::decode(&b).unwrap();
    assert!(matches!(p.as_slice(), [(1, Value::Float(f))] if f.is_nan()));
}

/// A zoned date-time before 1970 with a fractional second, offset-only and
/// named, round-trips through the record encoding, as do the extreme
/// instants (TS-21). jiff gives the fraction the instant's sign; the
/// record keeps floor seconds and nanoseconds in `0..1e9`.
#[test]
fn a_fractional_instant_before_1970_round_trips_through_the_record_encoding() {
    let min = Timestamp::MIN;
    let max = Timestamp::MAX;
    let utc = Offset::from_seconds(0).unwrap();
    for v in [
        offset_zoned("1969-12-31T23:59:59.5", 0),
        offset_zoned("1969-12-31T23:59:59.000000001", -3600),
        offset_zoned("1900-06-30T12:00:00.999999999", 3600),
        zoned("1969-12-31T23:59:59.5+00:00[UTC]"),
        zoned("1950-01-01T00:00:00.25+00:00[Europe/Lisbon]"),
        Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(min, utc))),
        Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(
            Timestamp::new(min.as_second() + 1, -1).unwrap(),
            utc,
        ))),
        Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(max, utc))),
    ] {
        let mut b = Vec::new();
        record::encode(&[], &[(1, v.clone())], &mut b).unwrap();
        assert_eq!(
            record::decode(&b),
            Some((vec![], vec![(1, v.clone())])),
            "{v:?}"
        );
    }
    // A non-canonical nanosecond field is rejected: 1e9 after a 0 second.
    let mut b = Vec::new();
    record::encode(&[], &[(1, offset_zoned("1970-01-01T00:00:00", 0))], &mut b).unwrap();
    let at = b.len() - 2; // [.., secs 0, nanos 0, offset 0]
    assert_eq!(&b[at - 1..], &[0, 0, 0]);
    let mut bad = b[..at].to_vec();
    bad.extend_from_slice(&[0x80, 0x94, 0xEB, 0xDC, 0x03, 0]); // varint 1e9
    assert!(record::decode(&bad).is_none());
}

/// Seeded random values round-trip through the record encoding: decoding
/// succeeds and re-encoding gives the same bytes (TS-21). The seed is
/// printed; `LPG_SEED` overrides it.
#[test]
fn random_values_round_trip_through_the_record_encoding() {
    let mut rng = StdRng::seed_from_u64(seed());
    for _ in 0..20_000 {
        let v = random_value(&mut rng, 0);
        let mut b = Vec::new();
        record::encode(&[], &[(1, v.clone())], &mut b).unwrap();
        let (_, p) = record::decode(&b).unwrap_or_else(|| panic!("{v:?} does not decode"));
        let mut again = Vec::new();
        record::encode(&[], &p, &mut again).unwrap();
        assert_eq!(again, b, "{v:?}");
    }
}

/// Minimum-width payloads: small integers take one payload byte, a float
/// that a 32-bit float holds exactly takes four.
#[test]
fn payloads_take_their_minimum_width() {
    let size = |v: Value| {
        let mut b = Vec::new();
        record::encode(&[], &[(1, v)], &mut b).unwrap();
        b.len() - 1 // the label count
    };
    assert_eq!(size(Value::Int(0)), 2);
    assert_eq!(size(Value::Int(-100)), 3);
    assert_eq!(size(Value::Int(1 << 40)), 10);
    assert_eq!(size(Value::Float(0.5)), 6);
    assert_eq!(size(Value::Float(0.1)), 10);
    assert_eq!(size(Value::Bool(true)), 2);
}

/// A reserved tag (BR-GQL-002), and truncated or malformed bytes, are
/// format errors, never a panic.
#[test]
fn reserved_tags_and_malformed_records_are_rejected() {
    for tag in record::RESERVED {
        assert!(record::decode(&[0, tag << 3, 1, 0]).is_none(), "tag {tag}");
    }
    let mut b = Vec::new();
    record::encode(
        &[1, 2],
        &[(3, Value::string("abc")), (9, Value::Int(70_000))],
        &mut b,
    )
    .unwrap();
    for n in 0..b.len() {
        if n == 0 {
            continue;
        }
        // Every proper prefix that ends inside a field fails.
        let r = record::decode(&b[..n]);
        assert!(
            r.is_none() || r.as_ref().is_some_and(|(_, p)| p.len() < 2),
            "{n}"
        );
    }
    let mut rng = StdRng::seed_from_u64(seed());
    for _ in 0..20_000 {
        let n = rng.random_range(0..24);
        let junk: Vec<u8> = (0..n).map(|_| rng.random()).collect();
        let _ = record::decode(&junk);
    }
}

/// Values round-trip through a record committed in a tree and read back
/// after a reopen (FR-TYPE-001, FR-TYPE-002).
#[test]
fn records_round_trip_after_commit_and_reopen() {
    use crate::tx::Isolation;
    use crate::tx::tests::{CAP, tuning};
    let d = crate::test_support::graph_dir();
    let props: Vec<(u64, Value)> = every_type()
        .into_iter()
        .enumerate()
        .map(|(i, v)| (i as u64, v))
        .collect();
    let mut b = Vec::new();
    record::encode(&[4, 1], &props, &mut b).unwrap();
    let t;
    {
        let e = crate::storage::engine::Engine::create_tuned(
            d.path(),
            crate::test_support::no_flush(),
            CAP,
            Arc::new(crate::obs::recording::RecordingSink::default()),
            tuning(),
        )
        .unwrap();
        t = e.create_trees(1).unwrap()[0];
        let mut tx = e.begin(Isolation::Serializable).unwrap();
        tx.put(t, &7u64.to_be_bytes(), &b).unwrap();
        tx.commit().unwrap();
        e.close().unwrap();
    }
    let e = crate::tx::tests::reopen(d.path());
    let mut tx = e.begin(Isolation::Serializable).unwrap();
    let got = tx.get(t, &7u64.to_be_bytes()).unwrap().unwrap();
    tx.commit().unwrap();
    assert_eq!(record::decode(&got).unwrap(), (vec![1, 4], props));
    e.close().unwrap();
}

/// FR-TYPE-004: the Cartesian distance of (0, 0) and (3, 4) is 5; points
/// in different systems have none (null); WGS-84 distances equal constants
/// computed independently (Python `math`, from TS-21's formulas:
/// haversine with radius 6 378 140 m, and in 3D at the mean height,
/// combined with the height difference by Pythagoras).
#[test]
fn distances_follow_the_ts21_formulas() {
    let p = |s, c: &[f64]| Point::new(s, c).unwrap();
    let o = p(Srid::Cartesian2D, &[0.0, 0.0]);
    assert_eq!(distance(&o, &p(Srid::Cartesian2D, &[3.0, 4.0])), Some(5.0));
    assert_eq!(
        distance(
            &p(Srid::Cartesian3D, &[0.0, 0.0, 0.0]),
            &p(Srid::Cartesian3D, &[2.0, 3.0, 6.0])
        ),
        Some(7.0)
    );
    assert_eq!(distance(&o, &p(Srid::Wgs84_2D, &[0.0, 0.0])), None);
    assert_eq!(distance(&o, &p(Srid::Cartesian3D, &[0.0, 0.0, 0.0])), None);
    let close = |got: Option<f64>, want: f64| {
        let g = got.unwrap();
        assert!((g - want).abs() <= want * 1e-12, "{g} vs {want}");
    };
    let (lisbon, porto) = (
        p(Srid::Wgs84_2D, &[-9.1393, 38.7223]),
        p(Srid::Wgs84_2D, &[-8.6291, 41.1579]),
    );
    close(distance(&lisbon, &porto), 274_602.909_580_466_3);
    close(
        distance(
            &p(Srid::Wgs84_2D, &[0.0, 0.0]),
            &p(Srid::Wgs84_2D, &[180.0, 0.0]),
        ),
        20_037_517.767_567_202,
    );
    close(
        distance(
            &p(Srid::Wgs84_3D, &[-9.1393, 38.7223, 100.0]),
            &p(Srid::Wgs84_3D, &[-8.6291, 41.1579, 600.0]),
        ),
        274_618.433_573_905_2,
    );
    close(
        distance(
            &p(Srid::Wgs84_3D, &[151.2093, -33.8688, -50.0]),
            &p(Srid::Wgs84_3D, &[-0.1276, 51.5072, 8000.0]),
        ),
        17_023_582.192_560_054,
    );
}

/// A random value of a small domain per group, so that equal and adjacent
/// values are frequent.
fn random_value(rng: &mut StdRng, depth: u32) -> Value {
    let pick = rng.random_range(0..if depth > 1 { 17 } else { 20 });
    let small = |rng: &mut StdRng| rng.random_range(-3i64..=3);
    match pick {
        0 => Value::Null,
        1 => Value::Bool(rng.random()),
        2 => Value::Int(match rng.random_range(0..6) {
            0 => small(rng),
            1 => [i64::MIN, i64::MAX, i64::MIN + 1, i64::MAX - 1][rng.random_range(0..4)],
            2 => (1i64 << 53) + small(rng),
            3 => -(1i64 << 53) + small(rng),
            4 => rng.random(),
            _ => small(rng) << rng.random_range(0..62),
        }),
        3 => Value::Float(match rng.random_range(0..8) {
            0 => small(rng) as f64,
            1 => small(rng) as f64 + 0.5,
            2 => [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 0.0][rng.random_range(0..5)],
            3 => 9_223_372_036_854_775_808.0 * [1.0, -1.0, 2.0][rng.random_range(0..3)],
            4 => (1u64 << 53) as f64 * if rng.random() { 1.0 } else { -1.0 },
            5 => {
                f64::from_bits(rng.random::<u64>() & 0x000F_FFFF_FFFF_FFFF)
                    * if rng.random() { 1.0 } else { -1.0 }
            }
            6 => small(rng) as f64 * 1e-300,
            _ => rng.random::<f64>() * 1e10 - 5e9,
        }),
        4 => Value::Node(rng.random_range(0..4)),
        5 => Value::Edge(rng.random_range(0..4)),
        6 => Value::Date(
            Date::new(
                rng.random_range(-2..=2) * 3000 + 2000,
                rng.random_range(1..=2),
                rng.random_range(1..=2),
            )
            .unwrap(),
        ),
        7 => Value::LocalTime(
            Time::new(rng.random_range(0..3), 0, 0, rng.random_range(0..2)).unwrap(),
        ),
        8 => Value::LocalDateTime(DateTime::from_parts(
            Date::new(2026, 1, rng.random_range(1..=2)).unwrap(),
            Time::new(rng.random_range(0..2), 0, 0, rng.random_range(0..2)).unwrap(),
        )),
        9 => Value::ZonedTime(ZonedTime {
            time: Time::new(rng.random_range(0..3), 0, 0, 0).unwrap(),
            offset: rng.random_range(-1..=1) * 3600,
        }),
        10 => {
            // Instants with and without a fractional second on both sides
            // of the Unix epoch: jiff gives the fraction the instant's sign.
            let near_epoch = [
                "1969-12-31T23:59:58.5",
                "1969-12-31T23:59:59",
                "1969-12-31T23:59:59.5",
                "1970-01-01T00:00:00",
                "1970-01-01T00:00:00.5",
            ][rng.random_range(0..5)];
            match rng.random_range(0..6) {
                0 => offset_zoned("2026-03-28T01:00:00", rng.random_range(-1..=1) * 3600),
                1 => zoned("2026-03-28T01:00:00+00:00[UTC]"),
                2 => zoned("2026-03-28T01:00:00+00:00[Europe/Lisbon]"),
                3 => zoned("2026-03-28T01:00:00+00:00[Europe/London]"),
                4 => offset_zoned(near_epoch, rng.random_range(-1..=1) * 3600),
                _ => zoned(&format!("{near_epoch}+00:00[UTC]")),
            }
        }
        11 => Value::YearToMonth(small(rng)),
        12 => Value::DayToSecond(
            DayToSecond::new(small(rng) * 1_314_873, small(rng) * 500_000_000).unwrap(),
        ),
        13 => Value::string(["", "a", "a\0", "ab", "b", "é", "\0"][rng.random_range(0..7)]),
        14 => Value::Bytes(Arc::from(
            [&b""[..], b"\0", b"\0\0", b"\x01", b"\xff", b"a\0"][rng.random_range(0..6)],
        )),
        15 => Value::Path(Arc::from(
            [
                &[1u64][..],
                &[1, 5, 2],
                &[1, 5, 2, 6, 3],
                &[2, 5, 3],
                &[1, 4, 2],
            ][rng.random_range(0..5)],
        )),
        16 => {
            let srid =
                [Srid::Cartesian2D, Srid::Wgs84_2D, Srid::Cartesian3D][rng.random_range(0..3)];
            let c: Vec<f64> = (0..srid.dims())
                .map(|_| [0.0, -0.0, 1.0, -1.0, f64::NAN][rng.random_range(0..5)])
                .collect();
            point(srid, &c)
        }
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
            let names = ["a", "b", "c"];
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

fn enc(v: &Value) -> Vec<u8> {
    let mut b = Vec::new();
    key::encode(v, &mut b);
    b
}

/// The byte order of the key encoding equals the approved total order
/// ([`total_cmp`]) for 200 000 seeded pairs within and across groups, and
/// for 100 000 composite keys of two or three values (TS-21, BR-GQL-002).
/// The seed is printed; `LPG_SEED` overrides it.
#[test]
fn key_bytes_sort_in_the_approved_order() {
    let mut rng = StdRng::seed_from_u64(seed());
    let mut same_group = 0;
    for i in 0..200_000 {
        let a = random_value(&mut rng, 0);
        let b = if i % 2 == 0 {
            // Same group more often.
            let mut b = random_value(&mut rng, 0);
            for _ in 0..64 {
                if b.group() == a.group() {
                    break;
                }
                b = random_value(&mut rng, 0);
            }
            b
        } else {
            random_value(&mut rng, 0)
        };
        same_group += usize::from(a.group() == b.group());
        let want = total_cmp(&a, &b);
        assert_eq!(total_cmp(&b, &a), want.reverse(), "{a:?} {b:?}");
        assert_eq!(enc(&a).cmp(&enc(&b)), want, "{a:?} vs {b:?}");
    }
    assert!(same_group > 80_000, "{same_group} pairs in one group");
    for _ in 0..100_000 {
        let n = rng.random_range(2..=3);
        let a: Vec<Value> = (0..n).map(|_| random_value(&mut rng, 1)).collect();
        let b: Vec<Value> = a
            .iter()
            .map(|v| {
                if rng.random_range(0..3) == 0 {
                    random_value(&mut rng, 1)
                } else {
                    v.clone()
                }
            })
            .collect();
        let want = a
            .iter()
            .zip(&b)
            .map(|(x, y)| total_cmp(x, y))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal);
        assert_eq!(
            key::encode_all(&a).cmp(&key::encode_all(&b)),
            want,
            "{a:?} vs {b:?}"
        );
    }
}

/// The approved order's fixed points (TS-21): groups in Neo4j's order with
/// NULL last; numbers by exact value across integers and floats; NaN above
/// every number; −0.0 equal to +0.0; strings by code point without
/// padding; trailing X'00' significant in byte strings; zoned values with
/// the same instant ordered by offset, offset-only before named.
#[test]
fn the_order_has_the_approved_fixed_points() {
    use Ordering::*;
    let c = |a: &Value, b: &Value| {
        let o = total_cmp(a, b);
        assert_eq!(enc(a).cmp(&enc(b)), o, "{a:?} {b:?}");
        o
    };
    let groups = [
        Value::Record(Arc::from(vec![])),
        Value::Node(9),
        Value::Edge(0),
        Value::List(Arc::from(vec![])),
        Value::Path(Arc::from(vec![0u64])),
        point(Srid::Cartesian2D, &[0.0, 0.0]),
        zoned("2026-01-01T00:00:00+00:00[UTC]"),
        Value::LocalDateTime("2026-01-01T00:00".parse().unwrap()),
        Value::Date(Date::new(2026, 1, 1).unwrap()),
        Value::ZonedTime(ZonedTime {
            time: Time::midnight(),
            offset: 0,
        }),
        Value::LocalTime(Time::midnight()),
        Value::YearToMonth(0),
        Value::string(""),
        Value::Bytes(Arc::from(&[][..])),
        Value::Bool(false),
        Value::Float(f64::NEG_INFINITY),
        Value::Null,
    ];
    for w in groups.windows(2) {
        assert_eq!(c(&w[0], &w[1]), Less, "{:?} < {:?}", w[0], w[1]);
    }
    assert_eq!(c(&Value::Int(3), &Value::Float(3.0)), Equal);
    assert_eq!(enc(&Value::Int(3)), enc(&Value::Float(3.0)));
    assert_eq!(c(&Value::Float(-0.0), &Value::Float(0.0)), Equal);
    assert_eq!(c(&Value::Int(0), &Value::Float(-0.0)), Equal);
    assert_eq!(
        c(&Value::Float(f64::INFINITY), &Value::Float(f64::NAN)),
        Less
    );
    assert_eq!(
        c(
            &Value::Int(i64::MAX),
            &Value::Float(9_223_372_036_854_775_808.0)
        ),
        Less
    );
    assert_eq!(
        c(
            &Value::Int((1 << 53) + 1),
            &Value::Float((1u64 << 53) as f64)
        ),
        Greater
    );
    assert_eq!(
        c(
            &Value::Int(-(1 << 53) - 1),
            &Value::Float(-((1u64 << 53) as f64))
        ),
        Less
    );
    assert_eq!(c(&Value::Int(2), &Value::Float(2.5)), Less);
    assert_eq!(c(&Value::Int(-3), &Value::Float(-2.5)), Less);
    assert_eq!(c(&Value::string("a"), &Value::string("a ")), Less);
    assert_eq!(c(&Value::string("z"), &Value::string("é")), Less);
    let bytes = |b: &[u8]| Value::Bytes(Arc::from(b));
    assert_eq!(c(&bytes(b"a"), &bytes(b"a\0")), Less);
    assert_eq!(c(&bytes(b"a\0"), &bytes(b"a\x01")), Less);
    assert_eq!(
        c(
            &offset_zoned("2026-01-01T01:00:00", 3600),
            &zoned("2026-01-01T00:00:00+00:00[UTC]")
        ),
        Greater
    );
    assert_eq!(
        c(
            &offset_zoned("2026-01-01T00:00:00", 0),
            &zoned("2026-01-01T00:00:00+00:00[UTC]")
        ),
        Less,
        "offset-only before named"
    );
    // Durations by average length: 1 month (2 629 746 s) is above 30 days.
    assert_eq!(
        c(
            &Value::YearToMonth(1),
            &Value::DayToSecond(DayToSecond::new(30 * 86_400, 0).unwrap())
        ),
        Greater
    );
    assert_eq!(
        c(
            &Value::YearToMonth(1),
            &Value::DayToSecond(DayToSecond::new(2_629_746, 0).unwrap())
        ),
        Greater
    );
}

/// TS-21 normalises zone names through the IANA links before ordering:
/// two names of one zone compare equal.
#[test]
fn zone_names_are_normalised_through_iana_links() {
    let n = temporal::normalize_zone;
    assert_eq!(n("Europe/Lisbon"), "Europe/Lisbon");
    // A link of tzdata 2026e (`Link Europe/Rome Europe/Vatican`) that the
    // host also installs as a zone file.
    assert_eq!(n("Europe/Vatican"), "Europe/Rome");
    let a = zoned("2026-01-01T01:00:00+01:00[Europe/Vatican]");
    let b = zoned("2026-01-01T01:00:00+01:00[Europe/Rome]");
    assert_eq!(total_cmp(&a, &b), Ordering::Equal);
    assert_eq!(enc(&a), enc(&b));
}

/// TS-21: the links are the crate's fixed table of the format version,
/// not the host's. IANA's default build links `Europe/Oslo` to
/// `Europe/Berlin`; Debian's `tzdata.zi`, built with `backzone`, has it as
/// a zone (`Z Europe/Oslo`), so a host-read table returned `Europe/Oslo`
/// here. The table is sorted for binary search and has no chain.
#[test]
fn zone_links_are_the_formats_fixed_table_not_the_hosts() {
    let n = temporal::normalize_zone;
    assert_eq!(n("Europe/Oslo"), "Europe/Berlin");
    assert_eq!(n("US/Eastern"), "America/New_York");
    assert_eq!(n("GMT"), "Etc/GMT");
    assert_eq!(n("No/Such_Zone"), "No/Such_Zone");
    let links = super::zone_links::LINKS;
    assert_eq!(links.len(), 253);
    assert!(
        links
            .windows(2)
            .all(|w| w[0].0.as_bytes() < w[1].0.as_bytes())
    );
    assert!(links.iter().all(|(_, t)| n(t) == *t));
}

/// TS-21: a stored named zone keeps its name and offset when the host
/// lacks the zone, or when the host's rules give another offset at the
/// instant. The record decodes, formats and re-encodes to the same bytes,
/// and its key is the original's with the name changed.
#[test]
fn a_stored_zone_the_host_cannot_apply_keeps_its_name_and_offset() {
    let orig = zoned("2026-07-01T12:00:00+01:00[Europe/Lisbon]");
    let mut b = Vec::new();
    record::encode(&[], &[(1, orig.clone())], &mut b).unwrap();
    // Missing zone: the name patched to one no host has.
    let at = b
        .windows(13)
        .position(|w| w == b"Europe/Lisbon")
        .expect("the record holds the zone name");
    let mut missing = b.clone();
    missing[at..at + 13].copy_from_slice(b"Europe/Lisbox");
    // Other rules: the stored offset, the record's last field (zigzag
    // varint), patched from +01:00 to +00:30.
    let varint = |v: u64| {
        let mut o = Vec::new();
        crate::storage::btree::node::put_varint(&mut o, v);
        o
    };
    let (plus_one, plus_half) = (varint(7200), varint(3600));
    assert!(b.ends_with(&plus_one));
    let mut other = b[..b.len() - plus_one.len()].to_vec();
    other.extend_from_slice(&plus_half);
    for (bytes, name, offset) in [
        (&missing, "Europe/Lisbox", 3600),
        (&other, "Europe/Lisbon", 1800),
    ] {
        let (_, p) = record::decode(bytes).expect("the record decodes");
        let Value::ZonedDateTime(z) = &p[0].1 else {
            panic!("{p:?}");
        };
        assert_eq!(z.zone_name(), Some(name));
        assert_eq!(z.zoned.offset().seconds(), offset);
        assert_eq!(
            z.zoned.timestamp(),
            "2026-07-01T11:00:00Z".parse::<Timestamp>().unwrap()
        );
        let mut again = Vec::new();
        record::encode(&[], &p, &mut again).unwrap();
        assert_eq!(&again, bytes);
        let local = if offset == 3600 {
            "2026-07-01T12:00:00+01:00"
        } else {
            "2026-07-01T11:30:00+00:30"
        };
        assert_eq!(
            temporal::format(&p[0].1).unwrap(),
            format!("{local}[{name}]")
        );
    }
    let (_, p) = record::decode(&missing).unwrap();
    let k = enc(&p[0].1);
    let mut want = enc(&orig);
    let at = want
        .windows(13)
        .position(|w| w == b"Europe/Lisbon")
        .unwrap();
    want[at..at + 13].copy_from_slice(b"Europe/Lisbox");
    assert_eq!(k, want);
    // The host's own zone and offset: the value uses the zone.
    let (_, p) = record::decode(&b).unwrap();
    assert_eq!(p[0].1, orig);
}

/// The Hilbert encoding is a bijection on small grids in 2D and 3D, and a
/// Hilbert curve: consecutive indices are neighbouring cells. Random
/// cells round-trip at the production widths, 30 and 20 bits per axis.
#[test]
fn hilbert_keys_are_a_bijection_along_a_continuous_curve() {
    for (dims, bits) in [(2usize, 4u32), (3, 3), (2, 1), (3, 1)] {
        let n = 1u64 << (dims as u32 * bits);
        let mut seen = vec![false; n as usize];
        let mut prev: Option<Vec<u32>> = None;
        for h in 0..n {
            let c = spatial::decode(h, dims, bits);
            assert!(c.iter().all(|&x| x < 1 << bits));
            assert_eq!(spatial::encode(&c, bits), h);
            let flat = c.iter().fold(0u64, |acc, &x| (acc << bits) | u64::from(x));
            assert!(!seen[flat as usize], "{c:?} twice");
            seen[flat as usize] = true;
            if let Some(p) = &prev {
                let step: u32 = p.iter().zip(&c).map(|(a, b)| a.abs_diff(*b)).sum();
                assert_eq!(step, 1, "{p:?} -> {c:?} at {h}");
            }
            prev = Some(c);
        }
        assert!(seen.iter().all(|&s| s));
    }
    let mut rng = StdRng::seed_from_u64(seed());
    for (dims, bits) in [(2usize, 30u32), (3, 20)] {
        for _ in 0..10_000 {
            let c: Vec<u32> = (0..dims).map(|_| rng.random_range(0..1 << bits)).collect();
            let h = spatial::encode(&c, bits);
            assert!(h < 1 << 60);
            assert_eq!(spatial::decode(h, dims, bits), c);
        }
    }
}

/// For seeded boxes, the decomposed key ranges contain the key of every
/// grid cell inside the box (checked by brute force), with an ample and
/// a tight budget of ranges; the ranges are sorted and disjoint.
#[test]
fn box_ranges_cover_every_cell_of_the_box() {
    let mut rng = StdRng::seed_from_u64(seed());
    for (dims, bits) in [(2usize, 5u32), (3, 4)] {
        for trial in 0..300 {
            let side = 1u32 << bits;
            let (mut lo, mut hi) = (Vec::new(), Vec::new());
            for _ in 0..dims {
                let a = rng.random_range(0..side);
                let b = rng.random_range(0..side);
                lo.push(a.min(b));
                hi.push(a.max(b));
            }
            let budget = if trial % 2 == 0 { 64 } else { 4 };
            let r = spatial::ranges(&lo, &hi, bits, budget);
            assert!(r.windows(2).all(|w| w[0].1 < w[1].0), "{r:?}");
            let total = 1u64 << (dims as u32 * bits);
            let mut covered = 0u64;
            for h in 0..total {
                let c = spatial::decode(h, dims, bits);
                let inside = (0..dims).all(|i| lo[i] <= c[i] && c[i] <= hi[i]);
                let hit = r.iter().any(|&(a, b)| a <= h && h <= b);
                if inside {
                    assert!(hit, "cell {c:?} key {h} outside {r:?}");
                }
                covered += u64::from(hit);
            }
            let cells: u64 = (0..dims).map(|i| u64::from(hi[i] - lo[i] + 1)).product();
            assert!(covered >= cells);
        }
    }
    // Points: a box between two WGS-84 points holds the key of a point
    // inside it; coordinates outside the extents clamp to the edge.
    let p = |c: &[f64]| Point::new(Srid::Wgs84_2D, c).unwrap();
    let r = spatial::box_ranges(&p(&[-10.0, 38.0]), &p(&[-8.0, 42.0])).unwrap();
    let k = spatial::key(&p(&[-9.1393, 38.7223]));
    assert!(r.iter().any(|&(a, b)| a <= k && k <= b));
    assert_eq!(spatial::cells(&p(&[500.0, -100.0])), vec![(1 << 30) - 1, 0]);
}

/// FR-TYPE-007: the offset `jiff` gives for a zone, from the operating
/// system's zone data, equals the output of `TZ=<zone> date +%z` at the
/// same instant.
#[test]
fn zone_offsets_equal_the_operating_systems() {
    let instants = [
        0i64,
        1_711_846_800,
        1_727_000_000,
        1_790_000_000,
        -1_000_000_000,
    ];
    for name in [
        "Europe/Lisbon",
        "America/New_York",
        "Asia/Kolkata",
        "Australia/Lord_Howe",
        "UTC",
    ] {
        let tz = temporal::zone(name).unwrap();
        for &t in &instants {
            let off = tz.to_offset(Timestamp::from_second(t).unwrap()).seconds();
            let out = std::process::Command::new("date")
                .env("TZ", name)
                .args(["-d", &format!("@{t}"), "+%z"])
                .output()
                .unwrap();
            let s = String::from_utf8(out.stdout).unwrap();
            let s = s.trim();
            let sign = if s.starts_with('-') { -1 } else { 1 };
            let want = sign
                * (s[1..3].parse::<i32>().unwrap() * 3600 + s[3..5].parse::<i32>().unwrap() * 60);
            assert_eq!(off, want, "{name} at {t}: {s}");
        }
    }
    assert!(temporal::zone("No/Such_Zone").is_none());
}

/// FR-TYPE-006: the current timestamp comes from the system clock: it lies
/// between two `SystemTime::now()` reads around it.
#[test]
fn the_current_time_is_the_system_clock() {
    let ns = |t: std::time::SystemTime| {
        t.duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i128
    };
    let before = ns(std::time::SystemTime::now());
    let now = temporal::now().as_nanosecond();
    let after = ns(std::time::SystemTime::now());
    assert!(before <= now && now <= after, "{before} {now} {after}");
}

/// Lexical forms (TS-21): ISO 8601 temporal text with a named zone in
/// brackets, ISO 8601 durations, `{srid, x, y[, z]}` points; each parses
/// back to the same value.
#[test]
fn lexical_forms_round_trip() {
    let cases = [
        (Value::Date(Date::new(2026, 10, 8).unwrap()), "2026-10-08"),
        (
            Value::LocalTime(Time::new(1, 2, 3, 400_000_000).unwrap()),
            "01:02:03.4",
        ),
        (
            Value::LocalDateTime("2026-10-08T01:02:03".parse().unwrap()),
            "2026-10-08T01:02:03",
        ),
        (
            Value::ZonedTime(ZonedTime {
                time: Time::new(1, 2, 3, 0).unwrap(),
                offset: -5400,
            }),
            "01:02:03-01:30",
        ),
        (
            zoned("2026-07-01T12:00:00+01:00[Europe/Lisbon]"),
            "2026-07-01T12:00:00+01:00[Europe/Lisbon]",
        ),
        (
            offset_zoned("2026-07-01T12:00:00", 19_800),
            "2026-07-01T12:00:00+05:30",
        ),
        (
            offset_zoned("2026-07-01T12:00:00", 0),
            "2026-07-01T12:00:00+00:00",
        ),
        (
            zoned("2026-07-01T12:00:00+00:00[UTC]"),
            "2026-07-01T12:00:00+00:00[UTC]",
        ),
        (Value::YearToMonth(-27), "-P2Y3M"),
        (
            Value::DayToSecond(DayToSecond::new(90_061, 500_000_000).unwrap()),
            "P1DT1H1M1.5S",
        ),
        (
            Value::DayToSecond(DayToSecond::new(-1, 0).unwrap()),
            "-P0DT0H0M1S",
        ),
    ];
    for (v, text) in cases {
        assert_eq!(temporal::format(&v).as_deref(), Some(text));
        assert_eq!(temporal::parse(&v, text), Some(v.clone()), "{text}");
    }
    for p in [
        Point::new(Srid::Cartesian2D, &[1.5, -2.0]).unwrap(),
        Point::new(Srid::Wgs84_3D, &[-9.1393, 38.7223, 0.25]).unwrap(),
    ] {
        let text = p.lexical();
        assert_eq!(Point::parse(&text), Some(p), "{text}");
    }
    assert_eq!(
        Point::new(Srid::Cartesian2D, &[1.5, -2.0])
            .unwrap()
            .lexical(),
        "{srid: 7203, x: 1.5, y: -2.0}"
    );
    assert_eq!(Point::parse("{srid: 4326, x: 1, y: 2, z: 3}"), None);
}

/// Bit-level equality: floats and point coordinates by their bits, zoned
/// date-times by instant, offset and zone name.
fn bits_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Point(x), Value::Point(y)) => {
            x.srid() == y.srid()
                && x.coords()
                    .iter()
                    .zip(y.coords())
                    .all(|(p, q)| p.to_bits() == q.to_bits())
        }
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| bits_eq(p, q))
        }
        (Value::ZonedDateTime(x), Value::ZonedDateTime(y)) => {
            x.zoned.timestamp() == y.zoned.timestamp()
                && x.zoned.offset() == y.zoned.offset()
                && x.zone_name() == y.zone_name()
        }
        _ => a == b,
    }
}

/// A value with extreme fields, in and out of the record domain.
fn extreme_value(rng: &mut StdRng, depth: u32) -> Value {
    let floats = [
        0.0,
        -0.0,
        f64::NAN,
        f64::from_bits(0xFFF8_0000_0000_0123),
        f64::INFINITY,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
        16_777_217.0,
    ];
    let ints = [i64::MIN, i64::MAX, 0, -1, 127, 128, -129, 1 << 53];
    let offsets = [
        -93_599,
        93_599,
        93_600,
        -93_600,
        26 * 3600,
        i32::MIN,
        i32::MAX,
        0,
        19_800,
    ];
    let nanos = [0, 1, 999_999_999, 1_000_000_000, u32::MAX];
    let pick = |rng: &mut StdRng, n: usize| rng.random_range(0..n);
    match rng.random_range(0..9) {
        0 => Value::Int(ints[pick(rng, ints.len())]),
        1 => Value::Float(floats[pick(rng, floats.len())]),
        2 => Value::ZonedTime(ZonedTime {
            time: Time::new(rng.random_range(0..24), 59, 59, 999_999_999).unwrap(),
            offset: offsets[pick(rng, offsets.len())],
        }),
        3 => {
            let s = [Timestamp::MIN, Timestamp::MAX, Timestamp::UNIX_EPOCH][pick(rng, 3)];
            let off = Offset::from_seconds([-93_599, 93_599, 0][pick(rng, 3)]).unwrap();
            Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(s, off)))
        }
        4 => Value::YearToMonth(ints[pick(rng, ints.len())]),
        5 => Value::DayToSecond(DayToSecond {
            seconds: ints[pick(rng, ints.len())],
            nanos: nanos[pick(rng, nanos.len())],
        }),
        6 => {
            let srid = [Srid::Cartesian2D, Srid::Wgs84_3D][pick(rng, 2)];
            let c: Vec<f64> = (0..srid.dims())
                .map(|_| floats[pick(rng, floats.len())])
                .collect();
            point(srid, &c)
        }
        7 if depth < 3 => Value::List(
            (0..rng.random_range(0..3))
                .map(|_| extreme_value(rng, depth + 1))
                .collect::<Vec<_>>()
                .into(),
        ),
        _ => Value::Node([0, u64::MAX][pick(rng, 2)]),
    }
}

/// Whether `v` is inside the record domain: zoned-time offsets within
/// ±93 599 s and day-to-second nanoseconds in `0..1e9`.
fn in_domain(v: &Value) -> bool {
    match v {
        Value::ZonedTime(z) => z.offset.unsigned_abs() <= 93_599,
        Value::DayToSecond(d) => d.nanos < 1_000_000_000,
        Value::List(items) => items.iter().all(in_domain),
        _ => true,
    }
}

/// The record encoder and decoder accept the same domain (audit L1): every
/// value the encoder accepts decodes back bit-identically and re-encodes to
/// the same bytes, and the encoder rejects exactly the values outside the
/// domain. The seed is printed; `LPG_SEED` overrides it.
#[test]
fn the_record_encoder_accepts_exactly_what_the_decoder_accepts() {
    let mut rng = StdRng::seed_from_u64(seed());
    for _ in 0..20_000 {
        let v = extreme_value(&mut rng, 0);
        let mut b = Vec::new();
        let encoded = record::encode(&[], &[(1, v.clone())], &mut b);
        assert_eq!(encoded.is_ok(), in_domain(&v), "{v:?}");
        if encoded.is_err() {
            assert_eq!(encoded, Err(record::TooLarge::Range), "{v:?}");
            continue;
        }
        let (_, p) = record::decode(&b).unwrap_or_else(|| panic!("{v:?} does not decode"));
        assert!(p.len() == 1 && bits_eq(&p[0].1, &v), "{v:?} -> {p:?}");
        let mut again = Vec::new();
        record::encode(&[], &p, &mut again).unwrap();
        assert_eq!(again, b, "{v:?}");
    }
}

/// A parsed zoned-time offset outside ±93 599 s is rejected, not stored as
/// an undecodable record (audit L1).
#[test]
fn an_out_of_range_parsed_offset_is_rejected() {
    let dt = offset_zoned("2026-01-01T00:00:00", 0);
    let like = Value::ZonedTime(ZonedTime {
        time: Time::MIN,
        offset: 0,
    });
    for text in [
        "10:00:00+99:00",
        "10:00:00-26:00",
        "10:00:00+25:59:60",
        "10:00:00+2147483647:00",
        "10:00:00+05:99",
        "10:00:00+05:60",
        "10:00:00+05:00:60",
        "10:00:00-05:00:99",
        "10:00:00+26:00",
        "10:00:00+5:00",
        "10:00:00+05:+3",
    ] {
        assert_eq!(temporal::parse(&like, text), None, "{text}");
    }
    assert_eq!(
        temporal::parse(&like, "10:00:00-05:59:59"),
        Some(Value::ZonedTime(ZonedTime {
            time: Time::new(10, 0, 0, 0).unwrap(),
            offset: -21_599
        }))
    );
    assert_eq!(temporal::parse(&dt, "2026-01-01T00:00:00+05:60"), None);
    let max = temporal::parse(&like, "10:00:00+25:59:59").unwrap();
    let mut b = Vec::new();
    record::encode(&[], &[(1, max.clone())], &mut b).unwrap();
    assert_eq!(record::decode(&b), Some((vec![], vec![(1, max)])));
    assert_eq!(temporal::parse(&dt, "2026-01-01T00:00:00+99:00"), None);
}

/// Duration construction and parsing report overflow instead of wrapping
/// or panicking (audit L2).
#[test]
fn duration_overflow_is_rejected() {
    assert_eq!(DayToSecond::new(i64::MIN, -1), None);
    assert_eq!(
        DayToSecond::new(i64::MIN, 0),
        Some(DayToSecond {
            seconds: i64::MIN,
            nanos: 0
        })
    );
    assert_eq!(
        DayToSecond::new(i64::MAX, 999_999_999),
        Some(DayToSecond {
            seconds: i64::MAX,
            nanos: 999_999_999
        })
    );
    assert_eq!(DayToSecond::new(i64::MAX, 1_000_000_000), None);
    let ds = Value::DayToSecond(DayToSecond::new(0, 0).unwrap());
    let ym = Value::YearToMonth(0);
    for text in [
        "P106751991167301DT0H0M0S",
        "P0DT2562047788015216H0M0S",
        "P0DT0H153722867280912931M0S",
        "P0DT0H1M9223372036854775807S",
        "-P-106751991167300DT-15H-30M-8S",
    ] {
        assert_eq!(temporal::parse(&ds, text), None, "{text}");
    }
    assert_eq!(
        temporal::parse(&ds, "P0DT0H0M9223372036854775807.999999999S"),
        Some(Value::DayToSecond(DayToSecond {
            seconds: i64::MAX,
            nanos: 999_999_999
        }))
    );
    for text in ["P768614336404564651Y0M", "-P-768614336404564650Y-8M"] {
        assert_eq!(temporal::parse(&ym, text), None, "{text}");
    }
}
