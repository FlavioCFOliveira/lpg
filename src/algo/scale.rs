//! GDS's scalers of centrality scores (the `scaler` key of ALG-040 and
//! ALG-042 to ALG-044; rmp #63).
//!
//! Formulas as GDS 2026.09.0 applies them, studied in GDS @ `417d7ecf973d`
//! `algo-common/src/main/java/org/neo4j/gds/scaling/*.java` (GPL: studied,
//! not copied): statistics skip NaN values; a spread or norm below
//! `1e-15` scales every value to 0; `log` adds its `offset` (default 0)
//! before the natural logarithm. The statistics are summed in node order
//! on one thread, so the result does not depend on `concurrency`.

use std::sync::Arc;

use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// GDS's `CLOSE_TO_ZERO` (`Scaler.java:31`).
const CLOSE_TO_ZERO: f64 = 1e-15;

/// The scaler names GDS accepts, in lower case.
const NAMES: &[&str] = &[
    "none", "mean", "max", "log", "center", "stdscore", "l1norm", "l2norm", "minmax",
];

/// A scaler (module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Scaler {
    None,
    Mean,
    Max,
    Log(f64),
    Center,
    StdScore,
    L1Norm,
    L2Norm,
    MinMax,
}

fn bad(key: &str) -> Error {
    Error::new(
        codes::C22N11,
        format!(
            "invalid argument: the configuration key {key} must name a scaler: {}",
            NAMES.join(", ")
        ),
    )
}

impl Scaler {
    /// The scaler a configuration value names: a name, or a record with
    /// `type` and, for `log`, `offset`.
    ///
    /// # Errors
    ///
    /// 22N11 for an unknown scaler or record key; 22G03 for a value of the
    /// wrong type.
    pub(crate) fn parse(key: &str, v: &Value) -> Result<Scaler, Error> {
        let wrong = || {
            Error::new(
                codes::C22G03,
                format!(
                    "invalid value type: the configuration key {key} must be a string or a record"
                ),
            )
        };
        let (name, offset) = match v {
            Value::String(s) => (s.to_lowercase(), None),
            Value::Record(r) => {
                let mut name = None;
                let mut offset = None;
                for (k, x) in r.iter() {
                    match (&**k, x) {
                        ("type", Value::String(s)) => name = Some(s.to_lowercase()),
                        ("offset", Value::Int(i)) => offset = Some(*i as f64),
                        ("offset", Value::Float(f)) => offset = Some(*f),
                        ("type" | "offset", _) => return Err(wrong()),
                        _ => return Err(bad(key)),
                    }
                }
                (name.ok_or_else(|| bad(key))?, offset)
            }
            _ => return Err(wrong()),
        };
        if offset.is_some() && name != "log" {
            return Err(bad(key));
        }
        Ok(match name.as_str() {
            "none" => Scaler::None,
            "mean" => Scaler::Mean,
            "max" => Scaler::Max,
            "log" => Scaler::Log(offset.unwrap_or(0.0)),
            "center" => Scaler::Center,
            "stdscore" => Scaler::StdScore,
            "l1norm" => Scaler::L1Norm,
            "l2norm" => Scaler::L2Norm,
            "minmax" => Scaler::MinMax,
            _ => return Err(bad(key)),
        })
    }

    /// The scaler as the parameter record keeps it: its upper-case name,
    /// or a record with `type` and `offset` for `log` with an offset.
    pub(crate) fn value(self) -> Value {
        let name = match self {
            Scaler::None => "NONE",
            Scaler::Mean => "MEAN",
            Scaler::Max => "MAX",
            Scaler::Log(o) if o != 0.0 => {
                return Value::Record(
                    vec![
                        (Arc::from("type"), Value::string("LOG")),
                        (Arc::from("offset"), Value::Float(o)),
                    ]
                    .into(),
                );
            }
            Scaler::Log(_) => "LOG",
            Scaler::Center => "CENTER",
            Scaler::StdScore => "STDSCORE",
            Scaler::L1Norm => "L1NORM",
            Scaler::L2Norm => "L2NORM",
            Scaler::MinMax => "MINMAX",
        };
        Value::string(name)
    }

    /// The scaler a parameter record holds ([`Scaler::value`]).
    pub(crate) fn of(v: &Value) -> Scaler {
        Scaler::parse("scaler", v).unwrap_or(Scaler::None)
    }

    /// Scales `xs` in place.
    pub(crate) fn apply(self, xs: &mut [f64]) {
        let present = || xs.iter().copied().filter(|x| !x.is_nan());
        let count = present().count() as f64;
        let sum: f64 = present().sum();
        let min = present().fold(f64::MAX, f64::min);
        let max = present().fold(-f64::MAX, f64::max);
        let f: Box<dyn Fn(f64) -> f64> = match self {
            Scaler::None => return,
            Scaler::Log(offset) => Box::new(move |x| (x + offset).ln()),
            Scaler::Mean => {
                let (avg, diff) = (sum / count, max - min);
                if diff.abs() < CLOSE_TO_ZERO {
                    Box::new(|_| 0.0)
                } else {
                    Box::new(move |x| (x - avg) / diff)
                }
            }
            Scaler::MinMax => {
                let diff = max - min;
                if diff.abs() < CLOSE_TO_ZERO {
                    Box::new(|_| 0.0)
                } else {
                    Box::new(move |x| (x - min) / diff)
                }
            }
            Scaler::Max => {
                let abs = present().fold(0.0, |a: f64, x| a.max(x.abs()));
                if abs < CLOSE_TO_ZERO {
                    Box::new(|_| 0.0)
                } else {
                    Box::new(move |x| x / abs)
                }
            }
            Scaler::Center => {
                let avg = sum / count;
                Box::new(move |x| x - avg)
            }
            Scaler::StdScore => {
                let avg = sum / count;
                let squares: f64 = present().map(|x| x * x).sum();
                let std = ((squares - avg * sum) / count).sqrt();
                if std < CLOSE_TO_ZERO {
                    Box::new(|_| 0.0)
                } else {
                    Box::new(move |x| (x - avg) / std)
                }
            }
            Scaler::L1Norm => {
                let norm: f64 = present().map(f64::abs).sum();
                if norm < CLOSE_TO_ZERO {
                    Box::new(|_| 0.0)
                } else {
                    Box::new(move |x| x / norm)
                }
            }
            Scaler::L2Norm => {
                let norm = present().map(|x| x * x).sum::<f64>().sqrt();
                if norm < CLOSE_TO_ZERO {
                    Box::new(|_| 0.0)
                } else {
                    Box::new(move |x| x / norm)
                }
            }
        };
        for x in xs.iter_mut() {
            *x = f(*x);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled(s: Scaler) -> Vec<f64> {
        let mut xs = vec![1.0, 2.0, 3.0, 4.0];
        s.apply(&mut xs);
        xs
    }

    /// Each scaler on [1, 2, 3, 4], and the zero rule on a constant input.
    #[test]
    fn formulas() {
        assert_eq!(scaled(Scaler::None), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(scaled(Scaler::MinMax), [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]);
        assert_eq!(scaled(Scaler::Max), [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(scaled(Scaler::Center), [-1.5, -0.5, 0.5, 1.5]);
        assert_eq!(scaled(Scaler::Mean), [-0.5, -0.5 / 3.0, 0.5 / 3.0, 0.5]);
        assert_eq!(scaled(Scaler::L1Norm), [0.1, 0.2, 0.3, 0.4]);
        let n = 30f64.sqrt();
        assert_eq!(scaled(Scaler::L2Norm), [1.0 / n, 2.0 / n, 3.0 / n, 4.0 / n]);
        let sd = 1.25f64.sqrt();
        assert_eq!(
            scaled(Scaler::StdScore),
            [-1.5 / sd, -0.5 / sd, 0.5 / sd, 1.5 / sd]
        );
        assert_eq!(scaled(Scaler::Log(1.0))[0], 2f64.ln());
        let mut flat = vec![2.0; 3];
        Scaler::MinMax.apply(&mut flat);
        assert_eq!(flat, [0.0; 3]);
    }

    #[test]
    fn parse_forms() {
        let p = |v: Value| Scaler::parse("scaler", &v);
        assert_eq!(p(Value::string("minMax")).unwrap(), Scaler::MinMax);
        let log = Value::Record(
            vec![
                (Arc::from("type"), Value::string("log")),
                (Arc::from("offset"), Value::Int(2)),
            ]
            .into(),
        );
        assert_eq!(p(log.clone()).unwrap(), Scaler::Log(2.0));
        assert_eq!(Scaler::of(&Scaler::Log(2.0).value()), Scaler::Log(2.0));
        assert_eq!(p(Value::string("x")).unwrap_err().code(), "22N11");
        assert_eq!(p(Value::Int(1)).unwrap_err().code(), "22G03");
    }
}
