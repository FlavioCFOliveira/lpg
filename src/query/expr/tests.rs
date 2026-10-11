//! Tests of expression evaluation (rmp #42): three-valued logic, NULL
//! propagation, comparison and order, arithmetic errors, the temporal
//! functions of GV39–GV41 against `jiff`, and the aggregate accumulators.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{Offset, TimeZone};
use jiff::{SignedDuration, Span, Timestamp, Unit};

use super::*;
use crate::query::aggregate::{Accumulator, AggKind};
use crate::query::functions::{Func, TABLE};
use crate::types::{ZonedDateTime, ZonedTime};

fn now() -> Timestamp {
    // 2026-10-10T12:34:56.789Z, a fixed statement clock.
    Timestamp::new(1_791_635_696, 789_000_000).unwrap()
}

fn ev(e: &Expr) -> Result<Value, Error> {
    eval(e, &[], &mut NoEnv { now: now() })
}

fn tok(kind: TokenKind) -> Tok {
    Tok {
        kind,
        name: Arc::from("p"),
        token: Some(1),
    }
}

fn lit(v: Value) -> Box<Expr> {
    Box::new(Expr::Lit(v))
}

fn t3(t: Option<bool>) -> Value {
    t.map_or(Value::Null, Value::Bool)
}

const TRUTH: [Option<bool>; 3] = [Some(true), Some(false), None];

/// The truth tables of ISO/IEC 39075 §19.6 (as SQL's), written out.
#[test]
fn truth_tables_hold_for_every_combination_of_true_false_and_unknown() {
    let (t, f, u) = (Some(true), Some(false), None);
    let and = |a, b| match (a, b) {
        (Some(true), Some(true)) => t,
        (Some(false), _) | (_, Some(false)) => f,
        _ => u,
    };
    let or = |a, b| match (a, b) {
        (Some(false), Some(false)) => f,
        (Some(true), _) | (_, Some(true)) => t,
        _ => u,
    };
    let mut n = 0;
    for a in TRUTH {
        for b in TRUTH {
            let got = ev(&Expr::And(lit(t3(a)), lit(t3(b)))).unwrap();
            assert_eq!(got, t3(and(a, b)), "{a:?} AND {b:?}");
            let got = ev(&Expr::Or(lit(t3(a)), lit(t3(b)))).unwrap();
            assert_eq!(got, t3(or(a, b)), "{a:?} OR {b:?}");
            n += 2;
        }
        let not = ev(&Expr::Not(lit(t3(a)))).unwrap();
        assert_eq!(not, t3(a.map(|x| !x)), "NOT {a:?}");
        for (truth, want) in [(Truth::True, t), (Truth::False, f), (Truth::Unknown, u)] {
            for negated in [false, true] {
                let got = ev(&Expr::IsTruth(lit(t3(a)), negated, truth)).unwrap();
                assert_eq!(
                    got,
                    Value::Bool((a == want) != negated),
                    "{a:?} IS {truth:?}"
                );
                n += 1;
            }
        }
    }
    assert_eq!(n, 9 * 2 + 3 * 6);
    // A filter keeps only TRUE.
    assert!(is_true(&Value::Bool(true)));
    assert!(!is_true(&Value::Bool(false)) && !is_true(&Value::Null));
    // A non-boolean operand is 22G03.
    let e = ev(&Expr::And(lit(Value::Int(1)), lit(Value::Bool(true)))).unwrap_err();
    assert_eq!(e.code(), "22G03");
}

fn sample(f: Func) -> Vec<Value> {
    // A valid argument list of each arity-1 or arity-2 function.
    match f {
        Func::Left | Func::Right => vec![Value::string("abc"), Value::Int(1)],
        Func::Abs => vec![Value::YearToMonth(-3)],
        Func::DurationBetween | Func::DurationBetweenMonths => vec![
            Value::Date(Date::constant(2020, 1, 1)),
            Value::Date(Date::constant(2021, 3, 1)),
        ],
        Func::Date => vec![Value::string("2020-01-02")],
        Func::ZonedTime => vec![Value::string("10:00+01:00")],
        Func::LocalTime => vec![Value::string("10:00")],
        Func::ZonedDatetime => vec![Value::string("2020-01-02T03:04:05+01:00")],
        Func::LocalDatetime => vec![Value::string("2020-01-02T03:04:05")],
        Func::Duration => vec![Value::string("P1Y")],
        _ => vec![Value::string("a b")],
    }
}

/// NULL in, NULL out: every operator, and every function of the table that
/// takes an argument, with each argument NULL in turn.
#[test]
fn each_operator_and_function_returns_null_for_a_null_argument() {
    let null = || lit(Value::Null);
    let one = || lit(Value::Int(1));
    let mut cases: Vec<(&str, Expr)> = vec![
        ("NOT NULL", Expr::Not(null())),
        ("-NULL", Expr::Neg(null())),
        ("+NULL", Expr::Pos(null())),
        ("NULL || 'a'", Expr::Concat(null(), lit(Value::string("a")))),
        ("'a' || NULL", Expr::Concat(lit(Value::string("a")), null())),
        (
            "NULL IS NORMALIZED",
            Expr::IsNormalized(null(), false, NormalForm::Nfc),
        ),
        ("NULL.p", Expr::Prop(null(), tok(TokenKind::PropertyKey))),
        (
            "NULL:L",
            Expr::Labels(null(), LabelTest::Label(tok(TokenKind::Label))),
        ),
        (
            "COALESCE(NULL, NULL)",
            Expr::Coalesce(vec![Expr::Lit(Value::Null); 2]),
        ),
        (
            "CASE WHEN FALSE THEN 1 END",
            Expr::Case(
                vec![(Expr::Lit(Value::Bool(false)), Expr::Lit(Value::Int(1)))],
                null(),
            ),
        ),
    ];
    for op in [ArithOp::Add, ArithOp::Sub, ArithOp::Mul, ArithOp::Div] {
        cases.push(("arith", Expr::Arith(op, null(), one())));
        cases.push(("arith", Expr::Arith(op, one(), null())));
    }
    for op in [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Lt,
        CmpOp::Le,
        CmpOp::Gt,
        CmpOp::Ge,
    ] {
        cases.push(("cmp", Expr::Cmp(op, null(), one())));
        cases.push(("cmp", Expr::Cmp(op, one(), null())));
    }
    let mut functions = 0;
    for r in TABLE.iter().filter(|r| r.arity > 0) {
        let args = sample(r.func);
        assert_eq!(args.len(), r.arity, "{}", r.name);
        // The sample is valid: not NULL.
        let ok = ev(&Expr::Call(
            r.func,
            args.iter().cloned().map(Expr::Lit).collect(),
        ))
        .unwrap();
        assert!(!ok.is_null(), "{}", r.name);
        for i in 0..r.arity {
            let mut a: Vec<Expr> = args.iter().cloned().map(Expr::Lit).collect();
            a[i] = Expr::Lit(Value::Null);
            cases.push((r.name, Expr::Call(r.func, a)));
        }
        functions += 1;
    }
    assert_eq!(functions, TABLE.iter().filter(|r| r.arity > 0).count());
    assert!(functions >= 16);
    for (name, e) in &cases {
        assert_eq!(ev(e).unwrap(), Value::Null, "{name}: {e:?}");
    }
    // The IS predicates are never NULL.
    assert_eq!(ev(&Expr::IsNull(null(), false)).unwrap(), Value::Bool(true));
    assert_eq!(
        ev(&Expr::IsTruth(null(), false, Truth::Unknown)).unwrap(),
        Value::Bool(true)
    );
}

/// BR-GQL-002 acceptance: `1 = 1.0` is TRUE, `1 < 'a'` is NULL, NULL sorts
/// last ascending.
#[test]
fn integer_equals_float_cross_group_order_is_null_and_null_sorts_last() {
    let c = |op, a, b| ev(&Expr::Cmp(op, lit(a), lit(b))).unwrap();
    assert_eq!(
        c(CmpOp::Eq, Value::Int(1), Value::Float(1.0)),
        Value::Bool(true)
    );
    assert_eq!(c(CmpOp::Lt, Value::Int(1), Value::string("a")), Value::Null);
    assert_eq!(c(CmpOp::Ge, Value::Int(1), Value::string("a")), Value::Null);
    assert_eq!(
        c(CmpOp::Eq, Value::Int(1), Value::string("a")),
        Value::Bool(false)
    );
    assert_eq!(
        c(CmpOp::Ne, Value::Int(1), Value::string("a")),
        Value::Bool(true)
    );
    let nan = Value::Float(f64::NAN);
    assert_eq!(c(CmpOp::Eq, nan.clone(), nan.clone()), Value::Bool(false));
    assert_eq!(c(CmpOp::Lt, nan.clone(), Value::Int(1)), Value::Bool(false));
    assert_eq!(
        c(CmpOp::Lt, Value::Int(1), Value::Float(1.5)),
        Value::Bool(true)
    );
    let mut vs = vec![
        Value::Null,
        Value::Int(3),
        Value::string("b"),
        Value::Null,
        Value::Float(-1.5),
        Value::Bool(true),
    ];
    vs.sort_by(|a, b| sort_cmp(a, b, false));
    assert!(vs[4].is_null() && vs[5].is_null(), "{vs:?}");
    vs.sort_by(|a, b| sort_cmp(a, b, true));
    assert!(vs[0].is_null() && vs[1].is_null(), "{vs:?}");
}

/// IA010 "Erro sempre", IA011: overflow and division by zero.
#[test]
fn integer_overflow_and_division_by_zero_fail_with_class_22_codes() {
    let a = |op, x, y| ev(&Expr::Arith(op, lit(x), lit(y)));
    let code = |r: Result<Value, Error>| r.unwrap_err().code();
    assert_eq!(
        code(a(ArithOp::Add, Value::Int(i64::MAX), Value::Int(1))),
        "22003"
    );
    assert_eq!(
        code(a(ArithOp::Sub, Value::Int(i64::MIN), Value::Int(1))),
        "22003"
    );
    assert_eq!(
        code(a(ArithOp::Mul, Value::Int(i64::MAX), Value::Int(2))),
        "22003"
    );
    assert_eq!(
        code(a(ArithOp::Div, Value::Int(i64::MIN), Value::Int(-1))),
        "22003"
    );
    assert_eq!(code(ev(&Expr::Neg(lit(Value::Int(i64::MIN))))), "22003");
    assert_eq!(code(a(ArithOp::Div, Value::Int(1), Value::Int(0))), "22012");
    assert_eq!(
        code(a(ArithOp::Div, Value::Float(1.0), Value::Float(0.0))),
        "22012"
    );
    assert_eq!(
        code(a(ArithOp::Div, Value::Int(1), Value::Float(0.0))),
        "22012"
    );
    assert_eq!(
        code(a(ArithOp::Mul, Value::Float(1e308), Value::Int(10))),
        "22003"
    );
    assert_eq!(
        code(a(ArithOp::Add, Value::Int(1), Value::string("a"))),
        "22G03"
    );
    // IA011 and ID063: truncation toward zero; an INT64 meets a FLOAT64.
    assert_eq!(
        a(ArithOp::Div, Value::Int(-7), Value::Int(2)).unwrap(),
        Value::Int(-3)
    );
    assert_eq!(
        a(ArithOp::Add, Value::Int(1), Value::Float(0.5)).unwrap(),
        Value::Float(1.5)
    );
    assert_eq!(
        code(a(
            ArithOp::Add,
            Value::YearToMonth(i64::MAX),
            Value::YearToMonth(1)
        )),
        "22015"
    );
}

fn call(f: Func, args: Vec<Value>) -> Value {
    ev(&Expr::Call(f, args.into_iter().map(Expr::Lit).collect())).unwrap()
}

fn record(fields: &[(&str, i64)]) -> Value {
    Value::Record(
        fields
            .iter()
            .map(|(n, v)| (Arc::from(*n), Value::Int(*v)))
            .collect::<Vec<_>>()
            .into(),
    )
}

/// Each temporal function of GV39–GV41 against the value `jiff` computes
/// independently.
#[test]
fn temporal_functions_return_what_jiff_computes() {
    let utc = now().to_zoned(TimeZone::fixed(Offset::UTC));
    // Current values at the statement clock, in the session zone (UTC).
    assert_eq!(call(Func::CurrentDate, vec![]), Value::Date(utc.date()));
    assert_eq!(
        call(Func::CurrentLocalTime, vec![]),
        Value::LocalTime(utc.time())
    );
    assert_eq!(
        call(Func::CurrentLocalTimestamp, vec![]),
        Value::LocalDateTime(utc.datetime())
    );
    assert_eq!(
        call(Func::CurrentTime, vec![]),
        Value::ZonedTime(ZonedTime {
            time: utc.time(),
            offset: 0
        })
    );
    match call(Func::CurrentTimestamp, vec![]) {
        Value::ZonedDateTime(z) => assert_eq!(z.zoned.timestamp(), now()),
        other => panic!("{other:?}"),
    }
    // Constructors from strings and records.
    let d = Date::new(2024, 2, 29).unwrap();
    assert_eq!(
        call(Func::Date, vec![Value::string("2024-02-29")]),
        Value::Date(d)
    );
    assert_eq!(
        call(
            Func::Date,
            vec![record(&[("year", 2024), ("month", 2), ("day", 29)])]
        ),
        Value::Date(d)
    );
    let t = Time::new(12, 34, 56, 123_456_789).unwrap();
    assert_eq!(
        call(Func::LocalTime, vec![Value::string("12:34:56.123456789")]),
        Value::LocalTime(t)
    );
    assert_eq!(
        call(
            Func::LocalTime,
            vec![record(&[
                ("hour", 12),
                ("minute", 34),
                ("second", 56),
                ("nanosecond", 123_456_789)
            ])]
        ),
        Value::LocalTime(t)
    );
    assert_eq!(
        call(
            Func::ZonedTime,
            vec![Value::string("12:34:56.123456789+02:00")]
        ),
        Value::ZonedTime(ZonedTime {
            time: t,
            offset: 7200
        })
    );
    let dt = DateTime::from_parts(d, t);
    assert_eq!(
        call(
            Func::LocalDatetime,
            vec![Value::string("2024-02-29T12:34:56.123456789")]
        ),
        Value::LocalDateTime(dt)
    );
    let lisbon = TimeZone::get("Europe/Lisbon").unwrap();
    let want = dt.to_zoned(lisbon.clone()).unwrap();
    match call(
        Func::ZonedDatetime,
        vec![Value::string(
            "2024-02-29T12:34:56.123456789+00:00[Europe/Lisbon]",
        )],
    ) {
        Value::ZonedDateTime(z) => assert_eq!(z.zoned, want),
        other => panic!("{other:?}"),
    }
    let mut fields: Vec<(Arc<str>, Value)> =
        [("year", 2024), ("month", 2), ("day", 29), ("hour", 12)]
            .iter()
            .map(|(n, v)| (Arc::from(*n), Value::Int(*v)))
            .collect();
    fields.push((Arc::from("timezone"), Value::string("Europe/Lisbon")));
    match call(Func::ZonedDatetime, vec![Value::Record(fields.into())]) {
        Value::ZonedDateTime(z) => assert_eq!(
            z.zoned,
            DateTime::constant(2024, 2, 29, 12, 0, 0, 0)
                .to_zoned(lisbon)
                .unwrap()
        ),
        other => panic!("{other:?}"),
    }
    // Durations.
    assert_eq!(
        call(Func::Duration, vec![Value::string("P1Y2M")]),
        Value::YearToMonth(14)
    );
    assert_eq!(
        call(Func::Duration, vec![Value::string("PT36H")]),
        Value::DayToSecond(DayToSecond::new(36 * 3600, 0).unwrap())
    );
    assert_eq!(
        call(Func::Duration, vec![record(&[("days", 1), ("hours", 2)])]),
        Value::DayToSecond(DayToSecond::new(SignedDuration::from_hours(26).as_secs(), 0).unwrap())
    );
    let (a, b) = (Date::constant(2020, 1, 31), Date::constant(2021, 3, 1));
    let jd = b
        .to_datetime(Time::midnight())
        .duration_since(a.to_datetime(Time::midnight()));
    assert_eq!(
        call(Func::DurationBetween, vec![Value::Date(a), Value::Date(b)]),
        Value::DayToSecond(DayToSecond::new(jd.as_secs(), i64::from(jd.subsec_nanos())).unwrap())
    );
    let months = a
        .to_datetime(Time::midnight())
        .until((Unit::Month, b.to_datetime(Time::midnight())))
        .unwrap()
        .get_months();
    assert_eq!(
        call(
            Func::DurationBetweenMonths,
            vec![Value::Date(a), Value::Date(b)]
        ),
        Value::YearToMonth(i64::from(months))
    );
    assert_eq!(
        call(Func::Abs, vec![Value::YearToMonth(-5)]),
        Value::YearToMonth(5)
    );
    // Instant ± duration.
    let plus = ev(&Expr::Arith(
        ArithOp::Add,
        lit(Value::Date(Date::constant(2024, 1, 31))),
        lit(Value::YearToMonth(1)),
    ))
    .unwrap();
    assert_eq!(
        plus,
        Value::Date(
            Date::constant(2024, 1, 31)
                .checked_add(Span::new().months(1))
                .unwrap()
        )
    );
    let zdt = Value::ZonedDateTime(Box::new(ZonedDateTime::with_offset(now(), Offset::UTC)));
    match ev(&Expr::Arith(
        ArithOp::Sub,
        lit(zdt),
        lit(Value::DayToSecond(DayToSecond::new(90, 0).unwrap())),
    ))
    .unwrap()
    {
        Value::ZonedDateTime(z) => assert_eq!(
            z.zoned.timestamp(),
            now().checked_sub(SignedDuration::from_secs(90)).unwrap()
        ),
        other => panic!("{other:?}"),
    }
    let d2 = |s: i64| Value::DayToSecond(DayToSecond::new(s, 0).unwrap());
    assert_eq!(
        ev(&Expr::Arith(ArithOp::Add, lit(d2(90)), lit(d2(30)))).unwrap(),
        Value::DayToSecond(DayToSecond::new(SignedDuration::from_secs(120).as_secs(), 0).unwrap())
    );
    // Errors.
    let late = ev(&Expr::Arith(
        ArithOp::Add,
        lit(Value::Date(Date::MAX)),
        lit(Value::YearToMonth(12)),
    ))
    .unwrap_err();
    assert_eq!(late.code(), "22008");
    let bad: Vec<(Arc<str>, Value)> = vec![
        (Arc::from("year"), Value::string("x")),
        (Arc::from("month"), Value::Int(1)),
    ];
    let e = ev(&Expr::Call(
        Func::Date,
        vec![Expr::Lit(Value::Record(bad.into()))],
    ))
    .unwrap_err();
    assert_eq!(e.code(), "22G06");
    // Errors.
    let e = ev(&Expr::Call(
        Func::Date,
        vec![Expr::Lit(Value::string("2024-02-30"))],
    ))
    .unwrap_err();
    assert_eq!(e.code(), "22007");
    let e = ev(&Expr::Call(
        Func::Date,
        vec![Expr::Lit(record(&[("hour", 1)]))],
    ))
    .unwrap_err();
    assert_eq!(e.code(), "22G05");
    let e = ev(&Expr::Call(
        Func::Duration,
        vec![Expr::Lit(Value::string("P1X"))],
    ))
    .unwrap_err();
    assert_eq!(e.code(), "22G0H");
}

#[test]
fn string_functions_follow_the_approved_choices() {
    assert_eq!(
        call(Func::CharLength, vec![Value::string("aλb")]),
        Value::Int(3)
    );
    assert_eq!(
        call(Func::Upper, vec![Value::string("aλ")]),
        Value::string("AΛ")
    );
    assert_eq!(
        call(Func::Lower, vec![Value::string("AΛ")]),
        Value::string("aλ")
    );
    assert_eq!(
        call(Func::Left, vec![Value::string("aλbc"), Value::Int(2)]),
        Value::string("aλ")
    );
    assert_eq!(
        call(Func::Right, vec![Value::string("aλbc"), Value::Int(9)]),
        Value::string("aλbc")
    );
    // IV023: only U+0020 is trimmed.
    assert_eq!(
        call(Func::Trim, vec![Value::string(" \ta \u{A0} ")]),
        Value::string("\ta \u{A0}")
    );
    assert_eq!(
        call(
            Func::Normalize(NormalForm::Nfc),
            vec![Value::string("e\u{301}")]
        ),
        Value::string("\u{e9}")
    );
    let e = ev(&Expr::Call(
        Func::Left,
        vec![Expr::Lit(Value::string("a")), Expr::Lit(Value::Int(-1))],
    ))
    .unwrap_err();
    assert_eq!(e.code(), "22011");
    let e = ev(&Expr::Call(Func::Abs, vec![Expr::Lit(Value::Int(-1))])).unwrap_err();
    assert_eq!(e.cause().unwrap().code(), "51N26");
    assert_eq!(
        ev(&Expr::IsNormalized(
            lit(Value::string("e\u{301}")),
            false,
            NormalForm::Nfc
        ))
        .unwrap(),
        Value::Bool(false)
    );
}

fn agg(kind: AggKind, distinct: bool, vs: &[Value]) -> Result<Value, Error> {
    let mut a = Accumulator::new(kind, distinct);
    for v in vs {
        a.update(v)?;
    }
    a.finish()
}

#[test]
fn accumulators_ignore_nulls_widen_sums_and_deduplicate() {
    let vs = [Value::Int(1), Value::Null, Value::Float(1.0), Value::Int(3)];
    assert_eq!(agg(AggKind::CountStar, false, &vs).unwrap(), Value::Int(4));
    assert_eq!(agg(AggKind::Count, false, &vs).unwrap(), Value::Int(3));
    assert_eq!(agg(AggKind::Count, true, &vs).unwrap(), Value::Int(2));
    assert_eq!(agg(AggKind::Sum, false, &vs).unwrap(), Value::Float(5.0));
    assert_eq!(
        agg(AggKind::Sum, false, &[Value::Int(2), Value::Int(3)]).unwrap(),
        Value::Int(5)
    );
    assert_eq!(
        agg(AggKind::Sum, false, &[Value::Int(i64::MAX), Value::Int(1)]).unwrap(),
        Value::Float(i64::MAX as f64 + 1.0)
    );
    assert_eq!(
        agg(AggKind::Avg, false, &vs).unwrap(),
        Value::Float(5.0 / 3.0)
    );
    assert_eq!(agg(AggKind::Min, false, &vs).unwrap(), Value::Int(1));
    assert_eq!(agg(AggKind::Max, false, &vs).unwrap(), Value::Int(3));
    let sd = agg(
        AggKind::StddevSamp,
        false,
        &[Value::Int(2), Value::Int(4), Value::Int(6)],
    )
    .unwrap();
    assert_eq!(sd, Value::Float(2.0));
    let sp = agg(AggKind::StddevPop, false, &[Value::Int(2), Value::Int(4)]).unwrap();
    assert_eq!(sp, Value::Float(1.0));
    for k in [
        AggKind::Sum,
        AggKind::Avg,
        AggKind::Min,
        AggKind::Max,
        AggKind::StddevSamp,
    ] {
        assert_eq!(agg(k, false, &[Value::Null]).unwrap(), Value::Null, "{k:?}");
    }
    assert_eq!(agg(AggKind::Count, false, &[]).unwrap(), Value::Int(0));
    assert_eq!(
        agg(
            AggKind::Sum,
            false,
            &[Value::YearToMonth(1), Value::YearToMonth(2)]
        )
        .unwrap(),
        Value::YearToMonth(3)
    );
    assert_eq!(
        agg(AggKind::Sum, false, &[Value::Int(1), Value::string("a")])
            .unwrap_err()
            .code(),
        "22G03"
    );
}

#[test]
fn constant_folding_replaces_constant_calls_and_keeps_failing_ones() {
    let e = Expr::Arith(ArithOp::Add, lit(Value::Int(1)), lit(Value::Int(2))).fold();
    assert_eq!(e, Expr::Lit(Value::Int(3)));
    let clock = Expr::Call(Func::CurrentDate, vec![]).fold();
    assert!(matches!(clock, Expr::Call(..)));
    let div = Expr::Arith(ArithOp::Div, lit(Value::Int(1)), lit(Value::Int(0))).fold();
    assert!(matches!(div, Expr::Arith(..)));
    let param = Expr::Arith(ArithOp::Add, Box::new(Expr::Param(0)), lit(Value::Int(1))).fold();
    assert!(matches!(param, Expr::Arith(_, ref a, _) if **a == Expr::Param(0)));
}
