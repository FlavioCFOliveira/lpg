//! The traversal and ordering family (rmp #64): ALG-001 breadth-first
//! search, ALG-002 depth-first search, ALG-003 neighbourhood at hop,
//! ALG-004 path expansion and reachable subgraph, ALG-006 random walk,
//! ALG-007 topological sort, ALG-008 ancestors and descendants, ALG-009
//! reachability test, ALG-011 dominators.
//!
//! * **001, 002** follow GDS 2026.09.0's visit order
//!   ([`super::kernels::bfs`], [`super::kernels::dfs`]). GDS's `path`
//!   column joins consecutive visited nodes by virtual `NEXT`
//!   relationships, which no stored edge backs; lpg's path values hold
//!   stored edges only, so the column is null (rmp #64 open point).
//! * **003** ports MAGE @ `c72636014a8b`
//!   `cpp/neighbors_module/algorithm/neighbors.cpp` (`AtHop`, `ByHop`,
//!   `DetermineDirection`): levels of new nodes reached through edges of
//!   the given types and directions (`<T` incoming, `T>` outgoing, `T`
//!   both, an empty list every type both ways). Nodes of a level are
//!   returned in index order (MAGE's are in hash-set order).
//! * **004** ports MAGE `cpp/path_module/algorithm/path.cpp` (`PathHelper`
//!   label and relationship filters, `PathExpand::DFS`,
//!   `PathSubgraph::BFS`), with its rules kept as they are, including
//!   the duplicate node `subgraph_nodes` returns for a `<T>` match.
//! * **006** follows GDS's random walk (node2vec-biased, studied in GDS
//!   `traversal/RandomWalk*.java`): `walksPerNode` walks of `walkLength`
//!   nodes from each start node, a walk ending early at a node without
//!   outgoing edges. lpg draws each step exactly from the biased
//!   distribution (weight × 1/`returnFactor` back to the previous node,
//!   × 1 to a neighbour of it, × 1/`inOutFactor` otherwise) instead of
//!   GDS's rejection sampling, so its walks differ from GDS's (a parity
//!   entry, TS-25). The start nodes are split into blocks; block *b*
//!   draws from its own generator ([`super::sched::rng`]).
//! * **007** runs Kahn's levels ([`super::kernels::levels`]): nodes in
//!   level order, each level in index order; `maxDistanceFromSource` is
//!   the node's level, the longest path to it in edges, which GDS's
//!   longest-path tasks compute with weight 1.0
//!   (`dag/topologicalsort/TopologicalSort.java` `LongestPathTask`).
//! * **008** is a breadth-first search over incoming (ancestors) or
//!   outgoing (descendants) edges, as MAGE's `graph_util`
//!   (`cpp/graph_util_module/algorithms/ancestors.hpp`); the node itself
//!   is left out even on a cycle, as NetworkX's `ancestors` and
//!   `descendants`, the entry's oracle (`algorithm-entries.md`).
//! * **009** calls petgraph's `has_path_connecting`; **011** calls
//!   petgraph's `dominators::simple_fast` and computes the dominance
//!   frontiers with the rule NetworkX 3.7 applies
//!   (`networkx/algorithms/dominance.py` `dominance_frontiers`; Cooper,
//!   Harvey and Kennedy 2006): for a node with two or more distinct
//!   predecessors, or the root, each reached predecessor climbs the
//!   dominator tree to the node's immediate dominator.
//!
//! **MAGE licence** (Apache-2.0, see `LICENSE-APACHE`; Memgraph Ltd., the
//! files carry no copyright line). The functions below that port MAGE
//! code are modified: they run over the projection, with lpg's errors.

use rand::RngExt;

use super::config::{Def, Key, Ty};
use super::kernels::{self, NONE, id_value, node_index};
use super::projection::{Ix, Projection, with_projection};
use super::{Arg, ArgDefault, Entry, NODE_WRITE_COLUMNS, Output, Run, WriteKind};
use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

const TRAVERSE_COLUMNS: &[&str] = &["sourceNode", "nodeIds", "path"];

const TRAVERSE_KEYS: &[Key] = &[
    Key::new("sourceNode", Ty::Node, Def::Required),
    Key::new("targetNodes", Ty::Nodes, Def::EmptyList),
    Key::new("maxDepth", Ty::Int { min: -1 }, Def::Int(-1)),
];

/// ALG-001: `gds.bfs.stream`.
pub(crate) const BFS: Entry = Entry {
    algs: &[1],
    stream: "gds.bfs.stream",
    keys: TRAVERSE_KEYS,
    parallel: true,
    stream_columns: TRAVERSE_COLUMNS,
    run: run_bfs,
    ..Entry::BASE
};

/// ALG-002: `gds.dfs.stream`.
pub(crate) const DFS: Entry = Entry {
    algs: &[2],
    stream: "gds.dfs.stream",
    keys: TRAVERSE_KEYS,
    stream_columns: TRAVERSE_COLUMNS,
    run: run_dfs,
    ..Entry::BASE
};

const HOP_ARGS: &[Arg] = &[
    Arg::new("node", ArgDefault::Required),
    Arg::new("rel_type", ArgDefault::Required),
    Arg::new("distance", ArgDefault::Required),
];

/// ALG-003: `neighbors.at_hop`.
pub(crate) const AT_HOP: Entry = Entry {
    algs: &[3],
    stream: "neighbors.at_hop",
    stream_columns: &["nodes"],
    run: run_at_hop,
    args: HOP_ARGS,
    edge_types: true,
    ..Entry::BASE
};

/// ALG-003: `neighbors.by_hop`.
pub(crate) const BY_HOP: Entry = Entry {
    algs: &[3],
    stream: "neighbors.by_hop",
    stream_columns: &["nodes"],
    run: run_by_hop,
    args: HOP_ARGS,
    edge_types: true,
    ..Entry::BASE
};

/// ALG-004: `gds.pathExpand.stream`.
pub(crate) const PATH_EXPAND: Entry = Entry {
    algs: &[4],
    stream: "gds.pathExpand.stream",
    keys: &[
        Key::new("start", Ty::Nodes, Def::Required),
        Key::new("relationships", Ty::Names, Def::Required),
        Key::new("labels", Ty::Names, Def::Required),
        Key::new("minHops", Ty::Int { min: i64::MIN }, Def::Required),
        Key::new("maxHops", Ty::Int { min: i64::MIN }, Def::Required),
    ],
    stream_columns: &["result"],
    run: run_expand,
    edge_types: true,
    node_labels: true,
    ..Entry::BASE
};

const SUBGRAPH_KEYS: &[Key] = &[
    Key::new("startNode", Ty::Nodes, Def::Required),
    Key::new("minHops", Ty::Int { min: i64::MIN }, Def::Int(0)),
    Key::new("maxHops", Ty::Int { min: i64::MIN }, Def::Absent),
    Key::new("relationshipFilter", Ty::Names, Def::EmptyList),
    Key::new("labelFilter", Ty::Names, Def::EmptyList),
    Key::new("filterStartNode", Ty::Bool, Def::Bool(true)),
    Key::new("beginSequenceAtStart", Ty::Bool, Def::Bool(true)),
    Key::new("bfs", Ty::Bool, Def::Bool(false)),
];

/// ALG-004: `gds.pathSubgraphNodes.stream`.
pub(crate) const PATH_SUBGRAPH_NODES: Entry = Entry {
    algs: &[4],
    stream: "gds.pathSubgraphNodes.stream",
    keys: SUBGRAPH_KEYS,
    stream_columns: &["nodes"],
    run: run_subgraph_nodes,
    edge_types: true,
    node_labels: true,
    ..Entry::BASE
};

/// ALG-004: `gds.pathSubgraphAll.stream`.
pub(crate) const PATH_SUBGRAPH_ALL: Entry = Entry {
    algs: &[4],
    stream: "gds.pathSubgraphAll.stream",
    keys: SUBGRAPH_KEYS,
    stream_columns: &["nodes", "rels"],
    run: run_subgraph_all,
    edge_types: true,
    node_labels: true,
    ..Entry::BASE
};

const POSITIVE: Ty = Ty::Float {
    min: f64::MIN_POSITIVE,
    max: f64::INFINITY,
    max_open: false,
};

/// ALG-006: `gds.randomWalk.stream`.
pub(crate) const RANDOM_WALK: Entry = Entry {
    algs: &[6],
    stream: "gds.randomWalk.stream",
    keys: &[
        Key::new("sourceNodes", Ty::Nodes, Def::EmptyList),
        Key::new("walkLength", Ty::Int { min: 1 }, Def::Int(80)),
        Key::new("walksPerNode", Ty::Int { min: 1 }, Def::Int(10)),
        Key::new("returnFactor", POSITIVE, Def::Float(1.0)),
        Key::new("inOutFactor", POSITIVE, Def::Float(1.0)),
        Key::new("relationshipWeightProperty", Ty::Str, Def::Absent),
        Key::new("walkBufferSize", Ty::Int { min: 1 }, Def::Int(1000)),
    ],
    parallel: true,
    randomised: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &["nodeIds", "path"],
    run: run_random_walk,
    ..Entry::BASE
};

/// ALG-007: `gds.dag.topologicalSort.stream`.
pub(crate) const TOPOLOGICAL_SORT: Entry = Entry {
    algs: &[7],
    stream: "gds.dag.topologicalSort.stream",
    keys: &[Key::new(
        "computeMaxDistanceFromSource",
        Ty::Bool,
        Def::Bool(false),
    )],
    parallel: true,
    stream_columns: &["nodeId", "maxDistanceFromSource"],
    run: run_topological_sort,
    ..Entry::BASE
};

/// ALG-008: `graph_util.ancestors`.
pub(crate) const ANCESTORS: Entry = Entry {
    algs: &[8],
    stream: "graph_util.ancestors",
    stream_columns: &["ancestors"],
    run: run_ancestors,
    args: &[Arg::new("node", ArgDefault::Required)],
    ..Entry::BASE
};

/// ALG-008: `graph_util.descendants`.
pub(crate) const DESCENDANTS: Entry = Entry {
    algs: &[8],
    stream: "graph_util.descendants",
    stream_columns: &["descendants"],
    run: run_descendants,
    args: &[Arg::new("node", ArgDefault::Required)],
    ..Entry::BASE
};

/// ALG-009: `gds.hasPathConnecting.stream`.
pub(crate) const HAS_PATH: Entry = Entry {
    algs: &[9],
    stream: "gds.hasPathConnecting.stream",
    keys: &[
        Key::new("sourceNode", Ty::Node, Def::Required),
        Key::new("targetNode", Ty::Node, Def::Required),
    ],
    stream_columns: &["sourceNode", "targetNode", "connected"],
    run: run_has_path,
    ..Entry::BASE
};

/// ALG-011: `gds.dominatorsSimpleFast.stream` and `.write`.
pub(crate) const DOMINATORS: Entry = Entry {
    algs: &[11],
    stream: "gds.dominatorsSimpleFast.stream",
    write: Some("gds.dominatorsSimpleFast.write"),
    keys: &[Key::new("sourceNode", Ty::Node, Def::Required)],
    stream_columns: &["nodeId", "immediateDominator", "dominanceFrontier"],
    write_columns: NODE_WRITE_COLUMNS,
    run: run_dominators,
    write_kind: WriteKind::NodeProp { id: 0, value: 1 },
    ..Entry::BASE
};

/// The dense index of the node of configuration key `key`.
pub(crate) fn key_node<I: Ix>(
    p: &Projection<I>,
    r: &Run<'_, '_>,
    key: &str,
) -> Result<usize, Error> {
    node_index(p, r.cfg.get(key), key)
}

/// The dense indexes of the nodes of a [`Ty::Nodes`] key.
pub(crate) fn key_nodes<I: Ix>(
    p: &Projection<I>,
    r: &Run<'_, '_>,
    key: &str,
) -> Result<Vec<usize>, Error> {
    r.cfg
        .nodes(key)
        .into_iter()
        .map(|id| node_index(p, &Value::Node(id), key))
        .collect()
}

/// `maxDepth` as a limit: `None` for -1.
fn max_depth(r: &Run<'_, '_>) -> Option<u64> {
    u64::try_from(r.cfg.int("maxDepth")).ok()
}

fn run_bfs(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => traverse(p, r, true))
}

fn run_dfs(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => traverse(p, r, false))
}

fn traverse<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>, breadth: bool) -> Result<Output, Error> {
    let source = key_node(p, r, "sourceNode")?;
    let targets = key_nodes(p, r, "targetNodes")?;
    let depth = max_depth(r);
    let order = if breadth {
        kernels::bfs(p, r.sched, r.mem, source, depth, &targets)?
    } else {
        kernels::dfs(p, r.mem, source, depth, &targets)?
    };
    let row = vec![
        id_value(p, source),
        Value::List(
            order
                .iter()
                .map(|v| id_value(p, *v))
                .collect::<Vec<_>>()
                .into(),
        ),
        Value::Null,
    ];
    let mut rows = Vec::new();
    r.emit(&mut rows, row)?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// A positional argument, or 22G03 when it is not of the kind wanted.
fn arg_list(v: &Value, what: &str) -> Result<Vec<String>, Error> {
    match v {
        Value::List(l) => l
            .iter()
            .map(|x| match x {
                Value::String(s) => Ok(s.to_string()),
                _ => Err(type_error(what, "a list of strings")),
            })
            .collect(),
        Value::Null => Ok(Vec::new()),
        _ => Err(type_error(what, "a list of strings")),
    }
}

fn type_error(what: &str, kind: &str) -> Error {
    Error::new(
        codes::C22G03,
        format!("invalid value type: {what} must be {kind}"),
    )
}

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

/// The edge type filter of MAGE's `neighbors` (`DetermineDirection`):
/// the types followed on incoming and on outgoing edges; `None` in a set
/// stands for every type, `Some(None)` for a type no edge has.
#[derive(Debug, Default)]
struct HopTypes {
    incoming: Vec<Option<Option<u64>>>,
    outgoing: Vec<Option<Option<u64>>>,
}

impl HopTypes {
    fn parse(r: &Run<'_, '_>, names: &[String]) -> Result<HopTypes, Error> {
        let mut t = HopTypes::default();
        let names: Vec<String> = if names.is_empty() {
            vec![String::new()]
        } else {
            names.to_vec()
        };
        let tok = |name: &str| -> Option<Option<u64>> {
            if name.is_empty() {
                None
            } else {
                Some(r.token(TokenKind::EdgeType, name))
            }
        };
        for n in &names {
            if n.starts_with('<') && n.ends_with('>') && n.len() >= 2 {
                return Err(invalid("invalid relationship specification"));
            }
            if let Some(rest) = n.strip_prefix('<') {
                t.incoming.push(tok(rest));
            } else if let Some(rest) = n.strip_suffix('>') {
                t.outgoing.push(tok(rest));
            } else {
                t.incoming.push(tok(n));
                t.outgoing.push(tok(n));
            }
        }
        Ok(t)
    }

    fn admits(set: &[Option<Option<u64>>], ty: u64) -> bool {
        set.iter().any(|t| match t {
            None => true,
            Some(t) => *t == Some(ty),
        })
    }
}

/// The levels of new nodes around the start node (MAGE `AtHop`, `ByHop`):
/// level 0 is the start node; the loop stops after `distance` levels, or
/// at the first empty level when `stop_empty`.
fn hop_levels<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    stop_empty: bool,
) -> Result<Option<Vec<Vec<usize>>>, Error> {
    let start = node_index(p, &r.args[0], "node")?;
    let names = arg_list(&r.args[1], "rel_type")?;
    let distance = match &r.args[2] {
        Value::Int(d) => *d,
        _ => return Err(type_error("distance", "an integer")),
    };
    let types = HopTypes::parse(r, &names)?;
    let n = p.node_count();
    let mut known = r.mem.vec(n, false)?;
    known[start] = true;
    let mut levels = vec![vec![start]];
    while (levels.len() as i64) <= distance {
        r.mem.check_now()?;
        let mut set = Vec::new();
        for &v in levels.last().expect("a level") {
            if !types.incoming.is_empty() {
                for (u, slot) in p.inc(v) {
                    if HopTypes::admits(&types.incoming, p.edge_types[slot]) && !known[u.index()] {
                        set.push(u.index());
                    }
                }
            }
            if !types.outgoing.is_empty() {
                for (w, slot) in p.out(v) {
                    if HopTypes::admits(&types.outgoing, p.edge_types[slot]) && !known[w.index()] {
                        set.push(w.index());
                    }
                }
            }
        }
        set.sort_unstable();
        set.dedup();
        if set.is_empty() && stop_empty {
            return Ok(None);
        }
        for v in &set {
            known[*v] = true;
        }
        r.mem.grow(set.len() * size_of::<usize>())?;
        levels.push(set);
    }
    Ok(Some(levels))
}

fn run_at_hop(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => at_hop(p, r))
}

fn at_hop<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let mut rows = Vec::new();
    if let Some(levels) = hop_levels(p, r, true)? {
        for v in levels.last().expect("a level") {
            r.emit(&mut rows, vec![Value::Node(p.ids[*v])])?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_by_hop(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => by_hop(p, r))
}

fn by_hop<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let mut rows = Vec::new();
    let levels = hop_levels(p, r, false)?.unwrap_or_default();
    for level in levels.iter().skip(1) {
        let list: Vec<Value> = level.iter().map(|v| Value::Node(p.ids[*v])).collect();
        r.emit(&mut rows, vec![Value::List(list.into())])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// A relationship direction of MAGE's `path` filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    None,
    Any,
    Incoming,
    Outgoing,
    Both,
}

/// The label filter flags of one node (MAGE `LabelBools`).
#[derive(Debug, Clone, Copy, Default)]
struct LabelBools {
    blacklisted: bool,
    terminated: bool,
    end_node: bool,
    whitelisted: bool,
}

/// MAGE's `PathHelper`: parsed label and relationship filters and hop
/// limits.
#[derive(Debug, Default)]
struct PathHelper {
    termination: Vec<Option<u64>>,
    blacklist: Vec<Option<u64>>,
    whitelist: Vec<Option<u64>>,
    end_list: Vec<Option<u64>>,
    /// Edge type token (or a name no edge has) and its direction.
    rels: Vec<(Option<u64>, Dir)>,
    any_incoming: bool,
    any_outgoing: bool,
    min_hops: i64,
    max_hops: i64,
    filter_start_node: bool,
    begin_sequence_at_start: bool,
}

impl PathHelper {
    fn new(r: &Run<'_, '_>, labels: &[String], rels: &[String]) -> Result<PathHelper, Error> {
        let mut h = PathHelper {
            min_hops: 0,
            max_hops: i64::MAX,
            filter_start_node: true,
            begin_sequence_at_start: true,
            ..PathHelper::default()
        };
        for l in labels {
            let (set, name) = match l.chars().next() {
                Some('-') => (&mut h.blacklist, &l[1..]),
                Some('>') => (&mut h.end_list, &l[1..]),
                Some('+') => (&mut h.whitelist, &l[1..]),
                Some('/') => (&mut h.termination, &l[1..]),
                Some(_) => (&mut h.whitelist, &l[..]),
                None => return Err(invalid("an empty label filter")),
            };
            set.push(r.token(TokenKind::Label, name));
        }
        if rels.is_empty() {
            h.any_incoming = true;
            h.any_outgoing = true;
        }
        for t in rels {
            let starts = t.starts_with('<');
            let ends = t.ends_with('>');
            if t.len() == 1 {
                if starts {
                    h.any_incoming = true;
                } else if ends {
                    h.any_outgoing = true;
                } else {
                    h.rels.push((r.token(TokenKind::EdgeType, t), Dir::Any));
                }
                continue;
            }
            let (name, dir) = match (starts, ends) {
                (true, true) => (&t[1..t.len() - 1], Dir::Both),
                (true, false) => (&t[1..], Dir::Incoming),
                (false, true) => (&t[..t.len() - 1], Dir::Outgoing),
                (false, false) => (&t[..], Dir::Any),
            };
            if name.is_empty() {
                return Err(invalid("an empty relationship filter"));
            }
            h.rels.push((r.token(TokenKind::EdgeType, name), dir));
        }
        Ok(h)
    }

    fn direction(&self, ty: u64) -> Dir {
        // MAGE keeps one direction per type name: the last one given.
        self.rels
            .iter()
            .rev()
            .find(|(t, _)| *t == Some(ty))
            .map_or(Dir::None, |(_, d)| *d)
    }

    fn label_bools<I: Ix>(&self, p: &Projection<I>, v: usize) -> LabelBools {
        let mut b = LabelBools::default();
        let has = |set: &[Option<u64>], l: u64| set.contains(&Some(l));
        for &l in p.labels_of(v) {
            b.blacklisted |= has(&self.blacklist, l);
            b.terminated |= has(&self.termination, l);
            b.end_node |= has(&self.end_list, l);
            b.whitelisted |= has(&self.whitelist, l);
        }
        b
    }

    fn whitelisted(&self, w: bool) -> bool {
        self.whitelist.is_empty() || w
    }

    fn labels_valid(&self, b: LabelBools) -> bool {
        let end_activated = !self.end_list.is_empty();
        let termination_activated = !self.termination.is_empty();
        !b.blacklisted
            && ((b.end_node && end_activated)
                || b.terminated
                || (!termination_activated && !end_activated && self.whitelisted(b.whitelisted)))
    }

    fn continue_expanding(&self, b: LabelBools, path_size: i64) -> bool {
        path_size <= self.max_hops
            && ((!b.blacklisted
                && !b.terminated
                && (b.end_node || self.whitelisted(b.whitelisted)))
                || (path_size == 1 && !self.filter_start_node))
    }

    fn any_directed(&self, outgoing: bool) -> bool {
        if outgoing {
            self.any_outgoing
        } else {
            self.any_incoming
        }
    }
}

/// The edges of `v` on one side: (other node, forward position).
fn side<I: Ix>(p: &Projection<I>, v: usize, outgoing: bool) -> Vec<(usize, usize)> {
    let adj = if outgoing { p.out(v) } else { p.inc(v) };
    adj.map(|(w, slot)| (w.index(), slot)).collect()
}

/// MAGE `PathExpand`: every trail from the start nodes (module
/// documentation).
struct Expand<'a, I> {
    p: &'a Projection<I>,
    h: PathHelper,
    nodes: Vec<usize>,
    slots: Vec<usize>,
    used: Vec<bool>,
    out: Vec<Value>,
}

impl<I: Ix> Expand<'_, I> {
    /// The edges MAGE's `ExpandFromRelationships` follows from the path's
    /// last node, incoming first, then outgoing.
    fn candidates(&self, v: usize) -> Vec<(usize, usize)> {
        let mut seen: Vec<(u64, usize)> = Vec::new();
        let mut out = Vec::new();
        for outgoing in [false, true] {
            for (w, slot) in side(self.p, v, outgoing) {
                let ty = self.p.edge_types[slot];
                let wanted = self.h.direction(ty);
                if (wanted == Dir::None && !self.h.any_directed(outgoing)) || self.used[slot] {
                    continue;
                }
                let current = if outgoing {
                    Dir::Outgoing
                } else {
                    Dir::Incoming
                };
                if wanted == Dir::Any || current == wanted || self.h.any_directed(outgoing) {
                    out.push((w, slot));
                } else if wanted == Dir::Both {
                    if outgoing && seen.contains(&(ty, w)) {
                        out.push((w, slot));
                    } else {
                        // MAGE records the edge's source: the other node
                        // of an incoming edge, this node of an outgoing
                        // one.
                        seen.push((ty, if outgoing { v } else { w }));
                    }
                }
            }
        }
        out
    }

    /// MAGE's recursive `DFS` from the current path, with an explicit
    /// stack: each frame holds the edges its node follows and the next
    /// one to take. The edges of a frame do not depend on the deeper
    /// frames, which give back every edge they take.
    fn dfs(&mut self, r: &mut Run<'_, '_>) -> Result<(), Error> {
        let mut frames: Vec<(Vec<(usize, usize)>, usize)> = Vec::new();
        loop {
            // Enter the path's last node.
            let v = *self.nodes.last().expect("a node");
            let size = self.slots.len() as i64;
            let b = self.h.label_bools(self.p, v);
            if size >= self.h.min_hops && size <= self.h.max_hops && self.h.labels_valid(b) {
                let path = kernels::path_value(self.p, &self.nodes, &self.slots);
                r.mem.grow(crate::query::exec::value_bytes(&path))?;
                self.out.push(path);
            }
            let next = if self.h.continue_expanding(b, size + 1) {
                r.mem.check_now()?;
                self.candidates(v)
            } else {
                Vec::new()
            };
            frames.push((next, 0));
            // Take the next edge of the deepest frame that has one.
            loop {
                let Some((edges, at)) = frames.last_mut() else {
                    return Ok(());
                };
                if let Some(&(w, slot)) = edges.get(*at) {
                    *at += 1;
                    self.used[slot] = true;
                    self.nodes.push(w);
                    self.slots.push(slot);
                    break;
                }
                frames.pop();
                if let Some(slot) = self.slots.pop() {
                    self.used[slot] = false;
                    self.nodes.pop();
                }
            }
        }
    }
}

fn run_expand(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => expand(p, r))
}

fn expand<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let mut starts = key_nodes(p, r, "start")?;
    starts.sort_unstable();
    starts.dedup();
    let labels: Vec<String> = r
        .cfg
        .names("labels")
        .into_iter()
        .map(str::to_string)
        .collect();
    let rels: Vec<String> = r
        .cfg
        .names("relationships")
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut h = PathHelper::new(r, &labels, &rels)?;
    h.min_hops = r.cfg.int("minHops");
    h.max_hops = r.cfg.int("maxHops");
    let m = p.edge_ids.len();
    r.mem.grow(m)?;
    let mut e = Expand {
        p,
        h,
        nodes: Vec::new(),
        slots: Vec::new(),
        used: vec![false; m],
        out: Vec::new(),
    };
    for s in starts {
        e.nodes = vec![s];
        e.slots.clear();
        e.dfs(r)?;
    }
    let mut rows = Vec::new();
    for path in std::mem::take(&mut e.out) {
        r.emit(&mut rows, vec![path])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// MAGE `PathSubgraph::BFS`: the returned nodes, in MAGE's order.
fn subgraph<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Vec<usize>, Error> {
    let mut starts = key_nodes(p, r, "startNode")?;
    starts.sort_unstable();
    starts.dedup();
    let labels: Vec<String> = r
        .cfg
        .names("labelFilter")
        .into_iter()
        .map(str::to_string)
        .collect();
    let rels: Vec<String> = r
        .cfg
        .names("relationshipFilter")
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut h = PathHelper::new(r, &labels, &rels)?;
    if let Value::Int(max) = r.cfg.get("maxHops")
        && *max >= 0
    {
        h.max_hops = *max;
    }
    if let Value::Int(min) = r.cfg.get("minHops")
        && *min >= 0
    {
        h.min_hops = *min;
    }
    h.filter_start_node = r.cfg.bool("filterStartNode");
    h.begin_sequence_at_start = r.cfg.bool("beginSequenceAtStart");
    let n = p.node_count();
    let mut visited = r.mem.vec(n, false)?;
    r.mem.grow(3 * n * size_of::<usize>())?;
    let mut queue = std::collections::VecDeque::new();
    for s in &starts {
        queue.push_back((*s, 0i64));
        visited[*s] = true;
    }
    let mut out = Vec::new();
    while let Some((v, hops)) = queue.pop_front() {
        if hops > h.max_hops {
            continue;
        }
        let b = h.label_bools(p, v);
        if h.filter_start_node || hops != 0 {
            if h.labels_valid(b) {
                out.push(v);
            }
        } else {
            out.push(v);
        }
        if !h.continue_expanding(b, hops + 1) {
            continue;
        }
        let mut seen: Vec<(u64, usize)> = Vec::new();
        for outgoing in [false, true] {
            for (w, slot) in side(p, v, outgoing) {
                if visited[w] {
                    continue;
                }
                let ty = p.edge_types[slot];
                let wanted = h.direction(ty);
                if (h.begin_sequence_at_start || hops != 0)
                    && wanted == Dir::None
                    && !h.any_directed(outgoing)
                {
                    continue;
                }
                let current = if outgoing {
                    Dir::Outgoing
                } else {
                    Dir::Incoming
                };
                if wanted == Dir::Any || current == wanted || h.any_directed(outgoing) {
                    visited[w] = true;
                    queue.push_back((w, hops + 1));
                } else if wanted == Dir::Both {
                    if outgoing && seen.contains(&(ty, w)) {
                        visited[w] = true;
                        queue.push_back((w, hops + 1));
                        out.push(w);
                    } else {
                        seen.push((ty, if outgoing { v } else { w }));
                    }
                }
            }
        }
    }
    Ok(out)
}

fn run_subgraph_nodes(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => subgraph_nodes(p, r))
}

fn subgraph_nodes<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let nodes = subgraph(p, r)?;
    let mut rows = Vec::new();
    for v in nodes {
        r.emit(&mut rows, vec![Value::Node(p.ids[v])])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_subgraph_all(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => subgraph_all(p, r))
}

fn subgraph_all<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let nodes = subgraph(p, r)?;
    let mut member = r.mem.vec(p.node_count(), false)?;
    for v in &nodes {
        member[*v] = true;
    }
    let mut rels = Vec::new();
    for v in &nodes {
        for (w, slot) in p.out(*v) {
            if member[w.index()] {
                r.mem.grow(crate::query::exec::value_bytes(&Value::Null))?;
                rels.push(Value::Edge(p.edge_id(slot)));
            }
        }
    }
    let list: Vec<Value> = nodes.iter().map(|v| Value::Node(p.ids[*v])).collect();
    let mut rows = Vec::new();
    r.emit(
        &mut rows,
        vec![Value::List(list.into()), Value::List(rels.into())],
    )?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_random_walk(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => random_walk(p, r))
}

/// The parameters of a walk.
struct Walk {
    length: usize,
    per_node: usize,
    back: f64,
    out: f64,
    weighted: bool,
}

fn random_walk<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let mut starts = key_nodes(p, r, "sourceNodes")?;
    starts.sort_unstable();
    starts.dedup();
    let all = starts.is_empty();
    let w = Walk {
        length: usize::try_from(r.cfg.int("walkLength")).unwrap_or(usize::MAX),
        per_node: usize::try_from(r.cfg.int("walksPerNode")).unwrap_or(usize::MAX),
        back: 1.0 / r.cfg.float("returnFactor"),
        out: 1.0 / r.cfg.float("inOutFactor"),
        weighted: r.cfg.str("relationshipWeightProperty").is_some(),
    };
    if w.weighted && p.edge_props[0].iter().any(|x| x.is_nan() || *x < 0.0) {
        return Err(invalid(
            "the relationship weights of a random walk must be numbers of at least 0",
        ));
    }
    let count = if all { p.node_count() } else { starts.len() };
    r.mem.grow(
        count
            .saturating_mul(w.per_node)
            .saturating_mul(w.length)
            .saturating_mul(2 * size_of::<usize>()),
    )?;
    let seed = r.cfg.seed();
    let walks = r.sched.map(|b, range| {
        let mut rng = super::sched::rng(seed, b);
        let nodes: Vec<usize> = if all {
            range.collect()
        } else {
            let lo = starts.partition_point(|v| *v < range.start);
            let hi = starts.partition_point(|v| *v < range.end);
            starts[lo..hi].to_vec()
        };
        let mut out = Vec::new();
        for s in nodes {
            for _ in 0..w.per_node {
                out.push(one_walk(p, &w, s, &mut rng));
            }
        }
        out
    })?;
    let mut rows = Vec::new();
    for (nodes, slots) in walks.into_iter().flatten() {
        let ids: Vec<Value> = nodes.iter().map(|v| id_value(p, *v)).collect();
        let path = kernels::path_value(p, &nodes, &slots);
        r.emit(&mut rows, vec![Value::List(ids.into()), path])?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// One walk from `s`: its nodes and the edges taken.
fn one_walk<I: Ix>(
    p: &Projection<I>,
    w: &Walk,
    s: usize,
    rng: &mut rand::rngs::ChaCha8Rng,
) -> (Vec<usize>, Vec<usize>) {
    let mut nodes = vec![s];
    let mut slots = Vec::new();
    let mut choices: Vec<(usize, usize, f64)> = Vec::new();
    while nodes.len() < w.length {
        let v = *nodes.last().expect("a node");
        let prev = (nodes.len() >= 2).then(|| nodes[nodes.len() - 2]);
        choices.clear();
        let mut total = 0.0;
        for (t, slot) in p.out(v) {
            let t = t.index();
            let mut x = kernels::cost(p, w.weighted, slot);
            if let Some(prev) = prev {
                x *= if t == prev {
                    w.back
                } else if p.out(prev).any(|(u, _)| u.index() == t) {
                    1.0
                } else {
                    w.out
                };
            }
            total += x;
            choices.push((t, slot, total));
        }
        if choices.is_empty() || total <= 0.0 {
            break;
        }
        let u: f64 = rng.random::<f64>() * total;
        let at = choices
            .partition_point(|(_, _, c)| *c <= u)
            .min(choices.len() - 1);
        nodes.push(choices[at].0);
        slots.push(choices[at].1);
    }
    (nodes, slots)
}

fn run_topological_sort(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => topological_sort(p, r))
}

fn topological_sort<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let levels = kernels::levels(p, r.sched, r.mem)?;
    let with_distance = r.cfg.bool("computeMaxDistanceFromSource");
    let mut rows = Vec::new();
    for (k, level) in levels.iter().enumerate() {
        for v in level {
            let d = if with_distance {
                Value::Float(k as f64)
            } else {
                Value::Null
            };
            r.emit(&mut rows, vec![id_value(p, *v), d])?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_ancestors(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => reach(p, r, false))
}

fn run_descendants(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => reach(p, r, true))
}

/// The nodes reachable from the argument node over outgoing
/// (`forward`) or incoming edges, in breadth-first order, without the
/// node itself (module documentation).
fn reach<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>, forward: bool) -> Result<Output, Error> {
    let start = node_index(p, &r.args[0], "node")?;
    let n = p.node_count();
    let mut seen = r.mem.vec(n, false)?;
    r.mem.grow(2 * n * size_of::<usize>())?;
    let mut queue = std::collections::VecDeque::from([start]);
    seen[start] = true;
    let mut out = Vec::new();
    while let Some(v) = queue.pop_front() {
        let adj = if forward { p.out(v) } else { p.inc(v) };
        for (w, _) in adj {
            let w = w.index();
            if !seen[w] {
                seen[w] = true;
                out.push(w);
                queue.push_back(w);
            }
        }
    }
    let list: Vec<Value> = out.iter().map(|v| Value::Node(p.ids[*v])).collect();
    let mut rows = Vec::new();
    r.emit(&mut rows, vec![Value::List(list.into())])?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_has_path(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => has_path(p, r))
}

fn has_path<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let a = key_node(p, r, "sourceNode")?;
    let b = key_node(p, r, "targetNode")?;
    r.mem.grow(2 * p.node_count() * size_of::<usize>())?;
    let connected = petgraph::algo::has_path_connecting(p, I::new(a), I::new(b), None);
    let mut rows = Vec::new();
    r.emit(
        &mut rows,
        vec![id_value(p, a), id_value(p, b), Value::Bool(connected)],
    )?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_dominators(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => dominators(p, r))
}

fn dominators<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let root = key_node(p, r, "sourceNode")?;
    let n = p.node_count();
    r.mem.grow(8 * n * size_of::<usize>())?;
    let d = petgraph::algo::dominators::simple_fast(p, I::new(root));
    // Reached nodes have an immediate dominator; the root has NONE.
    let mut idom = vec![NONE; n];
    let mut reached = vec![false; n];
    reached[root] = true;
    for (v, slot) in idom.iter_mut().enumerate() {
        if let Some(x) = d.immediate_dominator(I::new(v)) {
            *slot = x.index();
            reached[v] = true;
        }
    }
    let mut frontier: Vec<Vec<usize>> = vec![Vec::new(); n];
    for u in (0..n).filter(|u| reached[*u]) {
        let mut preds: Vec<usize> = p.inc(u).map(|(x, _)| x.index()).collect();
        preds.sort_unstable();
        preds.dedup();
        if u == root || preds.len() >= 2 {
            for mut v in preds {
                if !reached[v] {
                    continue;
                }
                while v != idom[u] {
                    frontier[v].push(u);
                    if v == root {
                        break;
                    }
                    v = idom[v];
                }
            }
        }
    }
    let mut rows = Vec::new();
    for v in (0..n).filter(|v| reached[*v]) {
        let mut f = std::mem::take(&mut frontier[v]);
        f.sort_unstable();
        f.dedup();
        let dom = if idom[v] == NONE {
            Value::Null
        } else {
            id_value(p, idom[v])
        };
        let list: Vec<Value> = f.iter().map(|x| id_value(p, *x)).collect();
        r.emit(
            &mut rows,
            vec![id_value(p, v), dom, Value::List(list.into())],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}
