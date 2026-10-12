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
//! * **Row outputs** (rmp #64, #69). An entry whose result is not one
//!   float per node returns [`Output::Rows`]: its stream rows, each
//!   charged to the run's reservation as it is produced ([`Run::emit`]),
//!   so an output larger than the memory cap fails the statement with
//!   51N72 or 51N73 (FR-LIM-001); the reservation then shrinks to the
//!   rows' share, which is released with the last row. Its write mode
//!   writes what the rows describe ([`WriteKind`]): node properties,
//!   relationships of type `writeRelationshipType` (PathWrite, RelWrite)
//!   or a flag on edges (EdgePropWrite).
//! * **MAGE entries** take MAGE's positional parameters ([`Arg`]) before
//!   the `configuration` record (TS-25 Names and signatures).
//!
//! [TS-25]: ../../docs/technical-spec/algorithms.md

/// Records an operation of an entry in the test trace ([`trace`]):
/// `trace!(kind, format, args…)`. Under `cfg(test)` only; in normal and
/// release builds it expands to nothing.
#[cfg(test)]
macro_rules! trace {
    ($kind:expr, $($arg:tt)*) => {
        if $crate::algo::trace::on() {
            $crate::algo::trace::push($kind, &format!($($arg)*));
        }
    };
}

/// The release form of the trace macro: nothing.
#[cfg(not(test))]
macro_rules! trace {
    ($($arg:tt)*) => {};
}

mod blossom;
mod centrality;
mod clustering;
mod community;
mod components;
pub(crate) mod config;
mod cycles;
mod flow;
mod functions;
mod kernels;
mod matching;
mod measures;
pub(crate) mod mem;
mod pagerank;
mod paths;
pub(crate) mod projection;
pub(crate) mod scale;
pub(crate) mod sched;
mod similarity;
mod structure;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_centrality;
#[cfg(test)]
mod tests_community;
#[cfg(test)]
mod tests_components;
#[cfg(test)]
mod tests_flow;
#[cfg(test)]
mod tests_paths;
#[cfg(test)]
mod tests_structure;
#[cfg(test)]
mod tests_trace_a;
#[cfg(test)]
pub(crate) mod trace;
mod traversal;
mod trees;

use std::sync::Arc;
use std::time::Instant;

use crate::auth::Action;
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::proc::{Body, Parameter, Procs, ReadContext, Routine, Rows, Signature};
use crate::query::exec::{Cx, Held};
use crate::types::Value;
use config::{
    COMMON, Config, Key, NODE_LABELS, NODE_WRITE, ORIENTATION, RELATIONSHIP_TYPES, SEED,
    WRITE_RELATIONSHIP_TYPE,
};
use mem::Mem;
#[cfg(test)]
pub(crate) use paths::haversine_nm;
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

/// The stream columns of a path result (`algorithm-entries.md` PathRows).
pub(crate) const PATH_ROWS: &[&str] = &[
    "index",
    "sourceNode",
    "targetNode",
    "totalCost",
    "nodeIds",
    "costs",
    "path",
];

/// The write columns of a path result (PathWrite).
pub(crate) const PATH_WRITE: &[&str] = &[
    "preProcessingMillis",
    "computeMillis",
    "postProcessingMillis",
    "writeMillis",
    "relationshipsWritten",
    "configuration",
];

/// The write columns of a derived node-property write (NodeWrite).
pub(crate) const NODE_WRITE_COLUMNS: &[&str] = &["nodePropertiesWritten", "configuration"];

/// The write columns of a derived relationship write (RelWrite).
pub(crate) const REL_WRITE_COLUMNS: &[&str] = &["relationshipsWritten", "configuration"];

/// The write columns of a derived edge-property write (EdgePropWrite).
pub(crate) const EDGE_PROP_WRITE_COLUMNS: &[&str] =
    &["relationshipPropertiesWritten", "configuration"];

/// A positional parameter of a MAGE entry, before `configuration`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Arg {
    pub(crate) name: &'static str,
    pub(crate) default: ArgDefault,
}

impl Arg {
    pub(crate) const fn new(name: &'static str, default: ArgDefault) -> Arg {
        Arg { name, default }
    }
}

/// The default of a positional parameter.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgDefault {
    Required,
    Null,
    Str(&'static str),
    Float(f64),
    Int(i64),
}

/// What a write mode writes, read from the stream rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteKind {
    /// Node property `writeProperty` of the node in column `id` is set to
    /// column `value`.
    NodeProp { id: usize, value: usize },
    /// One relationship of type `writeRelationshipType` from
    /// `sourceNode` to `targetNode` per PathRows row, with `totalCost`,
    /// and `nodeIds` and `costs` when `writeNodeIds` and `writeCosts`
    /// are true. A row whose column `cycle` is true is written only when
    /// `writeNegativeCycles` is true.
    PathRels { cycle: Option<usize> },
    /// One relationship of type `writeRelationshipType` per row, from the
    /// node in column `src` to the node in column `dst`.
    Rels { src: usize, dst: usize },
    /// Edge property `writeProperty` of the edge in column `edge` is set
    /// to true.
    EdgeFlag { edge: usize },
    /// Each field of the record in column `map` is set as the node
    /// property of its name on the node in column `id` (HITS's
    /// `authProperty` and `hubProperty`).
    NodeMap { id: usize, map: usize },
    /// Node property `writeProperty` of the node in column `id` is set to
    /// field `field` of the record in column `map` (SLLPA's
    /// `communityIds`).
    NodeField {
        id: usize,
        map: usize,
        field: &'static str,
    },
    /// One relationship of type `writeRelationshipType` per row, from the
    /// node in column `src` to the node in column `dst`, with property
    /// `writeProperty` set to column `value` (node similarity, KNN).
    RelsProp {
        src: usize,
        dst: usize,
        value: usize,
    },
    /// Node property `writeProperty` of each node in the list in column
    /// `ids` is set to column `value` (the condensation's `componentId`).
    NodeList { ids: usize, value: usize },
}

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
    /// Keys whose value names node properties to project as vectors (a
    /// string, a list of strings or a [`config::Ty::Props`] record).
    pub(crate) vector_props: &'static [&'static str],
    pub(crate) stream_columns: &'static [&'static str],
    pub(crate) write_columns: &'static [&'static str],
    /// The computation.
    pub(crate) run: fn(&mut Run<'_, '_>) -> Result<Output, Error>,
    /// MAGE's positional parameters, before `configuration`.
    pub(crate) args: &'static [Arg],
    /// The positional parameters whose string value names an edge
    /// property to project, after those of `edge_props`.
    pub(crate) edge_args: &'static [usize],
    /// Node properties projected by name, after those of `node_props`.
    pub(crate) named_node_props: &'static [&'static str],
    /// Projects each edge's type and each node's labels.
    pub(crate) edge_types: bool,
    pub(crate) node_labels: bool,
    /// Projects a second graph from `otherNodeLabels` and
    /// `otherRelationshipTypes` ([`Run::other`]).
    pub(crate) second: bool,
    /// The keys of the write mode.
    pub(crate) write_keys: &'static [Key],
    pub(crate) write_kind: WriteKind,
}

impl Entry {
    /// The fields an entry leaves at their usual value.
    pub(crate) const BASE: Entry = Entry {
        algs: &[],
        stream: "",
        write: None,
        keys: &[],
        parallel: false,
        randomised: false,
        undirected: false,
        edge_props: &[],
        node_props: &[],
        vector_props: &[],
        stream_columns: &[],
        write_columns: &[],
        run: no_run,
        args: &[],
        edge_args: &[],
        named_node_props: &[],
        edge_types: false,
        node_labels: false,
        second: false,
        write_keys: NODE_WRITE,
        write_kind: WriteKind::NodeProp { id: 0, value: 1 },
    };
}

/// The function of [`Entry::BASE`], replaced by every entry.
fn no_run(_: &mut Run<'_, '_>) -> Result<Output, Error> {
    Err(Error::new(
        codes::C50N00,
        "internal error: an entry without a function",
    ))
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (ALG-{:03?})", self.stream, self.algs)
    }
}

/// Every entry.
pub(crate) static ENTRIES: &[Entry] = &[
    pagerank::ENTRY,
    traversal::BFS,
    traversal::DFS,
    traversal::AT_HOP,
    traversal::BY_HOP,
    traversal::PATH_EXPAND,
    traversal::PATH_SUBGRAPH_NODES,
    traversal::PATH_SUBGRAPH_ALL,
    traversal::RANDOM_WALK,
    traversal::TOPOLOGICAL_SORT,
    traversal::ANCESTORS,
    traversal::DESCENDANTS,
    traversal::HAS_PATH,
    traversal::DOMINATORS,
    paths::UNWEIGHTED,
    paths::DIJKSTRA,
    paths::DIJKSTRA_ALL,
    paths::BIDIRECTIONAL,
    paths::ASTAR,
    paths::BELLMAN_FORD,
    paths::NEGATIVE_CYCLE,
    paths::SPFA,
    paths::DELTA,
    paths::JOHNSON,
    paths::FLOYD_WARSHALL,
    paths::SEIDEL,
    paths::ALL_PAIRS,
    paths::ALL_SHORTEST_BETWEEN,
    paths::YENS,
    paths::K_SHORTEST,
    paths::ALL_SIMPLE_PATHS,
    paths::LONGEST_PATH,
    cycles::CYCLES,
    cycles::CIRCUITS,
    cycles::TSP,
    cycles::VRP,
    structure::IS_ISOMORPHIC,
    structure::IS_ISOMORPHIC_SUBGRAPH,
    structure::BIPARTITE,
    structure::TOURNAMENT,
    structure::GRAPHLIKE,
    structure::CLOSURE,
    structure::REDUCTION,
    centrality::DEGREE,
    centrality::BETWEENNESS,
    centrality::CLOSENESS,
    centrality::HARMONIC,
    pagerank::EIGENVECTOR,
    centrality::KATZ,
    pagerank::ARTICLE_RANK,
    centrality::HITS,
    centrality::CELF,
    similarity::NODE_SIMILARITY,
    similarity::NODE_SIMILARITY_FILTERED,
    similarity::KNN,
    similarity::KNN_FILTERED,
    community::LOUVAIN,
    community::LEIDEN,
    community::LABEL_PROPAGATION,
    community::SLLPA,
    community::MODULARITY_OPTIMIZATION,
    community::MAX_K_CUT,
    community::MODULARITY,
    community::CONDUCTANCE,
    clustering::SPECTRAL,
    clustering::BALANCED_CUT,
    clustering::KMEANS,
    clustering::HDBSCAN,
    components::WCC,
    components::CONNECTED,
    components::SCC,
    components::CONDENSATION,
    components::BICONNECTED,
    components::ARTICULATION_POINTS,
    components::BRIDGES,
    components::KCORE,
    measures::TRIANGLE_COUNT,
    measures::TRIANGLES,
    measures::LOCAL_CLUSTERING,
    measures::ECCENTRICITY,
    measures::GIRTH,
    measures::LAPLACIAN,
    trees::SPANNING_TREE,
    trees::K_SPANNING_TREE,
    trees::COUNT_SPANNING_TREES,
    trees::STEINER_TREE,
    trees::PRIZE_STEINER_TREE,
    flow::MAX_FLOW,
    flow::MIN_COST_FLOW,
    matching::MAXIMUM_MATCHING,
    matching::BIPARTITE_MATCHING,
    matching::MAXIMAL_CLIQUES,
    matching::CLIQUE_COUNTING,
    matching::GREEDY_COLORING,
    matching::DSATUR,
    matching::K1_COLORING,
    matching::FEEDBACK_ARC_SET,
    matching::SET_COVER,
];

/// The mode of an algorithm routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlgoMode {
    Stream,
    Write,
}

/// What an entry's function runs with.
pub(crate) struct Run<'r, 'm> {
    /// The mode of the call: a write mode may write rows the stream mode
    /// does not return (every node's flag of ALG-090, no root row for a
    /// tree's relationships).
    pub(crate) mode: AlgoMode,
    pub(crate) graph: &'r Any,
    pub(crate) cfg: &'r Config,
    pub(crate) sched: &'r Sched,
    pub(crate) mem: &'r mut Mem<'m>,
    /// The positional arguments ([`Entry::args`]).
    pub(crate) args: &'r [Value],
    /// The second projection ([`Entry::second`]).
    pub(crate) other: Option<&'r Any>,
}

impl Run<'_, '_> {
    /// Appends `row` to `rows`, charging it to the run's reservation
    /// first (module documentation).
    ///
    /// # Errors
    ///
    /// As [`Mem::grow`].
    pub(crate) fn emit(
        &mut self,
        rows: &mut Vec<Vec<Value>>,
        row: Vec<Value>,
    ) -> Result<(), Error> {
        let bytes = crate::query::exec::row_bytes(&row) as usize;
        self.mem.grow(bytes - size_of::<Vec<Value>>())?;
        self.mem.push(rows, row)
    }

    /// The token of name `name` of `kind`, `None` if no element has it.
    pub(crate) fn token(&self, kind: TokenKind, name: &str) -> Option<u64> {
        self.mem.tx.engine().token_id(kind, name)
    }
}

/// The bytes [`Run::emit`] charged for `rows`.
fn rows_bytes(rows: &[Vec<Value>]) -> usize {
    rows.iter()
        .map(|r| crate::query::exec::row_bytes(r) as usize - size_of::<Vec<Value>>())
        .sum::<usize>()
        + std::mem::size_of_val(rows)
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
    /// The stream rows, and the values of the write mode's columns that
    /// the rows do not give (`containsNegativeCycle`, `bipartite`).
    Rows {
        rows: Vec<Vec<Value>>,
        extra: Vec<(&'static str, Value)>,
    },
}

/// The routines of every entry, for the registry.
pub(crate) fn routines() -> Vec<Routine> {
    let params = |e: &Entry| {
        let mut ps: Vec<Parameter> = e
            .args
            .iter()
            .map(|a| match a.default {
                ArgDefault::Required => Parameter::new(a.name),
                ArgDefault::Null => Parameter::with_default(a.name, Value::Null),
                ArgDefault::Str(s) => Parameter::with_default(a.name, Value::string(s)),
                ArgDefault::Float(f) => Parameter::with_default(a.name, Value::Float(f)),
                ArgDefault::Int(i) => Parameter::with_default(a.name, Value::Int(i)),
            })
            .collect();
        ps.push(Parameter::with_default(
            "configuration",
            Value::Record(Vec::new().into()),
        ));
        ps
    };
    let mut out = Vec::new();
    for e in ENTRIES {
        // An entry with no stream mode (ALG-149) has no stream name.
        if !e.stream.is_empty() {
            out.push(Routine {
                sig: Signature::new(e.stream, params(e), e.stream_columns),
                body: Body::Algo(e, AlgoMode::Stream),
            });
        }
        if let Some(w) = e.write {
            out.push(Routine {
                sig: Signature::new(w, params(e), e.write_columns),
                body: Body::Algo(e, AlgoMode::Write),
            });
        }
    }
    out.extend(functions::routines());
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
    if !matches!(r.body, Body::Algo(..)) && !functions::is_function(name) {
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
/// working graph, and a write mode sets properties in it, and creates
/// relationships when it writes them.
pub(crate) fn requests(e: &Entry, mode: AlgoMode) -> Vec<(Action, bool)> {
    let mut r = vec![(Action::Traverse, false), (Action::Read, false)];
    if mode == AlgoMode::Write {
        if matches!(
            e.write_kind,
            WriteKind::PathRels { .. } | WriteKind::Rels { .. } | WriteKind::RelsProp { .. }
        ) {
            r.push((Action::Create, false));
        }
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
        k.extend_from_slice(e.write_keys);
    }
    k
}

/// The node properties key `key` names: a string, a list of strings, or
/// the names of a [`config::Ty::Props`] record.
fn vector_names(cfg: &Config, key: &str) -> Vec<String> {
    match cfg.get(key) {
        Value::String(s) => vec![s.to_string()],
        Value::List(_) => cfg.names(key).into_iter().map(str::to_string).collect(),
        Value::Record(_) => cfg.props(key).into_iter().map(|(n, _)| n).collect(),
        _ => Vec::new(),
    }
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
    let positional = &args[..e.args.len()];
    let mut cfg = Config::parse(&args[e.args.len()], &keys(e, mode))?;
    if e.undirected {
        cfg.set(ORIENTATION, Value::string("UNDIRECTED"));
    }
    let mut node_props: Vec<String> = e
        .node_props
        .iter()
        .filter_map(|k| cfg.str(k).map(str::to_string))
        .collect();
    node_props.extend(e.named_node_props.iter().map(|p| (*p).to_string()));
    let spec = Spec {
        labels: filter(&cfg, NODE_LABELS),
        types: filter(&cfg, RELATIONSHIP_TYPES),
        edge_props: e
            .edge_props
            .iter()
            .filter_map(|k| cfg.str(k).map(str::to_string))
            .chain(e.edge_args.iter().filter_map(|i| match &positional[*i] {
                Value::String(s) => Some(s.to_string()),
                _ => None,
            }))
            .collect(),
        node_props,
        edge_types: e.edge_types,
        node_labels: e.node_labels,
        vector_props: e
            .vector_props
            .iter()
            .flat_map(|k| vector_names(&cfg, k))
            .collect(),
    };
    let g = cx.st.graphs[cx.graph];
    let mut mem = Mem::new(cx.tx, cx.st);
    let orientation = Orientation::of(cfg.str(ORIENTATION));
    let graph = projection::build(&mut mem, &g, &spec, orientation)?;
    let other = if e.second {
        let other = Spec {
            labels: filter(&cfg, "otherNodeLabels"),
            types: filter(&cfg, "otherRelationshipTypes"),
            ..Spec::default()
        };
        Some(projection::build(&mut mem, &g, &other, orientation)?)
    } else {
        None
    };
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
        mode,
        graph: &graph,
        cfg: &cfg,
        sched: &sched,
        mem: &mut mem,
        args: positional,
        other: other.as_ref(),
    })?;
    let compute = millis(computing);
    drop(other);
    let ids = match graph {
        Any::Small(p) => p.ids,
        Any::Large(p) => p.ids,
    };
    let (rows, extra) = match out {
        Output::NodeFloats {
            values,
            ran_iterations,
            did_converge,
        } => {
            let held = mem.finish(ids.len() * (size_of::<u64>() + size_of::<f64>()))?;
            return node_floats(
                e,
                mode,
                cx,
                held,
                &g,
                &cfg,
                ids,
                values,
                (ran_iterations, did_converge),
                (pre, compute),
            );
        }
        Output::Rows { rows, extra } => (rows, extra),
    };
    drop(ids);
    let held = mem.keep(rows_bytes(&rows));
    match mode {
        AlgoMode::Stream => Ok(Box::new(rows.into_iter().map(move |row| {
            let _ = &held;
            Ok(row)
        }))),
        AlgoMode::Write => {
            let writing = Instant::now();
            let written = write_rows(cx, &g, &cfg, e.write_kind, &rows)?;
            drop(rows);
            drop(held);
            let summary = Summary {
                written: written.0,
                rels_written: written.1,
                edge_props_written: written.2,
                ran_iterations: None,
                did_converge: None,
                values: &[],
                pre,
                compute,
                write: millis(writing),
                config: cfg.record(),
                extra: &extra,
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

/// The rows of an entry with one float per node (PageRank's).
#[allow(
    clippy::too_many_arguments,
    reason = "the parts of one run, handed over once"
)]
fn node_floats(
    e: &Entry,
    mode: AlgoMode,
    cx: &mut ReadContext<'_>,
    held: Held,
    g: &crate::storage::graph::Graph,
    cfg: &Config,
    ids: Vec<u64>,
    values: Vec<f64>,
    (ran_iterations, did_converge): (Option<u64>, Option<bool>),
    (pre, compute): (Value, Value),
) -> Result<Rows, Error> {
    match mode {
        AlgoMode::Stream => Ok(stream_rows(ids, values, held)),
        AlgoMode::Write => {
            let _arrays = held;
            let writing = Instant::now();
            let pairs: Vec<(u64, Value)> = ids
                .iter()
                .zip(&values)
                .map(|(id, x)| (*id, Value::Float(*x)))
                .collect();
            let written = write_node_values(cx, g, cfg, &pairs)?;
            let summary = Summary {
                written,
                rels_written: 0,
                edge_props_written: 0,
                ran_iterations,
                did_converge,
                values: &values,
                pre,
                compute,
                write: millis(writing),
                config: cfg.record(),
                extra: &[],
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

/// Sets property `writeProperty` of each node of `pairs` to its value,
/// through the caller's write set; returns the number of nodes written.
fn write_node_values(
    cx: &mut ReadContext<'_>,
    g: &crate::storage::graph::Graph,
    cfg: &Config,
    pairs: &[(u64, Value)],
) -> Result<u64, Error> {
    let name = cfg.str(config::WRITE_PROPERTY).unwrap_or_default();
    let key = cx.tx.token(TokenKind::PropertyKey, name)?;
    let mut written = Vec::new();
    for (i, (id, x)) in pairs.iter().enumerate() {
        if i % crate::query::exec::CHECK_EVERY == 0 {
            cx.check()?;
        }
        let Some((labels, mut props)) = cx.tx.node(g, *id)? else {
            continue;
        };
        props.retain(|(k, _)| *k != key);
        props.push((key, x.clone()));
        cx.tx.set_node(g, *id, &labels, &props)?;
        cx.st.counters.properties_set += 1;
        written.push(Value::Node(*id));
    }
    check_closed(cx, g, &written)?;
    Ok(written.len() as u64)
}

/// Checks a closed graph type against the elements a write mode wrote,
/// as a `SET` or `INSERT` checks it.
fn check_closed(
    cx: &mut ReadContext<'_>,
    g: &crate::storage::graph::Graph,
    written: &[Value],
) -> Result<(), Error> {
    if let Some(gt) = cx.st.graph_types[cx.graph].clone() {
        let mut c = Cx {
            tx: cx.tx,
            st: cx.st,
        };
        crate::query::exec::write::check_closed(&mut c, g, &gt, written)?;
    }
    Ok(())
}

/// The node element identifier in a row's column.
fn row_node(v: &Value) -> Option<u64> {
    match v {
        Value::Int(i) => Some(i.cast_unsigned()),
        Value::Node(id) => Some(*id),
        _ => None,
    }
}

/// Writes what `rows` describe (module documentation); returns the node
/// properties, relationships and relationship properties written.
fn write_rows(
    cx: &mut ReadContext<'_>,
    g: &crate::storage::graph::Graph,
    cfg: &Config,
    kind: WriteKind,
    rows: &[Vec<Value>],
) -> Result<(u64, u64, u64), Error> {
    match kind {
        WriteKind::NodeMap { id, map } => {
            let mut written = 0;
            let mut nodes = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                if i % crate::query::exec::CHECK_EVERY == 0 {
                    cx.check()?;
                }
                let (Some(node), Value::Record(fields)) = (row_node(&r[id]), &r[map]) else {
                    continue;
                };
                let Some((labels, mut props)) = cx.tx.node(g, node)? else {
                    continue;
                };
                for (k, v) in fields.iter() {
                    let key = cx.tx.token(TokenKind::PropertyKey, k)?;
                    props.retain(|(p, _)| *p != key);
                    props.push((key, v.clone()));
                    cx.st.counters.properties_set += 1;
                    written += 1;
                }
                cx.tx.set_node(g, node, &labels, &props)?;
                nodes.push(Value::Node(node));
            }
            check_closed(cx, g, &nodes)?;
            Ok((written, 0, 0))
        }
        WriteKind::NodeField { id, map, field } => {
            let pairs: Vec<(u64, Value)> = rows
                .iter()
                .filter_map(|r| {
                    let Value::Record(fields) = &r[map] else {
                        return None;
                    };
                    let v = fields.iter().find(|(k, _)| &**k == field)?.1.clone();
                    Some((row_node(&r[id])?, v))
                })
                .collect();
            Ok((write_node_values(cx, g, cfg, &pairs)?, 0, 0))
        }
        WriteKind::RelsProp { src, dst, value } => {
            let name = cfg.str(WRITE_RELATIONSHIP_TYPE).unwrap_or_default();
            let ty = cx.tx.token(TokenKind::EdgeType, name)?;
            let prop = cfg.str(config::WRITE_PROPERTY).unwrap_or_default();
            let key = cx.tx.token(TokenKind::PropertyKey, prop)?;
            let mut written = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                if i % crate::query::exec::CHECK_EVERY == 0 {
                    cx.check()?;
                }
                let (Some(a), Some(b)) = (row_node(&r[src]), row_node(&r[dst])) else {
                    continue;
                };
                let id = cx.tx.create_edge(g, a, b, ty, &[(key, r[value].clone())])?;
                cx.st.counters.edges_created += 1;
                cx.st.counters.properties_set += 1;
                written.push(Value::Edge(id));
            }
            check_closed(cx, g, &written)?;
            Ok((0, written.len() as u64, 0))
        }
        WriteKind::NodeList { ids, value } => {
            let mut pairs: Vec<(u64, Value)> = Vec::new();
            for r in rows {
                if let Value::List(l) = &r[ids] {
                    pairs.extend(
                        l.iter()
                            .filter_map(|v| Some((row_node(v)?, r[value].clone()))),
                    );
                }
            }
            Ok((write_node_values(cx, g, cfg, &pairs)?, 0, 0))
        }
        WriteKind::NodeProp { id, value } => {
            let pairs: Vec<(u64, Value)> = rows
                .iter()
                .filter_map(|r| Some((row_node(&r[id])?, r[value].clone())))
                .collect();
            Ok((write_node_values(cx, g, cfg, &pairs)?, 0, 0))
        }
        WriteKind::PathRels { .. } | WriteKind::Rels { .. } => {
            let name = cfg.str(WRITE_RELATIONSHIP_TYPE).unwrap_or_default();
            let ty = cx.tx.token(TokenKind::EdgeType, name)?;
            let (path, cycle) = match kind {
                WriteKind::PathRels { cycle } => (true, cycle),
                _ => (false, None),
            };
            let with_ids = path && cfg.bool("writeNodeIds");
            let with_costs = path && cfg.bool("writeCosts");
            let negative_cycles = cycle.is_some() && cfg.bool("writeNegativeCycles");
            let mut keys = Vec::new();
            for k in ["totalCost", "nodeIds", "costs"] {
                keys.push(if path {
                    cx.tx.token(TokenKind::PropertyKey, k)?
                } else {
                    0
                });
            }
            let (src, dst) = match kind {
                WriteKind::Rels { src, dst } => (src, dst),
                _ => (1, 2),
            };
            let mut written = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                if i % crate::query::exec::CHECK_EVERY == 0 {
                    cx.check()?;
                }
                if let Some(c) = cycle
                    && r[c] == Value::Bool(true)
                    && !negative_cycles
                {
                    continue;
                }
                let (Some(a), Some(b)) = (row_node(&r[src]), row_node(&r[dst])) else {
                    continue;
                };
                let mut props = Vec::new();
                if path {
                    props.push((keys[0], r[3].clone()));
                    if with_ids {
                        props.push((keys[1], r[4].clone()));
                    }
                    if with_costs {
                        props.push((keys[2], r[5].clone()));
                    }
                }
                let id = cx.tx.create_edge(g, a, b, ty, &props)?;
                cx.st.counters.edges_created += 1;
                cx.st.counters.properties_set += props.len() as u64;
                written.push(Value::Edge(id));
            }
            check_closed(cx, g, &written)?;
            Ok((0, written.len() as u64, 0))
        }
        WriteKind::EdgeFlag { edge } => {
            let name = cfg.str(config::WRITE_PROPERTY).unwrap_or_default();
            let key = cx.tx.token(TokenKind::PropertyKey, name)?;
            let mut written = Vec::new();
            for (i, r) in rows.iter().enumerate() {
                if i % crate::query::exec::CHECK_EVERY == 0 {
                    cx.check()?;
                }
                let Value::Int(e) = r[edge] else {
                    continue;
                };
                let id = e.cast_unsigned();
                let Some(rec) = cx.tx.edge(g, id)? else {
                    continue;
                };
                let mut props = rec.props;
                props.retain(|(k, _)| *k != key);
                props.push((key, Value::Bool(true)));
                cx.tx.set_edge(g, id, &props)?;
                cx.st.counters.properties_set += 1;
                written.push(Value::Edge(id));
            }
            check_closed(cx, g, &written)?;
            Ok((0, 0, written.len() as u64))
        }
    }
}

/// What a write mode's summary row is built from.
struct Summary<'a> {
    written: u64,
    rels_written: u64,
    edge_props_written: u64,
    ran_iterations: Option<u64>,
    did_converge: Option<bool>,
    values: &'a [f64],
    pre: Value,
    compute: Value,
    write: Value,
    config: Value,
    /// The entry's own columns ([`Output::Rows`]).
    extra: &'a [(&'static str, Value)],
}

impl Summary<'_> {
    /// The value of summary column `c` (module documentation).
    fn column(&self, c: &str) -> Value {
        let int = |x: u64| Value::Int(i64::try_from(x).unwrap_or(i64::MAX));
        // The entry's own value of a column comes first (an iterative
        // row-output entry gives `ranIterations` and `didConverge`).
        if let Some((_, v)) = self.extra.iter().find(|(k, _)| *k == c) {
            return v.clone();
        }
        match c {
            "nodePropertiesWritten" => int(self.written),
            "relationshipsWritten" => int(self.rels_written),
            "relationshipPropertiesWritten" => int(self.edge_props_written),
            "ranIterations" => self.ran_iterations.map_or(Value::Null, int),
            "didConverge" => self.did_converge.map_or(Value::Null, Value::Bool),
            "centralityDistribution" => distribution(self.values),
            "preProcessingMillis" => self.pre.clone(),
            "computeMillis" => self.compute.clone(),
            "postProcessingMillis" => Value::Int(0),
            "writeMillis" => self.write.clone(),
            "configuration" => self.config.clone(),
            other => self
                .extra
                .iter()
                .find(|(k, _)| *k == other)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("no summary value for the column {other}")),
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
