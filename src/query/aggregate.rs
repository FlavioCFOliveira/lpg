//! Aggregate accumulators (rmp #42, used by the aggregation operator of
//! #45): `COUNT(*)`, `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `STDDEV_SAMP`,
//! `STDDEV_POP`, each with or without `DISTINCT`.
//!
//! * NULL inputs are ignored; an empty input gives 0 for the counts and
//!   NULL for the others.
//! * `COUNT` is `INT64` (ID059). `SUM` of `INT64` values is `INT64` and
//!   widens to `FLOAT64` when the total leaves the `INT64` range (ID095,
//!   Neo4j `overflowSafeAdd`); with a `FLOAT64` input it is `FLOAT64`
//!   (ID097); `SUM` of durations of one kind is that kind. `AVG` and the
//!   standard deviations of numbers are `FLOAT64` (ID096, ID098).
//! * `MIN` and `MAX` take the total order ([`total_cmp`]) over the
//!   non-null values.
//! * `DISTINCT` keeps the values whose order keys ([`key::encode`]) differ:
//!   equal values, such as 1 and 1.0, have equal keys.
//!
//! `COLLECT_LIST` is feature GF10, which is not supported.
//!
//! The accumulator shape (one state per function, `update` then
//! `finish`) is that of Grafeo @ `3c23dbe`
//! `crates/grafeo-core/src/execution/operators/accumulator.rs`, studied
//! only.

use std::collections::BTreeSet;

use super::expr::{as_f64, finite, invalid_type, type_name};
use crate::error::Error;
use crate::types::{DayToSecond, Value, key, total_cmp};

/// An aggregate function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AggKind {
    CountStar,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    StddevSamp,
    StddevPop,
}

/// The running total of `SUM`.
#[derive(Debug, Clone, Copy)]
enum Total {
    Empty,
    Int(i64),
    Float(f64),
    Months(i64),
    Nanos(i128),
}

/// The running state of one aggregate over one group.
#[derive(Debug, Clone)]
pub(crate) struct Accumulator {
    kind: AggKind,
    distinct: Option<BTreeSet<Vec<u8>>>,
    count: i64,
    total: Total,
    /// The sum of the inputs of `AVG`, and Welford's running mean and sum
    /// of squared deviations of the standard deviations.
    float_sum: f64,
    mean: f64,
    m2: f64,
    best: Option<Value>,
}

impl Accumulator {
    /// An empty accumulator.
    pub(crate) fn new(kind: AggKind, distinct: bool) -> Accumulator {
        Accumulator {
            kind,
            distinct: distinct.then(BTreeSet::new),
            count: 0,
            total: Total::Empty,
            float_sum: 0.0,
            mean: 0.0,
            m2: 0.0,
            best: None,
        }
    }

    /// The bytes the accumulator holds beyond its own size: the keys of
    /// its distinct values and its best value.
    pub(crate) fn held(&self) -> u64 {
        let d: usize = self
            .distinct
            .as_ref()
            .map_or(0, |s| s.iter().map(|k| k.len() + 48).sum());
        (d + self.best.as_ref().map_or(0, super::exec::value_bytes)) as u64
    }

    /// Adds one input value (for `COUNT(*)`, any value: a row).
    ///
    /// # Errors
    ///
    /// 22G03 for a value the function does not take; 22003 for a
    /// `FLOAT64` total that overflows; 22015 for a duration total that
    /// overflows.
    pub(crate) fn update(&mut self, v: &Value) -> Result<(), Error> {
        if self.kind == AggKind::CountStar {
            self.count += 1;
            return Ok(());
        }
        if v.is_null() {
            return Ok(());
        }
        if let Some(seen) = &mut self.distinct {
            let mut k = Vec::new();
            key::encode(v, &mut k);
            if !seen.insert(k) {
                return Ok(());
            }
        }
        self.count += 1;
        match self.kind {
            AggKind::CountStar | AggKind::Count => {}
            AggKind::Min | AggKind::Max => {
                let replace = match &self.best {
                    None => true,
                    Some(b) => {
                        let o = total_cmp(v, b);
                        if self.kind == AggKind::Min {
                            o.is_lt()
                        } else {
                            o.is_gt()
                        }
                    }
                };
                if replace {
                    self.best = Some(v.clone());
                }
            }
            AggKind::Sum => self.total = self.add(v)?,
            AggKind::Avg | AggKind::StddevSamp | AggKind::StddevPop => {
                if !matches!(v, Value::Int(_) | Value::Float(_)) {
                    return Err(self.wrong(v));
                }
                let x = as_f64(v);
                let delta = x - self.mean;
                self.mean += delta / self.count as f64;
                self.m2 += delta * (x - self.mean);
                self.float_sum += x;
                finite(self.float_sum)?;
            }
        }
        Ok(())
    }

    fn wrong(&self, v: &Value) -> Error {
        invalid_type(&format!("{:?} of {}", self.kind, type_name(v)))
    }

    /// The `SUM` total after `v`.
    fn add(&self, v: &Value) -> Result<Total, Error> {
        let overflow = || Error::new(crate::gqlstatus::codes::C22015, "interval field overflow");
        Ok(match (self.total, v) {
            (Total::Empty, Value::Int(i)) => Total::Int(*i),
            (Total::Int(s), Value::Int(i)) => match s.checked_add(*i) {
                Some(t) => Total::Int(t),
                // ID095: the total widens to FLOAT64.
                None => Total::Float(s as f64 + *i as f64),
            },
            (Total::Float(s), Value::Int(_) | Value::Float(_)) => {
                finite(s + as_f64(v))?;
                Total::Float(s + as_f64(v))
            }
            (Total::Empty | Total::Int(_), Value::Float(f)) => {
                let s = match self.total {
                    Total::Int(s) => s as f64,
                    _ => 0.0,
                };
                finite(s + f)?;
                Total::Float(s + f)
            }
            (Total::Empty, Value::YearToMonth(m)) => Total::Months(*m),
            (Total::Months(s), Value::YearToMonth(m)) => {
                Total::Months(s.checked_add(*m).ok_or_else(overflow)?)
            }
            (Total::Empty | Total::Nanos(_), Value::DayToSecond(d)) => {
                let n = i128::from(d.seconds) * 1_000_000_000 + i128::from(d.nanos);
                match self.total {
                    Total::Nanos(s) => Total::Nanos(s + n),
                    _ => Total::Nanos(n),
                }
            }
            _ => return Err(self.wrong(v)),
        })
    }

    /// The aggregate's value.
    ///
    /// # Errors
    ///
    /// 22015 for a duration total out of range; 22003 for a `FLOAT64`
    /// result that is not finite.
    pub(crate) fn finish(&self) -> Result<Value, Error> {
        Ok(match self.kind {
            AggKind::CountStar | AggKind::Count => Value::Int(self.count),
            _ if self.count == 0 => Value::Null,
            AggKind::Min | AggKind::Max => self.best.clone().unwrap_or(Value::Null),
            AggKind::Sum => match self.total {
                Total::Empty => Value::Null,
                Total::Int(s) => Value::Int(s),
                Total::Float(f) => finite(f)?,
                Total::Months(m) => Value::YearToMonth(m),
                Total::Nanos(n) => {
                    let s = i64::try_from(n.div_euclid(1_000_000_000)).ok();
                    match s.and_then(|s| DayToSecond::new(s, n.rem_euclid(1_000_000_000) as i64)) {
                        Some(d) => Value::DayToSecond(d),
                        None => {
                            return Err(Error::new(
                                crate::gqlstatus::codes::C22015,
                                "interval field overflow",
                            ));
                        }
                    }
                }
            },
            AggKind::Avg => finite(self.float_sum / self.count as f64)?,
            AggKind::StddevSamp => {
                if self.count < 2 {
                    Value::Null
                } else {
                    finite((self.m2 / (self.count - 1) as f64).sqrt())?
                }
            }
            AggKind::StddevPop => finite((self.m2 / self.count as f64).sqrt())?,
        })
    }
}
