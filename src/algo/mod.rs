//! The graph-algorithm runtime ([TS-25]; rmp #63): one projection, one
//! block scheduler and one procedure binding for every entry of the
//! inventory.
//!
//! * **Entries.** Each algorithm is an [`Entry`] in [`ENTRIES`]: its
//!   procedure names, its keys as `algorithm-entries.md` gives them, its
//!   parallel, randomised and undirected-only flags, the keys that name
//!   the properties it projects, its output columns per mode, and its
//!   function. Registration ([`routines`]), the parameter record
//!   ([`config`]), the projection ([`projection`]), the scheduler
//!   ([`sched`]) and the modes below are shared, so an entry is a key
//!   table and a function from a [`Run`] to an [`Output`].
//! * **Calls.** A `CALL` and [`crate::Session::run_algorithm`] reach
//!   [`invoke`] through the same registry entry (FR-ALG-002). A run reads
//!   the caller's snapshot into a projection reserved from the memory cap
//!   ([`mem`]), computes, and releases the projection before it returns
//!   its rows.
//! * **Modes** (FR-ALG-003, FR-ALG-007). Only the stream and write modes
//!   exist, with no graph-name argument; any other mode name is no
//!   registered routine and fails with 42N08. Stream is a `ReadOnly`
//!   routine that returns one row per node. Write is a `ReadWrite` routine
//!   that sets the node property `writeProperty` through the caller's
//!   write set, which the memory cap bounds (FR-LIM-007), checks a closed
//!   graph type like a `SET`, and returns one summary row. Write modes
//!   are counted in the statement's `properties_set`.
//! * **Summary columns.** `nodePropertiesWritten`, `ranIterations`,
//!   `didConverge`, `configuration` (the parameter record), the
//!   `…Millis` times of the run's phases, and `centralityDistribution`:
//!   `min`, `mean`, `max` and the nearest-rank percentiles `p50`, `p75`,
//!   `p90`, `p95`, `p99`, `p999` of the non-NaN values, exact where GDS
//!   reports an HdrHistogram estimate (GDS @ `417d7ecf973d`
//!   `algo-common/.../result/HistogramUtils.java` `centralitySummary`,
//!   studied for the keys only).
//!
//! [TS-25]: ../../docs/technical-spec/algorithms.md

pub(crate) mod config;
pub(crate) mod mem;
mod pagerank;
pub(crate) mod projection;
pub(crate) mod scale;
pub(crate) mod sched;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Instant;

use crate::auth::Action;
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::proc::{Body, Parameter, Procs, ReadContext, Routine, Rows, Signature};
use crate::query::exec::{Cx, Held};
use crate::types::Value;
use config::{COMMON, Config, Key, NODE_LABELS, ORIENTATION, RELATIONSHIP_TYPES, SEED, WRITE};
use mem::Mem;
use projection::{Any, Orientation, Spec, with_projection};
use sched::Sched;

/// The stream columns of a node score (`algorithm-entries.md` Score).
pub(crate) const SCORE: &[&str] = &["nodeId", "score"];

/// The write columns of an iterative ranking (RankWrite).
pub(crate) const RANK_WRITE: &[&str] = &[
    "ranIterations",
    "didConverge",
    "centralityDistribution",
    "preProcessingMillis",
    "computeMillis",
    "postProcessingMillis",
    "writeMillis",
    "nodePropertiesWritten",
    "configuration",
];

/// An algorithm of the inventory (module documentation).
pub(crate) struct Entry {
    /// The inventory numbers the entry serves (ALG-043 is ALG-042 with
    /// `sourceNodes`).
    pub(crate) algs: &'static [u16],
    /// The stream procedure's name.
    pub(crate) stream: &'static str,
    /// The write procedure's name, if the entry has a write mode.
    pub(crate) write: Option<&'static str>,
    /// The entry's keys, after the common keys.
    pub(crate) keys: &'static [Key],
    /// Runs on up to `concurrency` workers (`algorithm-entries.md`
    /// Parallel classification).
    pub(crate) parallel: bool,
    /// Takes `randomSeed`.
    pub(crate) randomised: bool,
    /// Defined only for undirected graphs: always runs `UNDIRECTED`.
    pub(crate) undirected: bool,
    /// Keys whose string value names an edge property to project.
    pub(crate) edge_props: &'static [&'static str],
    /// Keys whose string value names a node property to project.
    pub(crate) node_props: &'static [&'static str],
    pub(crate) stream_columns: &'static [&'static str],
    pub(crate) write_columns: &'static [&'static str],
    /// The computation.
    pub(crate) run: fn(&mut Run<'_, '_>) -> Result<Output, Error>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (ALG-{:03?})", self.stream, self.algs)
    }
}

/// Every entry.
pub(crate) static ENTRIES: &[Entry] = &[pagerank::ENTRY];

/// The mode of an algorithm routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlgoMode {
    Stream,
    Write,
}

/// What an entry's function runs with.
pub(crate) struct Run<'r, 'm> {
    pub(crate) graph: &'r Any,
    pub(crate) cfg: &'r Config,
    pub(crate) sched: &'r Sched,
    pub(crate) mem: &'r mut Mem<'m>,
}

/// What an entry computes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Output {
    /// One float per projected node, by dense index; an iterative entry
    /// adds its iteration count and whether it converged.
    NodeFloats {
        values: Vec<f64>,
        ran_iterations: Option<u64>,
        did_converge: Option<bool>,
    },
}

/// The routines of every entry, for the registry.
pub(crate) fn routines() -> Vec<Routine> {
    let config = || Parameter::with_default("configuration", Value::Record(Vec::new().into()));
    let mut out = Vec::new();
    for e in ENTRIES {
        out.push(Routine {
            sig: Signature::new(e.stream, vec![config()], e.stream_columns),
            body: Body::Algo(e, AlgoMode::Stream),
        });
        if let Some(w) = e.write {
            out.push(Routine {
                sig: Signature::new(w, vec![config()], e.write_columns),
                body: Body::Algo(e, AlgoMode::Write),
            });
        }
    }
    out
}

/// The GQL statement and parameters with which the Rust API calls the
/// algorithm routine `name` (TS-25 Rust API): `CALL name($p0, …)`, each
/// argument the value `parameters` gives under the signature's parameter
/// name, or that parameter's default when a later one is given. The
/// statement runs through the same binder and registry entry as a `CALL`
/// (FR-ALG-002); a missing required parameter fails there.
///
/// # Errors
///
/// 42N08 if no algorithm routine is named `name`; 22N11 for a key of
/// `parameters` that is no parameter of the routine.
pub(crate) fn call(
    procs: &Procs,
    name: &str,
    mut parameters: crate::Parameters,
) -> Result<(String, crate::Parameters), Error> {
    let no_such = || Error::new(codes::C42N08, format!("no such procedure: {name}"));
    let routine = procs.get(name).ok_or_else(no_such)?;
    let r = routine.get();
    if !matches!(r.body, Body::Algo(..)) {
        return Err(no_such());
    }
    if let Some(k) = parameters
        .keys()
        .find(|k| !r.sig.params.iter().any(|p| p.name == **k))
    {
        return Err(Error::new(
            codes::C22N11,
            format!("invalid argument: the procedure {name} has no parameter {k}"),
        ));
    }
    let given = r
        .sig
        .params
        .iter()
        .rposition(|p| parameters.contains_key(&p.name))
        .map_or(0, |i| i + 1);
    let mut args = crate::Parameters::new();
    let mut list = Vec::with_capacity(given);
    for (i, p) in r.sig.params[..given].iter().enumerate() {
        let v = parameters
            .remove(&p.name)
            .or_else(|| p.default.clone())
            .unwrap_or(Value::Null);
        args.insert(format!("p{i}"), v);
        list.push(format!("$p{i}"));
    }
    let quoted: Vec<String> = name
        .split('.')
        .map(|part| format!("`{}`", part.replace('`', "``")))
        .collect();
    Ok((
        format!("CALL {}({})", quoted.join("."), list.join(", ")),
        args,
    ))
}

/// The actions a run performs (TS-23): it traverses and reads the
/// working graph, and a write mode sets properties in it.
pub(crate) fn requests(mode: AlgoMode) -> Vec<(Action, bool)> {
    let mut r = vec![(Action::Traverse, false), (Action::Read, false)];
    if mode == AlgoMode::Write {
        r.push((Action::SetProperty, false));
    }
    r
}

/// The keys of `e` in `mode`: the common keys, `randomSeed`, the entry's,
/// then `writeProperty`.
fn keys(e: &Entry, mode: AlgoMode) -> Vec<Key> {
    let mut k = COMMON.to_vec();
    if e.randomised {
        k.push(SEED);
    }
    k.extend_from_slice(e.keys);
    if mode == AlgoMode::Write {
        k.push(WRITE);
    }
    k
}

/// `None` for `["*"]`, else the names.
fn filter(cfg: &Config, key: &str) -> Option<Vec<String>> {
    let names = cfg.names(key);
    (!names.contains(&"*")).then(|| names.into_iter().map(str::to_string).collect())
}

/// Elapsed milliseconds since `t`.
fn millis(t: Instant) -> Value {
    Value::Int(i64::try_from(t.elapsed().as_millis()).unwrap_or(i64::MAX))
}

/// Runs entry `e` in `mode` with `args` (module documentation).
///
/// # Errors
///
/// 22N11 and 22G03 for the configuration; 51N72 and 51N73 when the memory
/// refuses; 25N14 once the statement is stopped; the entry's and storage
/// errors.
pub(crate) fn invoke(
    e: &'static Entry,
    mode: AlgoMode,
    cx: &mut ReadContext<'_>,
    args: &[Value],
) -> Result<Rows, Error> {
    let started = Instant::now();
    let mut cfg = Config::parse(&args[0], &keys(e, mode))?;
    if e.undirected {
        cfg.set(ORIENTATION, Value::string("UNDIRECTED"));
    }
    let spec = Spec {
        labels: filter(&cfg, NODE_LABELS),
        types: filter(&cfg, RELATIONSHIP_TYPES),
        edge_props: e
            .edge_props
            .iter()
            .filter_map(|k| cfg.str(k).map(str::to_string))
            .collect(),
        node_props: e
            .node_props
            .iter()
            .filter_map(|k| cfg.str(k).map(str::to_string))
            .collect(),
    };
    let g = cx.st.graphs[cx.graph];
    let mut mem = Mem::new(cx.tx, cx.st);
    let graph = projection::build(&mut mem, &g, &spec, Orientation::of(cfg.str(ORIENTATION)))?;
    let n = with_projection!(&graph, p => p.node_count());
    #[allow(unused_mut, reason = "the probe key is set under test only")]
    let mut sched = Sched::new(n, cfg.concurrency(), e.parallel, mem.check());
    #[cfg(test)]
    {
        sched.probe = Some(mem.tx.engine().core().location.display().to_string());
    }
    let pre = millis(started);
    let computing = Instant::now();
    let out = (e.run)(&mut Run {
        graph: &graph,
        cfg: &cfg,
        sched: &sched,
        mem: &mut mem,
    })?;
    let compute = millis(computing);
    let ids = match graph {
        Any::Small(p) => p.ids,
        Any::Large(p) => p.ids,
    };
    let Output::NodeFloats {
        values,
        ran_iterations,
        did_converge,
    } = out;
    match mode {
        AlgoMode::Stream => {
            let held = mem.finish(n * (size_of::<u64>() + size_of::<f64>()))?;
            Ok(stream_rows(ids, values, held))
        }
        AlgoMode::Write => {
            let _arrays = mem.finish(n * (size_of::<u64>() + size_of::<f64>()))?;
            let writing = Instant::now();
            let written = write_floats(cx, &g, &cfg, &ids, &values)?;
            let summary = Summary {
                written,
                ran_iterations,
                did_converge,
                values: &values,
                pre,
                compute,
                write: millis(writing),
                config: cfg.record(),
            };
            let row = e
                .write_columns
                .iter()
                .map(|c| summary.column(c))
                .collect::<Vec<_>>();
            Ok(Box::new(std::iter::once(Ok(row))))
        }
    }
}

/// The stream rows of one float per node: `(nodeId, value)`, in element
/// identifier order. `held` is the reservation of the two arrays, released
/// with the rows.
fn stream_rows(ids: Vec<u64>, values: Vec<f64>, held: Held) -> Rows {
    Box::new(ids.into_iter().zip(values).map(move |(id, x)| {
        let _ = &held;
        Ok(vec![Value::Int(id.cast_signed()), Value::Float(x)])
    }))
}

/// Sets property `writeProperty` of each projected node to its value,
/// through the caller's write set; returns the number of nodes written.
fn write_floats(
    cx: &mut ReadContext<'_>,
    g: &crate::storage::graph::Graph,
    cfg: &Config,
    ids: &[u64],
    values: &[f64],
) -> Result<u64, Error> {
    let name = cfg.str(config::WRITE_PROPERTY).unwrap_or_default();
    let key = cx.tx.token(TokenKind::PropertyKey, name)?;
    let mut written = Vec::new();
    for (i, (id, x)) in ids.iter().zip(values).enumerate() {
        if i % crate::query::exec::CHECK_EVERY == 0 {
            cx.check()?;
        }
        let Some((labels, mut props)) = cx.tx.node(g, *id)? else {
            continue;
        };
        props.retain(|(k, _)| *k != key);
        props.push((key, Value::Float(*x)));
        cx.tx.set_node(g, *id, &labels, &props)?;
        cx.st.counters.properties_set += 1;
        written.push(Value::Node(*id));
    }
    if let Some(gt) = cx.st.graph_types[cx.graph].clone() {
        let mut c = Cx {
            tx: cx.tx,
            st: cx.st,
        };
        crate::query::exec::write::check_closed(&mut c, g, &gt, &written)?;
    }
    Ok(written.len() as u64)
}

/// What a write mode's summary row is built from.
struct Summary<'a> {
    written: u64,
    ran_iterations: Option<u64>,
    did_converge: Option<bool>,
    values: &'a [f64],
    pre: Value,
    compute: Value,
    write: Value,
    config: Value,
}

impl Summary<'_> {
    /// The value of summary column `c` (module documentation).
    fn column(&self, c: &str) -> Value {
        let int = |x: u64| Value::Int(i64::try_from(x).unwrap_or(i64::MAX));
        match c {
            "nodePropertiesWritten" => int(self.written),
            "ranIterations" => self.ran_iterations.map_or(Value::Null, int),
            "didConverge" => self.did_converge.map_or(Value::Null, Value::Bool),
            "centralityDistribution" => distribution(self.values),
            "preProcessingMillis" => self.pre.clone(),
            "computeMillis" => self.compute.clone(),
            "postProcessingMillis" => Value::Int(0),
            "writeMillis" => self.write.clone(),
            "configuration" => self.config.clone(),
            other => panic!("no summary value for the column {other}"),
        }
    }
}

/// `centralityDistribution` of `values` (module documentation).
fn distribution(values: &[f64]) -> Value {
    let mut xs: Vec<f64> = values.iter().copied().filter(|x| !x.is_nan()).collect();
    let field = |k: &str, v: f64| (Arc::<str>::from(k), Value::Float(v));
    if xs.is_empty() {
        return Value::Record(Vec::new().into());
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    xs.sort_unstable_by(f64::total_cmp);
    let at = |p: f64| {
        let rank = (p / 100.0 * xs.len() as f64).ceil() as usize;
        xs[rank.clamp(1, xs.len()) - 1]
    };
    Value::Record(
        vec![
            field("min", xs[0]),
            field("mean", mean),
            field("max", xs[xs.len() - 1]),
            field("p50", at(50.0)),
            field("p75", at(75.0)),
            field("p90", at(90.0)),
            field("p95", at(95.0)),
            field("p99", at(99.0)),
            field("p999", at(99.9)),
        ]
        .into(),
    )
}
