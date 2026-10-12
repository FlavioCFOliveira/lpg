//! The laboratory harness of the graph algorithms (rmp #61): the
//! determinism check, oracle outputs and their comparison rules, and a
//! fixture database. A family task (#64 to #69) checks its entries with
//! one table of [`Case`]s and [`check_cases`].
//!
//! * **Determinism** ([`determinism`]): a run at `concurrency` 1 to 16,
//!   twice each, gives rows bit-equal to the first run's (floats by their
//!   bits, `crate::types::lexical::bit_eq`); the first difference names
//!   the `concurrency`, the repetition, the row and the column.
//! * **Oracles** ([`expected`]): the JSON Lines that
//!   `docs/evidence/lab-sprint-5/gds_oracle.sh` and `nx_oracle.py` print,
//!   committed under `src/algo/testdata/expected`, so the tests need
//!   neither Docker nor Python. Rows are keyed by the node's `uid`.
//! * **Rules** ([`Rule`], `algorithm-entries.md` Verification): X exact,
//!   F `|a − b| ≤ t · max(1, |b|)`, P equal partitions up to renaming.
//!   Rules V and Q are entry-specific checks a family task writes as
//!   closures over the rows.
//! * Turso's SQL test runner compares a query's rows with an expected
//!   output stored next to the test, by an exact, unordered or pattern
//!   rule, and reports the first difference (R-TURSO @ `be5bab8`
//!   `testing/sqltest/src/comparison/`); the harness has the same shape
//!   with the entry rules. Studied, not copied.

use std::collections::BTreeMap;
use std::path::Path;

use crate::types::Value;
use crate::types::lexical::bit_eq;
use crate::{Database, Parameters, Session};

/// The `concurrency` values of the determinism check.
pub(crate) const CONCURRENCY: std::ops::RangeInclusive<usize> = 1..=16;

/// The first difference of a determinism check.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Mismatch {
    pub(crate) concurrency: usize,
    pub(crate) repetition: usize,
    pub(crate) row: usize,
    pub(crate) detail: String,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "concurrency {}, run {}: row {} differs from the first run: {}",
            self.concurrency, self.repetition, self.row, self.detail
        )
    }
}

/// The first row where `got` differs from `want`, bit for bit.
pub(crate) fn first_difference(want: &[Vec<Value>], got: &[Vec<Value>]) -> Option<(usize, String)> {
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        if w.len() != g.len() || w.iter().zip(g).any(|(a, b)| !bit_eq(a, b)) {
            return Some((i, format!("{g:?} instead of {w:?}")));
        }
    }
    (want.len() != got.len()).then(|| {
        (
            want.len().min(got.len()),
            format!("{} rows instead of {}", got.len(), want.len()),
        )
    })
}

/// Runs `run(concurrency)` at every [`CONCURRENCY`] value, twice each, and
/// returns the first run's rows, or the first difference (module
/// documentation).
///
/// # Errors
///
/// The first [`Mismatch`].
pub(crate) fn determinism(
    mut run: impl FnMut(usize) -> Vec<Vec<Value>>,
) -> Result<Vec<Vec<Value>>, Mismatch> {
    let first = run(*CONCURRENCY.start());
    for concurrency in CONCURRENCY {
        for repetition in 1..=2 {
            let rows = run(concurrency);
            if let Some((row, detail)) = first_difference(&first, &rows) {
                return Err(Mismatch {
                    concurrency,
                    repetition,
                    row,
                    detail,
                });
            }
        }
    }
    Ok(first)
}

/// The async form of [`determinism`]: `run` gives the rows of one run.
///
/// # Errors
///
/// As [`determinism`].
pub(crate) async fn determinism_async<F, Fut>(run: F) -> Result<Vec<Vec<Value>>, Mismatch>
where
    F: Fn(usize) -> Fut,
    Fut: std::future::Future<Output = Vec<Vec<Value>>>,
{
    let first = run(*CONCURRENCY.start()).await;
    for concurrency in CONCURRENCY {
        for repetition in 1..=2 {
            let rows = run(concurrency).await;
            if let Some((row, detail)) = first_difference(&first, &rows) {
                return Err(Mismatch {
                    concurrency,
                    repetition,
                    row,
                    detail,
                });
            }
        }
    }
    Ok(first)
}

/// A comparison rule (module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Rule {
    /// X: exact equality.
    Exact,
    /// F with tolerance `t`.
    Float(f64),
    /// P: equal partitions up to renaming of their identifiers.
    Partition,
}

/// The directory of the committed oracle outputs.
pub(crate) fn expected_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/algo/testdata/expected")
}

/// The rows of oracle output `file` (JSON Lines in [`expected_dir`]), each
/// a JSON object.
pub(crate) fn expected(file: &str) -> Vec<serde_json::Map<String, serde_json::Value>> {
    let path = expected_dir().join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| match serde_json::from_str(l) {
            Ok(serde_json::Value::Object(m)) => m,
            other => panic!("{file}: not a JSON object: {l} ({other:?})"),
        })
        .collect()
}

/// The values of column `value` of oracle output `file` by the integer in
/// column `key`.
pub(crate) fn expected_by_key(file: &str, key: &str, value: &str) -> BTreeMap<i64, Value> {
    expected(file)
        .into_iter()
        .map(|m| {
            let k = m[key]
                .as_i64()
                .unwrap_or_else(|| panic!("{file}: key {key}"));
            let v = match &m[value] {
                serde_json::Value::Number(n) if n.is_i64() => Value::Int(n.as_i64().unwrap()),
                serde_json::Value::Number(n) => Value::Float(n.as_f64().unwrap()),
                serde_json::Value::Bool(b) => Value::Bool(*b),
                serde_json::Value::String(s) => Value::string(s),
                serde_json::Value::Null => Value::Null,
                other => panic!("{file}: unsupported value {other}"),
            };
            (k, v)
        })
        .collect()
}

fn float(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Compares `got` with `want` (both by key) under `rule`; returns the
/// largest relative difference `|a − b| / max(1, |b|)` of rule F (0
/// otherwise).
///
/// # Errors
///
/// The first difference, by key.
pub(crate) fn compare(
    rule: Rule,
    got: &BTreeMap<i64, Value>,
    want: &BTreeMap<i64, Value>,
) -> Result<f64, String> {
    let gk: Vec<_> = got.keys().collect();
    let wk: Vec<_> = want.keys().collect();
    if gk != wk {
        return Err(format!("keys {gk:?} instead of {wk:?}"));
    }
    let mut worst = 0.0f64;
    match rule {
        Rule::Exact => {
            for (k, w) in want {
                if !bit_eq(&got[k], w) && got[k] != *w {
                    return Err(format!("key {k}: {:?} instead of {w:?}", got[k]));
                }
            }
        }
        Rule::Float(t) => {
            for (k, w) in want {
                let (Some(a), Some(b)) = (float(&got[k]), float(w)) else {
                    return Err(format!("key {k}: {:?} or {w:?} is not a number", got[k]));
                };
                let rel = if a == b || (a.is_nan() && b.is_nan()) {
                    0.0
                } else {
                    (a - b).abs() / b.abs().max(1.0)
                };
                if rel.is_nan() || rel > t {
                    return Err(format!(
                        "key {k}: {a} instead of {b} (relative {rel} > {t})"
                    ));
                }
                worst = worst.max(rel);
            }
        }
        Rule::Partition => {
            let mut map: BTreeMap<String, String> = BTreeMap::new();
            let mut back: BTreeMap<String, String> = BTreeMap::new();
            for (k, w) in want {
                let (a, b) = (format!("{:?}", got[k]), format!("{w:?}"));
                if map.entry(a.clone()).or_insert_with(|| b.clone()) != &b
                    || back.entry(b.clone()).or_insert_with(|| a.clone()) != &a
                {
                    return Err(format!("key {k}: the partitions differ"));
                }
            }
        }
    }
    Ok(worst)
}

/// A database loaded with one oracle graph fixture.
pub(crate) struct Fixture {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) db: Database,
    pub(crate) s: Session,
    /// The element identifier of each node, by `uid`.
    pub(crate) ids: Vec<u64>,
    /// The `uid` of each node, by element identifier.
    pub(crate) uids: BTreeMap<u64, i64>,
}

impl Fixture {
    /// A new database under `/tmp/lpgram` (CLAUDE.md) with the fixture
    /// `name` of `src/algo/testdata` loaded, and memory cap `cap`.
    pub(crate) async fn load(name: &str, cap: u64) -> Fixture {
        let dir = super::graph_dir();
        let db = Database::create(
            dir.path(),
            super::no_flush(),
            cap,
            super::OWNER,
            crate::Options::new(std::sync::Arc::new(AllowAll)),
        )
        .await
        .expect("create");
        let s = db.session(super::OWNER, None, None).await.expect("session");
        let ids = crate::lab::generator::load_oracle(&s, &fixture_dir(name)).await;
        let uids = ids
            .iter()
            .enumerate()
            .map(|(u, id)| (*id, u as i64))
            .collect();
        Fixture {
            _dir: dir,
            db,
            s,
            ids,
            uids,
        }
    }

    /// The rows of `CALL proc($c)` with `config`, in this session.
    pub(crate) async fn call(
        &self,
        proc: &str,
        config: Value,
    ) -> Result<Vec<Vec<Value>>, crate::Error> {
        rows(&self.s, &format!("CALL {proc}($c)"), param("c", config)).await
    }

    /// `rows` keyed by the `uid` of the node in column 0, with column 1.
    pub(crate) fn by_uid(&self, rows: &[Vec<Value>]) -> BTreeMap<i64, Value> {
        rows.iter()
            .map(|r| {
                let Value::Int(id) = r[0] else {
                    panic!("a node identifier: {r:?}")
                };
                (self.uids[&id.cast_unsigned()], r[1].clone())
            })
            .collect()
    }
}

/// The directory of fixture `name`.
pub(crate) fn fixture_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/algo/testdata")
        .join(name)
}

/// One parameter.
pub(crate) fn param(name: &str, v: Value) -> Parameters {
    Parameters::from([(name.to_string(), v)])
}

/// A GQL record of `pairs`.
pub(crate) fn record(pairs: &[(&str, Value)]) -> Value {
    Value::Record(
        pairs
            .iter()
            .map(|(k, v)| (std::sync::Arc::from(*k), v.clone()))
            .collect::<Vec<_>>()
            .into(),
    )
}

/// `config` with `key` set to `v`.
pub(crate) fn with(config: &Value, key: &str, v: Value) -> Value {
    let Value::Record(r) = config else {
        panic!("a record")
    };
    let mut out: Vec<_> = r.iter().filter(|(k, _)| &**k != key).cloned().collect();
    out.push((std::sync::Arc::from(key), v));
    Value::Record(out.into())
}

/// Every row of `gql` with `params` in `s`.
///
/// # Errors
///
/// The statement's error.
pub(crate) async fn rows(
    s: &Session,
    gql: &str,
    params: Parameters,
) -> Result<Vec<Vec<Value>>, crate::Error> {
    let mut stream = s.execute(gql, params).await?;
    let mut out = Vec::new();
    while let Some(r) = stream.next().await? {
        out.push(r);
    }
    Ok(out)
}

/// The rows of a stream.
///
/// # Errors
///
/// The stream's error.
pub(crate) async fn drain(
    mut stream: crate::ResultStream,
) -> Result<Vec<Vec<Value>>, crate::Error> {
    let mut out = Vec::new();
    while let Some(r) = stream.next().await? {
        out.push(r);
    }
    Ok(out)
}

/// An authoriser that allows everything.
pub(crate) struct AllowAll;

impl crate::auth::Authoriser for AllowAll {
    fn authenticate<'a>(
        &'a self,
        _scheme: &'a str,
        _credentials: &'a str,
    ) -> crate::auth::BoxFuture<'a, Option<crate::auth::Authenticated>> {
        Box::pin(async { None })
    }

    fn authorise<'a>(
        &'a self,
        _principal: &'a str,
        _action: crate::auth::Action,
        _object: &'a crate::auth::Object,
        _owner: &'a str,
    ) -> crate::auth::BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

/// One oracle comparison of an entry: a configuration, the oracle output
/// file and the rule (a family task's table row).
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) config: Value,
    pub(crate) expected: &'static str,
    /// Columns of the oracle output: the node key and the value.
    pub(crate) columns: (&'static str, &'static str),
    pub(crate) rule: Rule,
}

/// Checks each case of procedure `proc` on `f`: bit-equal rows at
/// `concurrency` 1 to 16, twice each; at most `concurrency` workers per
/// run (FR-ALG-006); the oracle's values under the case's rule. Returns
/// the largest rule-F difference of each case.
pub(crate) async fn check_cases(
    f: &Fixture,
    proc: &str,
    cases: &[Case],
) -> Vec<(&'static str, f64)> {
    let key = f.db.engine_handle().core().location.display().to_string();
    let mut out = Vec::new();
    for case in cases {
        crate::algo::sched::probe::take(&key);
        let rows = determinism_async(|c| {
            let config = with(&case.config, "concurrency", Value::Int(c as i64));
            let key = key.clone();
            async move {
                let rows = f
                    .call(proc, config)
                    .await
                    .unwrap_or_else(|e| panic!("{proc}: {e}"));
                let workers =
                    crate::algo::sched::probe::take(&key).expect("the run started workers");
                assert!(workers <= c, "{} workers at concurrency {c}", workers);
                rows
            }
        })
        .await
        .unwrap_or_else(|m| panic!("{proc} {}: {m}", case.name));
        let want = expected_by_key(case.expected, case.columns.0, case.columns.1);
        let worst = compare(case.rule, &f.by_uid(&rows), &want)
            .unwrap_or_else(|e| panic!("{proc} {} against {}: {e}", case.name, case.expected));
        out.push((case.name, worst));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(i64, Value)]) -> BTreeMap<i64, Value> {
        pairs.iter().cloned().collect()
    }

    /// Each rule accepts what it should and names the first difference.
    #[test]
    fn rules() {
        let a = map(&[(1, Value::Int(5)), (2, Value::Int(5)), (3, Value::Int(7))]);
        let renamed = map(&[(1, Value::Int(0)), (2, Value::Int(0)), (3, Value::Int(1))]);
        let split = map(&[(1, Value::Int(0)), (2, Value::Int(1)), (3, Value::Int(1))]);
        assert!(compare(Rule::Exact, &a, &a).is_ok());
        assert!(
            compare(Rule::Exact, &a, &renamed)
                .unwrap_err()
                .starts_with("key 1")
        );
        assert!(compare(Rule::Partition, &a, &renamed).is_ok());
        assert!(compare(Rule::Partition, &a, &split).is_err());
        let f = map(&[(1, Value::Float(1.0)), (2, Value::Float(100.0))]);
        let g = map(&[
            (1, Value::Float(1.0 + 1e-10)),
            (2, Value::Float(100.0 + 1e-8)),
        ]);
        let worst = compare(Rule::Float(1e-9), &g, &f).unwrap();
        assert!(worst > 9e-11 && worst <= 1e-9, "{worst}");
        assert!(compare(Rule::Float(1e-11), &g, &f).is_err());
        assert!(compare(Rule::Float(1.0), &map(&[(1, Value::Int(1))]), &f).is_err());
    }
}
