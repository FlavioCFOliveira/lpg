use rand::SeedableRng;
use rand::rngs::StdRng;

use super::*;
use crate::test_support::seed;
use crate::test_support::values::{offset_zoned, point, random_value, zoned};
use crate::types::{DayToSecond, Srid, ZonedTime};
use jiff::civil::{Date, DateTime, Time};

/// Node 12 with label `A` and property `name`, edge 7 of type `R` from 12
/// to 13, and edge 5 of type `R` from 1 to 2: the contents of the TS-21
/// node, edge and path examples. Edge 9 of type `R` from 12 to 13 has the
/// properties `s` and `w`.
struct Example;

impl Contents for Example {
    fn node(&self, id: u64) -> (Vec<String>, Vec<(String, Value)>) {
        if id == 12 {
            (vec!["A".into()], vec![("name".into(), Value::string("x"))])
        } else {
            (Vec::new(), Vec::new())
        }
    }

    fn edge(&self, id: u64) -> (String, u64, u64, Vec<(String, Value)>) {
        match id {
            7 => ("R".into(), 12, 13, Vec::new()),
            5 => ("R".into(), 1, 2, Vec::new()),
            9 => (
                "R".into(),
                12,
                13,
                vec![
                    ("s".into(), Value::string("a")),
                    ("w".into(), Value::Float(1.5)),
                ],
            ),
            _ => (String::new(), 0, 0, Vec::new()),
        }
    }
}

/// One example row of the TS-21 table: `v` is written `text` with type
/// name `t`, and `text` reads back as `v`.
fn row(v: &Value, t: &str, text: &str) {
    assert_eq!(type_name(v), t, "type name of {v:?}");
    assert_eq!(lexical_with(v, &Example), text, "lexical form of {v:?}");
    let back = parse(&TypeName::parse(t).unwrap(), text).unwrap_or_else(|| panic!("{text}"));
    assert!(bit_eq(&back, v), "{text}: {back:?} != {v:?}");
}

#[test]
fn ts21_null() {
    row(&Value::Null, "NULL", "NULL");
}

#[test]
fn ts21_boolean() {
    row(&Value::Bool(false), "BOOLEAN", "FALSE");
    // Read case-insensitively.
    assert_eq!(parse(&TypeName::Boolean, "tRuE"), Some(Value::Bool(true)));
}

#[test]
fn ts21_integer() {
    row(&Value::Int(i64::MIN), "INTEGER", "-9223372036854775808");
    assert_eq!(parse(&TypeName::Integer, "+5"), Some(Value::Int(5)));
    assert_eq!(parse(&TypeName::Integer, "9223372036854775808"), None);
}

#[test]
fn ts21_float() {
    row(&Value::Float(-0.0), "FLOAT", "-0.0");
    row(&Value::Float(1.25e-7), "FLOAT", "1.25e-7");
    row(&Value::Float(f64::NAN), "FLOAT", "NaN");
    // The measured forms of TS-21: `e` from 10^16, `5e-324`.
    row(&Value::Float(1e16), "FLOAT", "1e16");
    row(&Value::Float(5e-324), "FLOAT", "5e-324");
    row(&Value::Float(f64::INFINITY), "FLOAT", "Infinity");
    row(&Value::Float(f64::NEG_INFINITY), "FLOAT", "-Infinity");
    // A text without `.` or `e` is not a float; neither are Rust's other
    // spellings of the non-finite values.
    for bad in ["1", "inf", "infinity", "nan", "1e999", "", "e"] {
        assert_eq!(parse(&TypeName::Float, bad), None, "{bad}");
    }
}

#[test]
fn ts21_string() {
    row(&Value::string("it's\r"), "STRING", "'it\\'s\\u000D'");
    // Every GQL escape and the doubled quote are read.
    assert_eq!(
        parse(&TypeName::String, r"'a''b\\\'\`\t\b\n\r\fé\U01D11E'"),
        Some(Value::string("a'b\\'`\t\u{8}\n\r\u{c}é𝄞"))
    );
    // U+FFFE and U+FFFF are written escaped.
    row(
        &Value::string("\u{FFFE}\u{FFFF}"),
        "STRING",
        "'\\uFFFE\\uFFFF'",
    );
    // An escape that names no scalar value, a raw CR and an unknown escape
    // fail; the GQL parser's U+FFFD substitution is not applied.
    for bad in [r"'\uD800'", "'a\rb'", r"'\q'", "'open", "'a'b"] {
        assert_eq!(parse(&TypeName::String, bad), None, "{bad}");
    }
}

#[test]
fn ts21_bytes() {
    row(
        &Value::Bytes(Arc::from(&[0u8, 255][..])),
        "BYTES",
        "X'00FF'",
    );
    // The spaces `BYTE_STRING_LITERAL` allows.
    assert_eq!(
        parse(&TypeName::Bytes, "X' 0 0 F F '"),
        Some(Value::Bytes(Arc::from(&[0u8, 255][..])))
    );
    assert_eq!(parse(&TypeName::Bytes, "X'0'"), None);
}

#[test]
fn ts21_date() {
    let d = Value::Date(Date::new(2015, 7, 21).unwrap());
    row(&d, "DATE", "2015-07-21");
    for alt in ["2015-W30-2", "2015-202", "20150721"] {
        assert_eq!(parse(&TypeName::Date, alt), Some(d.clone()), "{alt}");
    }
}

#[test]
fn ts21_local_time() {
    row(
        &Value::LocalTime(Time::new(21, 40, 32, 142_000_000).unwrap()),
        "LOCAL TIME",
        "21:40:32.142",
    );
}

#[test]
fn ts21_zoned_time() {
    row(
        &Value::ZonedTime(ZonedTime {
            time: Time::new(21, 40, 32, 0).unwrap(),
            offset: 3600,
        }),
        "ZONED TIME",
        "21:40:32+01:00",
    );
}

#[test]
fn ts21_local_datetime() {
    row(
        &Value::LocalDateTime(DateTime::from_parts(
            Date::new(2015, 7, 21).unwrap(),
            Time::new(21, 40, 32, 0).unwrap(),
        )),
        "LOCAL DATETIME",
        "2015-07-21T21:40:32",
    );
}

#[test]
fn ts21_zoned_datetime() {
    row(
        &zoned("2015-07-21T21:40:32+02:00[Europe/Berlin]"),
        "ZONED DATETIME",
        "2015-07-21T21:40:32+02:00[Europe/Berlin]",
    );
    row(
        &offset_zoned("2015-07-21T21:40:32", 3600),
        "ZONED DATETIME",
        "2015-07-21T21:40:32+01:00",
    );
}

#[test]
fn ts21_durations() {
    row(
        &Value::YearToMonth(-14),
        "DURATION(YEAR TO MONTH)",
        "-P1Y2M",
    );
    row(
        &Value::DayToSecond(DayToSecond::new(93_784, 500_000_000).unwrap()),
        "DURATION(DAY TO SECOND)",
        "P1DT2H3M4.5S",
    );
}

#[test]
fn ts21_point() {
    row(
        &point(Srid::Wgs84_3D, &[1.0, 2.0, 3.0]),
        "POINT",
        "{srid: 4979, x: 1.0, y: 2.0, z: 3.0}",
    );
    row(
        &point(Srid::Cartesian2D, &[f64::NAN, f64::NEG_INFINITY]),
        "POINT",
        "{srid: 7203, x: NaN, y: -Infinity}",
    );
    for bad in [
        "{srid: 4326, x: 1.0, y: 2.0, z: 3.0}",
        "{srid: 7203, x: 1.0, x: 2.0, y: 1.0}",
        "{srid: 1, x: 1.0, y: 2.0}",
    ] {
        assert_eq!(parse(&TypeName::Point, bad), None, "{bad}");
    }
}

#[test]
fn ts21_list() {
    let v = Value::List(
        vec![
            Value::Int(1),
            Value::string("a"),
            Value::Date(Date::new(2015, 7, 21).unwrap()),
            Value::List(vec![Value::Float(2.5), Value::Null].into()),
        ]
        .into(),
    );
    row(&v, "LIST<ANY>", "[1, 'a', DATE '2015-07-21', [2.5, NULL]]");
    row(
        &Value::List(vec![Value::Int(1), Value::Int(2)].into()),
        "LIST<INTEGER>",
        "[1, 2]",
    );
    row(&Value::List(Vec::new().into()), "LIST<ANY>", "[]");
    // `LIST<T>` reads only a non-empty list of T.
    let ints = TypeName::parse("LIST<INTEGER>").unwrap();
    assert_eq!(parse(&ints, "[1, 'a']"), None);
    assert_eq!(parse(&ints, "[]"), None);
    assert_eq!(TypeName::parse("LIST<NULL>"), None);
    // Nesting beyond the bound is refused.
    let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert_eq!(parse(&TypeName::List(None), &deep), None);
}

#[test]
fn ts21_record() {
    let v = Value::Record(
        vec![
            (Arc::from("a"), Value::Int(1)),
            (Arc::from("b c"), Value::string("x")),
        ]
        .into(),
    );
    row(&v, "RECORD", "RECORD {a: 1, `b c`: 'x'}");
    assert_eq!(parse(&TypeName::Record, "RECORD {a: 1, a: 2}"), None);
}

#[test]
fn ts21_node() {
    row(
        &Value::Node(12),
        "NODE",
        "RECORD {id: 12, labels: ['A'], properties: RECORD {name: 'x'}}",
    );
}

#[test]
fn ts21_edge() {
    row(
        &Value::Edge(7),
        "RELATIONSHIP",
        "RECORD {id: 7, type: 'R', start: 12, end: 13, properties: RECORD {}}",
    );
}

/// rmp #56: the edge record form holds the edge's properties, and the
/// text reads back to the edge, bit-equal.
#[test]
fn an_edge_record_form_holds_its_properties() {
    row(
        &Value::Edge(9),
        "RELATIONSHIP",
        "RECORD {id: 9, type: 'R', start: 12, end: 13, properties: RECORD {s: 'a', w: 1.5}}",
    );
}

#[test]
fn ts21_path() {
    row(
        &Value::Path(vec![1, 5, 2].into()),
        "PATH",
        "[NODE 'RECORD {id: 1, labels: [], properties: RECORD {}}', \
         RELATIONSHIP 'RECORD {id: 5, type: \\'R\\', start: 1, end: 2, properties: RECORD {}}', \
         NODE 'RECORD {id: 2, labels: [], properties: RECORD {}}']",
    );
    assert_eq!(parse(&TypeName::Path, "[]"), None);
}

#[test]
fn type_names_are_case_sensitive_and_annotations_split_at_the_first_colon() {
    assert_eq!(TypeName::parse("integer"), None);
    assert_eq!(
        TypeName::parse("LIST<LIST<INTEGER>>").unwrap().to_string(),
        "LIST<LIST<INTEGER>>"
    );
    let v = offset_zoned("2015-07-21T21:40:32", 3600);
    let a = annotated(&v, &NoContents);
    assert_eq!(a, "ZONED DATETIME:2015-07-21T21:40:32+01:00");
    assert!(bit_eq(&parse_annotated(&a).unwrap(), &v));
}

/// rmp #56 acceptance: for 100 000 generated values (a fixed seed, then
/// `LPG_SEED` or a random one), `parse(type_name(v), lexical(v))` equals
/// `v` under bit equality (NaN for NaN).
#[test]
fn every_generated_value_round_trips_through_its_lexical_form() {
    for s in [1, seed()] {
        let mut rng = StdRng::seed_from_u64(s);
        for _ in 0..50_000 {
            let v = random_value(&mut rng, 0);
            let t = TypeName::of(&v);
            let text = lexical(&v);
            let back = parse(&TypeName::parse(&t.to_string()).unwrap(), &text)
                .unwrap_or_else(|| panic!("seed {s}: {text:?} ({t}) does not read back"));
            assert!(bit_eq(&back, &v), "seed {s}: {text:?}: {back:?} != {v:?}");
        }
    }
}
