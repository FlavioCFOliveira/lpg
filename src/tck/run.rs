//! The steps of a scenario, run through the public API (own code).
//!
//! Each scenario gets a new database under `/tmp/lpgram` and one session of
//! [`OWNER`], which every request may act as (an authoriser that allows
//! everything: authorisation is not what the scenarios test).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::scan::{Scenario, Step};
use super::value::{self, Element, Expected};
use crate::api::{Counters, Summary};
use crate::auth::{Action, Authoriser, BoxFuture, Object};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::proc::{Parameter, ReadContext, ReadProcedure, Rows, Signature};
use crate::sync::Arc;
use crate::test_support::{OWNER, no_flush};
use crate::tx::Isolation;
use crate::types::Value;
use crate::{Database, Options, Parameters, Session};

/// Statements profiled to completion, and the highest number of operators
/// any of them ran at once (NFR-PERF-007), over a test process. Plain
/// `std` atomics: only counted, never used to order other memory.
pub(super) static PROFILED: AtomicUsize = AtomicUsize::new(0);
pub(super) static MAX_RUNNING: AtomicUsize = AtomicUsize::new(0);

/// The memory cap of a scenario's database: 32 MiB.
const CAP: u64 = 32 << 20;

/// Allows every request.
struct AllowAll;

impl Authoriser for AllowAll {
    fn authorise<'a>(
        &'a self,
        _principal: &'a str,
        _action: Action,
        _object: &'a Object,
        _owner: &'a str,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

/// What the last executed statement gave.
enum Outcome {
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
        summary: Option<Box<Summary>>,
    },
    Failed(Error),
}

/// The GQLSTATUS an openCypher error detail maps to: the code lpg's
/// specification assigns to that failure (`interfaces.md` "GQLSTATUS
/// codes"; binder codes of rmp #43). A type error may be found when the
/// statement is compiled (a syntax rule, class 42) or when it runs (a data
/// exception, 22G03), as the openCypher TCK itself allows both phases.
pub(super) fn detail_codes(detail: &str) -> Option<&'static [&'static str]> {
    Some(match detail {
        "UndefinedVariable" => &["42002"],
        "VariableAlreadyBound"
        | "VariableTypeConflict"
        | "RelationshipUniquenessViolation"
        | "InvalidAggregation"
        | "NestedAggregation"
        | "AmbiguousAggregationExpression"
        | "ColumnNameConflict"
        | "NoVariablesInScope"
        | "InvalidClauseComposition" => &["42000"],
        "InvalidArgumentType" => &["42000", "22G03"],
        "InvalidNumberOfArguments" => &["22N11"],
        "IntegerOverflow" | "FloatingPointOverflow" => &["22003"],
        "ProcedureNotFound" => &["42N08"],
        _ => return None,
    })
}

/// The side-effect keys of the TCK and the counter each adds to. Catalog
/// objects (graphs, schemas, directories, graph types) all count in
/// `catalog_updates`; a removed property counts as a property set, as in
/// Neo4j's `SummaryCounters`.
fn add_effect(c: &mut Counters, key: &str, n: u64) -> Result<(), String> {
    match key {
        "+nodes" => c.nodes_created += n,
        "-nodes" => c.nodes_deleted += n,
        "+relationships" => c.edges_created += n,
        "-relationships" => c.edges_deleted += n,
        "+properties" | "-properties" => c.properties_set += n,
        "+labels" => c.labels_added += n,
        "-labels" => c.labels_removed += n,
        "+graphs" | "-graphs" | "+schemas" | "-schemas" | "+directories" | "-directories"
        | "+graphtypes" | "-graphtypes" => c.catalog_updates += n,
        _ => return Err(format!("unknown side effect {key}")),
    }
    Ok(())
}

/// A procedure of the step `there exists a procedure …`: its table, with
/// the input values of each row selecting its outputs (openCypher TCK
/// semantics). The registry has no parameter types (rmp #50), so the
/// procedure checks them and fails with 22G03, invalid value type.
struct TableProcedure {
    inputs: Vec<(String, String)>,
    rows: Vec<(Vec<Value>, Vec<Value>)>,
}

impl ReadProcedure for TableProcedure {
    fn call(&self, _cx: &mut ReadContext<'_>, args: &[Value]) -> Result<Rows, Error> {
        let mut coerced = Vec::with_capacity(args.len());
        for ((_, ty), a) in self.inputs.iter().zip(args) {
            let ok = matches!(
                (ty.trim_end_matches('?'), a),
                (_, Value::Null)
                    | ("ANY", _)
                    | ("INTEGER", Value::Int(_))
                    | ("FLOAT" | "NUMBER", Value::Float(_))
                    | ("NUMBER", Value::Int(_))
                    | ("STRING", Value::String(_))
                    | ("BOOLEAN", Value::Bool(_))
            ) || matches!((ty.trim_end_matches('?'), a), ("FLOAT", Value::Int(_)));
            if !ok {
                return Err(Error::new(
                    codes::C22G03,
                    "an argument does not have its parameter's type",
                ));
            }
            coerced.push(match (ty.trim_end_matches('?'), a) {
                #[allow(clippy::cast_precision_loss)]
                ("FLOAT", Value::Int(i)) => Value::Float(*i as f64),
                _ => a.clone(),
            });
        }
        let out: Vec<Result<Vec<Value>, Error>> = self
            .rows
            .iter()
            .filter(|(ins, _)| *ins == coerced)
            .map(|(_, outs)| Ok(outs.clone()))
            .collect();
        Ok(Box::new(out.into_iter()))
    }
}

/// Parses `name(a :: T, …) :: (x :: T, …)`, the text before the final
/// `:`: name, inputs and outputs, each `(name, type)`.
type Sig = (String, Vec<(String, String)>, Vec<(String, String)>);

fn parse_signature(text: &str) -> Result<Sig, String> {
    let text = text.trim().trim_end_matches(':').trim();
    let open = text.find('(').ok_or("no '(' in the signature")?;
    let name = text[..open].trim().to_string();
    let close = text.find(')').ok_or("no ')' in the signature")?;
    let ins = &text[open + 1..close];
    let rest = text[close + 1..].trim();
    let rest = rest
        .strip_prefix("::")
        .ok_or("no '::' in the signature")?
        .trim();
    let outs = rest
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .ok_or("no output list in the signature")?;
    let list = |s: &str| -> Result<Vec<(String, String)>, String> {
        s.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (n, t) = p.split_once("::").ok_or("a field without '::'")?;
                Ok((n.trim().to_string(), t.trim().to_string()))
            })
            .collect()
    };
    Ok((name, list(ins)?, list(outs)?))
}

/// An expected value as a parameter or procedure value.
fn to_value(e: &Expected) -> Result<Value, String> {
    Ok(match e {
        Expected::Null => Value::Null,
        Expected::Bool(b) => Value::Bool(*b),
        Expected::Int(i) => Value::Int(*i),
        Expected::Float(f) => Value::Float(*f),
        Expected::String(s) => Value::string(s),
        Expected::List(items) => Value::List(
            items
                .iter()
                .map(to_value)
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        ),
        Expected::Map(m) => Value::Record(
            m.iter()
                .map(|(k, v)| Ok((Arc::from(k.as_str()), to_value(v)?)))
                .collect::<Result<Vec<_>, String>>()?
                .into(),
        ),
        Expected::Node { .. } | Expected::Edge { .. } => {
            return Err("a node or relationship cannot be a parameter".into());
        }
    })
}

/// The columns, rows and summary of a statement that succeeded.
type Produced<'a> = (&'a [String], &'a [Vec<Value>], Option<&'a Summary>);

/// The state of one scenario.
struct World<'a> {
    dir: &'a Path,
    data: &'a Path,
    db: Option<Database>,
    session: Option<Session>,
    params: Parameters,
    outcome: Option<Outcome>,
    procedures: Vec<(Signature, TableProcedure)>,
}

impl World<'_> {
    async fn session(&mut self) -> Result<&Session, String> {
        if self.db.is_none() {
            let db = Database::create(
                self.dir,
                no_flush(),
                CAP,
                OWNER,
                Options::new(Arc::new(AllowAll)),
            )
            .await
            .map_err(|e| format!("create: {e}"))?;
            self.db = Some(db);
        }
        let db = self.db.as_ref().expect("created above");
        for (sig, p) in self.procedures.drain(..) {
            db.register_procedure(sig, Arc::new(p))
                .map_err(|e| format!("register: {e}"))?;
        }
        if self.session.is_none() {
            self.session = Some(
                db.session(OWNER, None, None)
                    .await
                    .map_err(|e| format!("session: {e}"))?,
            );
        }
        Ok(self.session.as_ref().expect("opened above"))
    }

    /// Runs `gql` profiled, so that the number of its operators running at
    /// once is measured (NFR-PERF-007): more than one fails the scenario.
    /// PROFILE returns the same rows and counters as a plain run.
    async fn execute(&mut self, gql: &str) -> Result<Outcome, String> {
        let params = self.params.clone();
        let s = self.session().await?;
        let mut st = match s.profile(gql, params).await {
            Ok(st) => st,
            Err(e) => return Ok(Outcome::Failed(e)),
        };
        let columns = st.columns().to_vec();
        let mut rows = Vec::new();
        loop {
            match st.next().await {
                Ok(Some(r)) => rows.push(r),
                Ok(None) => break,
                Err(e) => return Ok(Outcome::Failed(e)),
            }
        }
        let running = st.max_running();
        PROFILED.fetch_add(1, Ordering::Relaxed);
        MAX_RUNNING.fetch_max(running, Ordering::Relaxed);
        if running > 1 {
            return Err(format!("{running} operators ran at once (NFR-PERF-007)"));
        }
        Ok(Outcome::Rows {
            columns,
            rows,
            summary: st.summary().cloned().map(Box::new),
        })
    }

    /// The labels and properties of the node or edge `v`, read from the
    /// latest committed state of whichever graph holds it.
    fn element(&self, v: &Value) -> Option<Element> {
        let (id, node) = match v {
            Value::Node(id) => (*id, true),
            Value::Edge(id) => (*id, false),
            _ => return None,
        };
        let e = self.db.as_ref()?.engine_handle();
        let mut tx = e.begin(Isolation::Snapshot).ok()?;
        let name = |t: u64| e.token_name(t).map(|(_, n)| n);
        for g in tx.graph_ids().ok()? {
            let graph = tx.graph(g).ok()?;
            let (labels, props) = if node {
                match tx.node(&graph, id).ok()? {
                    Some(r) => r,
                    None => continue,
                }
            } else {
                match tx.edge(&graph, id).ok()? {
                    Some(r) => (vec![r.ty], r.props),
                    None => continue,
                }
            };
            let labels = labels.into_iter().map(name).collect::<Option<Vec<_>>>()?;
            let props = props
                .into_iter()
                .map(|(k, v)| name(k).map(|k| (k, v)))
                .collect::<Option<BTreeMap<_, _>>>()?;
            return Some((labels, props));
        }
        None
    }

    fn rows(&self) -> Result<Produced<'_>, String> {
        match &self.outcome {
            Some(Outcome::Rows {
                columns,
                rows,
                summary,
            }) => Ok((columns, rows, summary.as_deref())),
            Some(Outcome::Failed(e)) => Err(format!("the statement failed: {e}")),
            None => Err("no statement was executed".into()),
        }
    }

    fn compare_table(&self, table: &[Vec<String>], ordered: bool) -> Result<(), String> {
        let (columns, rows, _) = self.rows()?;
        let (header, expected) = table.split_first().ok_or("an empty result table")?;
        if header.as_slice() != columns {
            return Err(format!("columns {columns:?}, expected {header:?}"));
        }
        let expected: Vec<Vec<Expected>> = expected
            .iter()
            .map(|r| r.iter().map(|c| value::parse(c)).collect())
            .collect::<Result<_, _>>()?;
        let mut element = |v: &Value| self.element(v);
        let same =
            |e: &[Expected], a: &[Value], element: &mut dyn FnMut(&Value) -> Option<Element>| {
                e.len() == a.len() && e.iter().zip(a).all(|(e, a)| value::equals(e, a, element))
            };
        let ok = if ordered {
            expected.len() == rows.len()
                && expected
                    .iter()
                    .zip(rows)
                    .all(|(e, a)| same(e, a, &mut element))
        } else {
            // A matching of expected rows to actual rows, one to one.
            let mut used = vec![false; rows.len()];
            expected.len() == rows.len()
                && expected.iter().all(|e| {
                    rows.iter().enumerate().any(|(i, a)| {
                        if !used[i] && same(e, a, &mut element) {
                            used[i] = true;
                            true
                        } else {
                            false
                        }
                    })
                })
        };
        if ok {
            Ok(())
        } else {
            Err(format!("rows {rows:?}, expected {expected:?}"))
        }
    }

    fn check_error(&self, wanted: &[&str]) -> Result<(), String> {
        match &self.outcome {
            Some(Outcome::Failed(e)) if wanted.contains(&e.code()) => Ok(()),
            Some(Outcome::Failed(e)) => Err(format!("failed with {e}, expected {wanted:?}")),
            Some(Outcome::Rows { rows, .. }) => {
                Err(format!("succeeded with {rows:?}, expected {wanted:?}"))
            }
            None => Err("no statement was executed".into()),
        }
    }

    fn check_effects(&self, expected: Counters) -> Result<(), String> {
        let (_, _, summary) = self.rows()?;
        let got = summary.ok_or("no summary")?.counters;
        if got == expected {
            Ok(())
        } else {
            Err(format!("side effects {got:?}, expected {expected:?}"))
        }
    }

    async fn step(&mut self, step: &Step) -> Result<(), String> {
        let text = step.text.as_str();
        let doc = || step.doc.as_deref().ok_or("the step has no docstring");
        match text {
            "an empty graph" | "any graph" | "the omitted graph" | "an empty catalog" => {
                self.session().await?;
            }
            "having executed:" | "having executed the program:" => {
                let gql = doc()?.to_string();
                if let Outcome::Failed(e) = self.execute(&gql).await? {
                    return Err(format!("setup failed: {e}"));
                }
            }
            "executing query:" | "executing the program:" | "executing control query:" => {
                let gql = doc()?.to_string();
                self.outcome = Some(self.execute(&gql).await?);
            }
            "parameters are:" => {
                for r in &step.table {
                    let [name, v] = r.as_slice() else {
                        return Err(format!("a parameter row {r:?}"));
                    };
                    self.params
                        .insert(name.clone(), to_value(&value::parse(v)?)?);
                }
            }
            "the result should be, in any order:" => self.compare_table(&step.table, false)?,
            "the result should be, in order:" => self.compare_table(&step.table, true)?,
            "the result should be empty" | "result should be empty" => {
                let (_, rows, _) = self.rows()?;
                if !rows.is_empty() {
                    return Err(format!("rows {rows:?}, expected none"));
                }
            }
            "no side effects" => self.check_effects(Counters::default())?,
            "the side effects should be:" => {
                let mut c = Counters::default();
                for r in &step.table {
                    let [k, n] = r.as_slice() else {
                        return Err(format!("a side-effect row {r:?}"));
                    };
                    add_effect(&mut c, k, n.parse().map_err(|_| "a count")?)?;
                }
                self.check_effects(c)?;
            }
            _ => return self.other_step(step).await,
        }
        Ok(())
    }

    async fn other_step(&mut self, step: &Step) -> Result<(), String> {
        let text = step.text.as_str();
        if let Some(spec) = text.strip_prefix("an exception condition should be raised: ") {
            // One code, or alternatives joined by ` or ` (gql-compat's
            // `also_gqlstatus`); `X caused by Y`: the code X with a cause
            // whose code is Y (lpg's own form, for a cause lpg decided).
            let (codes, cause) = match spec.split_once(" caused by ") {
                Some((codes, cause)) => (codes, Some(cause.trim())),
                None => (spec, None),
            };
            let codes: Vec<&str> = codes.split(" or ").map(str::trim).collect();
            self.check_error(&codes)?;
            return match (cause, &self.outcome) {
                (None, _) => Ok(()),
                (Some(want), Some(Outcome::Failed(e))) => match e.cause() {
                    Some(c) if c.code() == want => Ok(()),
                    Some(c) => Err(format!("failed with the cause {c}, expected {want}")),
                    None => Err(format!("failed with {e} without a cause, expected {want}")),
                },
                (Some(_), _) => Err("no statement failed".into()),
            };
        }
        if text == "the statement succeeds" {
            return self.rows().map(|_| ());
        }
        if let Some(sig) = text.strip_prefix("there exists a procedure ") {
            let (name, inputs, outputs) = parse_signature(sig)?;
            let (header, rows) = step.table.split_first().ok_or("no procedure table")?;
            let mut parsed = Vec::new();
            for r in rows {
                let vals: Vec<Value> = r
                    .iter()
                    .map(|c| value::parse(c).and_then(|e| to_value(&e)))
                    .collect::<Result<_, _>>()?;
                let (ins, outs) = vals.split_at(inputs.len());
                parsed.push((ins.to_vec(), outs.to_vec()));
            }
            if !header.iter().all(String::is_empty) && header.len() != inputs.len() + outputs.len()
            {
                return Err(format!("procedure table header {header:?}"));
            }
            let sig = Signature::new(
                &name,
                inputs.iter().map(|(n, _)| Parameter::new(n)).collect(),
                &outputs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            );
            self.procedures.push((
                sig,
                TableProcedure {
                    inputs,
                    rows: parsed,
                },
            ));
            if self.db.is_some() {
                self.session().await?;
            }
            return Ok(());
        }
        if let Some(rest) = text.strip_suffix(" catalog") {
            // opengql/tck ships no `data/catalogs/`; `catalog-1` is read as
            // the catalog that holds the schema its scenarios drop
            // (`drop1.feature` [1]).
            return match rest {
                "catalog-1" => {
                    if let Outcome::Failed(e) = self.execute("CREATE SCHEMA /myschema").await? {
                        return Err(format!("catalog-1: {e}"));
                    }
                    Ok(())
                }
                _ => Err(format!("unknown catalog {rest}")),
            };
        }
        if let Some(name) = text
            .strip_prefix("the ")
            .and_then(|t| t.strip_suffix(" graph"))
        {
            let path = self.data.join(format!("graphs/{name}-creator.gql"));
            let gql =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if let Outcome::Failed(e) = self.execute(&gql).await? {
                return Err(format!("graph {name}: {e}"));
            }
            return Ok(());
        }
        // `a <Type> should be raised at <phase>: <Detail>`.
        if let Some((_, detail)) = text
            .strip_prefix("a ")
            .filter(|t| t.contains(" should be raised at "))
            .and_then(|t| t.rsplit_once(": "))
        {
            let codes =
                detail_codes(detail.trim()).ok_or_else(|| format!("unmapped error {detail}"))?;
            return self.check_error(codes);
        }
        Err(format!("unknown step {text:?}"))
    }
}

/// Whether `e` is the "feature not supported" error of `code`: 42001 with
/// the cause 51N26 naming the code.
fn is_gate(e: &Error, code: &str) -> bool {
    e.code() == "42001"
        && e.cause()
            .is_some_and(|c| c.code() == "51N26" && c.message().contains(code))
}

/// Whether `e` has the code `code` and a cause with the code `cause`.
fn is_caused(e: &Error, code: &str, cause: &str) -> bool {
    e.code() == code && e.cause().is_some_and(|c| c.code() == cause)
}

/// What a scenario listed in `super::EXCEPTIONS` must do instead of its
/// own expectations: its statements run in order.
#[derive(Debug, Clone, Copy)]
pub(super) enum Instead {
    /// A statement fails with 42001 and the cause 51N26 naming the code.
    Gate(&'static str),
    /// A statement fails with this GQLSTATUS.
    Fails(&'static str),
    /// A statement fails with the first GQLSTATUS and a cause with the
    /// second.
    FailsCaused(&'static str, &'static str),
    /// Every statement succeeds.
    Succeeds,
}

/// Runs `scenario` with its database in `dir`; `data` is its artefact's
/// `data/` directory. `Err` names the failing step.
///
/// With `instead`, the scenario's expectations are not checked: its
/// statements run in order, and it passes when they do what `instead`
/// says.
pub(super) fn run(
    scenario: &Scenario,
    dir: &Path,
    data: &Path,
    instead: Option<Instead>,
) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a current_thread runtime");
    rt.block_on(async {
        let mut w = World {
            dir,
            data,
            db: None,
            session: None,
            params: Parameters::new(),
            outcome: None,
            procedures: Vec::new(),
        };
        let mut result = match instead {
            Some(Instead::Succeeds) | None => Ok(()),
            Some(i) => Err(format!("no statement failed as {i:?}")),
        };
        for (i, step) in scenario.steps.iter().enumerate() {
            let text = step.text.as_str();
            if let Some(want) = instead {
                if matches!(
                    text,
                    "having executed:"
                        | "having executed the program:"
                        | "executing query:"
                        | "executing the program:"
                ) {
                    let gql = step.doc.clone().unwrap_or_default();
                    match (w.execute(&gql).await, want) {
                        (Ok(Outcome::Failed(e)), Instead::Gate(code)) if is_gate(&e, code) => {
                            result = Ok(());
                            break;
                        }
                        (Ok(Outcome::Failed(e)), Instead::Fails(code)) if e.code() == code => {
                            result = Ok(());
                            break;
                        }
                        (Ok(Outcome::Failed(e)), Instead::FailsCaused(code, cause))
                            if is_caused(&e, code, cause) =>
                        {
                            result = Ok(());
                            break;
                        }
                        (Ok(Outcome::Failed(e)), _) => {
                            result = Err(format!("step {} failed with {e}", i + 1));
                            break;
                        }
                        (Ok(Outcome::Rows { .. }), _) => continue,
                        (Err(e), _) => {
                            result = Err(e);
                            break;
                        }
                    }
                }
                let is_expectation = text.starts_with("the result")
                    || text.starts_with("result should")
                    || text.contains("side effect")
                    || text.contains("should be raised");
                if is_expectation {
                    continue;
                }
            }
            if let Err(e) = w.step(step).await {
                result = Err(format!("step {} {:?}: {e}", i + 1, step.text));
                break;
            }
        }
        if let Some(db) = w.db.take() {
            drop(w.session.take());
            db.close().await.map_err(|e| format!("close: {e}"))?;
        }
        result
    })
}
