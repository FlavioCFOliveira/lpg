//! The cycles, tours and routing family (rmp #69): ALG-180 cycle
//! detection, ALG-181 simple-cycle enumeration, ALG-187 travelling
//! salesman, ALG-188 vehicle routing. All run on one worker
//! (`algorithm-entries.md` Parallel classification).
//!
//! * **180** ports MAGE @ `c72636014a8b`
//!   `cpp/cycles_module/algorithm/cycles.cpp` (`GetCycles`,
//!   `FindNonSpanningTreeEdges`, `FindFundamentalCycle`, `CombineCycles`;
//!   Apache-2.0, see `LICENSE-APACHE`; Memgraph Ltd., no copyright line in
//!   the file): on the undirected projection, the fundamental cycles of a
//!   depth-first spanning tree of each component, every combination of
//!   them whose symmetric difference is one cycle, then a two-node cycle
//!   per pair of nodes joined by parallel edges and per self-loop.
//!   Modified: an explicit stack instead of recursion; MAGE's mask is a
//!   32-bit integer, so more than 31 fundamental cycles in one component
//!   is undefined there, and lpg fails such a run with 22N11 above 63.
//! * **181** copies graphalgs @ `1f8a412ef4af` `src/elementary_circuits.rs`
//!   (`elementary_circuits`, `elementary_circuits_starting_at_s`; MIT,
//!   "Copyright (c) 2021 PrototypeRailGun", licence text in
//!   [`super::paths`]): for each node `s`, a search over nodes above `s`
//!   that records each path ending at a predecessor of `s`. Modified: over
//!   the projection instead of a petgraph `Graph`, parallel edges counted
//!   once, as graphalgs and NetworkX's `simple_cycles` count them.
//! * **187** ports MAGE `python/mage/geography/travelling_salesman.py` and
//!   `distance_calculator.py` (`create_distance_matrix`, `solve_greedy`,
//!   `solve_2_approx`, `solve_1_5_approx`, `get_euler_circuit`,
//!   `get_hamiltonian_circuit`; Apache-2.0, as above): the haversine
//!   distance in metres between the `lat` and `lng` of the points, then a
//!   tour. Modified: MAGE's 1.5-approximation pairs the odd-degree
//!   vertices of the spanning tree in list order ("dummy perfect
//!   matchings", its own TODO), which voids the 1.5 bound the entry's
//!   verification row requires; lpg uses a minimum-weight perfect matching,
//!   as Christofides' algorithm and NetworkX's `christofides`
//!   (`approximation/traveling_salesman.py`, BSD-3-Clause, method only)
//!   do: exact, by dynamic programming over subsets, for at most 20 odd
//!   vertices; above that, greedy (the bound then does not hold; rmp #69
//!   open point).
//! * **188** is the deterministic Clarke-Wright savings heuristic followed
//!   by 2-opt on each route (TS-25; user decision 2026-10-12, item D),
//!   over every node of the projection, with MAGE's parameters
//!   (`python/vrp.py`: `depot_node`, `number_of_vehicles` default 1,
//!   at least 1) and output columns. No capacity applies, as in MAGE.
//!   Savings `d(0,i) + d(0,j) − d(i,j)` are taken in decreasing order
//!   (ties by node pair); two routes merge when the pair joins an end of
//!   each; positive savings first, then, while more routes than vehicles
//!   remain, the best remaining ones.

use std::collections::{BTreeMap, BTreeSet};

use super::kernels::{self, id_value, node_index};
use super::projection::{Ix, Projection, with_projection};
use super::{Arg, ArgDefault, Entry, Output, Run};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// ALG-180: `cycles.get`.
pub(crate) const CYCLES: Entry = Entry {
    algs: &[180],
    stream: "cycles.get",
    undirected: true,
    stream_columns: &["cycle_id", "node"],
    run: run_cycles,
    ..Entry::BASE
};

/// ALG-181: `gds.elementaryCircuits.stream`.
pub(crate) const CIRCUITS: Entry = Entry {
    algs: &[181],
    stream: "gds.elementaryCircuits.stream",
    stream_columns: &["cycleId", "nodeIds", "path"],
    run: run_circuits,
    ..Entry::BASE
};

/// ALG-187: `tsp.solve`.
pub(crate) const TSP: Entry = Entry {
    algs: &[187],
    stream: "tsp.solve",
    stream_columns: &["sources", "destinations"],
    run: run_tsp,
    args: &[
        Arg::new("points", ArgDefault::Required),
        Arg::new("method", ArgDefault::Str("1.5_approx")),
    ],
    named_node_props: &["lat", "lng"],
    ..Entry::BASE
};

/// ALG-188: `vrp.route`.
pub(crate) const VRP: Entry = Entry {
    algs: &[188],
    stream: "vrp.route",
    stream_columns: &["from_vertex", "to_vertex"],
    run: run_vrp,
    args: &[
        Arg::new("depot_node", ArgDefault::Required),
        Arg::new("number_of_vehicles", ArgDefault::Null),
    ],
    named_node_props: &["lat", "lng"],
    ..Entry::BASE
};

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

fn run_cycles(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => cycles(p, r))
}

/// MAGE's `FindNonSpanningTreeEdges` from `root` over the undirected
/// projection, with an explicit stack: the parent and depth of each node
/// of the component, and the edges that are not tree edges, as sorted
/// pairs.
fn spanning_tree<I: Ix>(
    p: &Projection<I>,
    root: usize,
    visited: &mut [bool],
    parent: &mut [usize],
    depth: &mut [usize],
    non_tree: &mut BTreeSet<(usize, usize)>,
) {
    parent[root] = root;
    visited[root] = true;
    // A frame: node, its neighbours (one per edge), the next one, the
    // neighbours already seen.
    let mut frames: Vec<(usize, Vec<usize>, usize, BTreeSet<usize>)> = vec![(
        root,
        p.out(root).map(|(w, _)| w.index()).collect(),
        0,
        BTreeSet::new(),
    )];
    while let Some((v, nbrs, at, unique)) = frames.last_mut() {
        let Some(&next) = nbrs.get(*at) else {
            frames.pop();
            continue;
        };
        *at += 1;
        let v = *v;
        if parent[v] != kernels::NONE && (next == parent[v] || unique.contains(&next)) {
            continue;
        }
        unique.insert(next);
        if visited[next] {
            non_tree.insert((v.min(next), v.max(next)));
            continue;
        }
        parent[next] = v;
        depth[next] = depth[v] + 1;
        visited[next] = true;
        let nb: Vec<usize> = p.out(next).map(|(w, _)| w.index()).collect();
        frames.push((next, nb, 0, BTreeSet::new()));
    }
}

/// MAGE's `FindFundamentalCycle`: the tree path from the deeper end of a
/// non-tree edge up to the other, closed.
fn fundamental(a: usize, b: usize, parent: &[usize], depth: &[usize]) -> Option<Vec<usize>> {
    let (mut a, b) = if depth[a] < depth[b] { (b, a) } else { (a, b) };
    let mut cycle = vec![a];
    while depth[a] > depth[b] {
        a = parent[a];
        cycle.push(a);
    }
    if a != b {
        // MAGE throws: no cross edges in a depth-first tree.
        return None;
    }
    cycle.push(cycle[0]);
    Some(cycle)
}

/// MAGE's `CombineCycles`: the cycle that the symmetric difference of the
/// fundamental cycles of `mask` forms, if it is one cycle of more than two
/// nodes.
fn combine(mask: u64, fundamentals: &[Vec<usize>]) -> Option<Vec<usize>> {
    let mut count: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    for (i, f) in fundamentals.iter().enumerate() {
        if mask & (1 << i) == 0 {
            continue;
        }
        for w in f.windows(2) {
            *count.entry((w[0].min(w[1]), w[0].max(w[1]))).or_insert(0) += 1;
        }
    }
    let mut adj: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for ((a, b), c) in count {
        if c % 2 == 0 {
            continue;
        }
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    if adj.values().any(|v| v.len() != 2) {
        return None;
    }
    let first = *adj.keys().next()?;
    let mut seen = BTreeSet::new();
    let mut cycle = Vec::new();
    let mut cur = first;
    while seen.insert(cur) {
        cycle.push(cur);
        if let Some(next) = adj[&cur].iter().find(|x| !seen.contains(x)) {
            cur = *next;
        }
    }
    (seen.len() == adj.len() && cycle.len() > 2).then_some(cycle)
}

fn cycles<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem.grow(4 * n * size_of::<usize>())?;
    let mut visited = vec![false; n];
    let mut parent = vec![kernels::NONE; n];
    let mut depth = vec![0usize; n];
    let mut found: Vec<Vec<usize>> = Vec::new();
    for root in 0..n {
        if visited[root] {
            continue;
        }
        let mut non_tree = BTreeSet::new();
        spanning_tree(
            p,
            root,
            &mut visited,
            &mut parent,
            &mut depth,
            &mut non_tree,
        );
        let fundamentals: Vec<Vec<usize>> = non_tree
            .iter()
            .filter_map(|(a, b)| fundamental(*a, *b, &parent, &depth))
            .collect();
        if fundamentals.len() > 63 {
            return Err(invalid(
                "a component has more than 63 fundamental cycles, too many to combine",
            ));
        }
        for mask in 1..(1u64 << fundamentals.len()) {
            if mask % 4096 == 0 {
                r.mem.check_now()?;
            }
            if let Some(c) = combine(mask, &fundamentals) {
                r.mem.grow(c.len() * size_of::<usize>())?;
                found.push(c);
            }
        }
    }
    // Parallel edges and self-loops, each pair once.
    let mut pairs: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for v in 0..n {
        for (w, _) in p.out(v) {
            let w = w.index();
            if w >= v {
                *pairs.entry((v, w)).or_insert(0) += 1;
            }
        }
    }
    for ((a, b), c) in pairs {
        // Under UNDIRECTED a pair is seen once per edge from its smaller
        // end, a self-loop twice.
        let edges = if a == b { c / 2 } else { c };
        if edges > 1 || a == b {
            found.push(vec![a, b]);
        }
    }
    let mut rows = Vec::new();
    for (id, c) in found.iter().enumerate() {
        for v in c {
            r.emit(
                &mut rows,
                vec![Value::Int(id as i64), Value::Node(p.ids[*v])],
            )?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

fn run_circuits(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => circuits(p, r))
}

/// A task of graphalgs's search.
enum Task {
    Search(usize),
    Unblock(usize),
}

fn circuits<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    r.mem
        .grow(4 * n * size_of::<usize>() + p.edge_ids.len() * size_of::<usize>())?;
    let succ: Vec<Vec<usize>> = (0..n).map(|v| kernels::targets(p, v)).collect();
    let mut rows = Vec::new();
    let mut blocked = vec![false; n];
    let mut steps = 0usize;
    for s in 0..n {
        let is_pred: Vec<bool> = (0..n).map(|x| succ[x].binary_search(&s).is_ok()).collect();
        let mut path: Vec<usize> = Vec::new();
        let mut tasks = vec![Task::Search(s)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Search(v) => {
                    steps += 1;
                    if steps.is_multiple_of(4096) {
                        r.mem.check_now()?;
                    }
                    path.push(v);
                    blocked[v] = true;
                    tasks.push(Task::Unblock(v));
                    if is_pred[v] {
                        let mut closed = path.clone();
                        closed.push(s);
                        let (slots, _) = kernels::path_edges(p, false, &closed);
                        let ids: Vec<Value> = path.iter().map(|x| id_value(p, *x)).collect();
                        let row = vec![
                            Value::Int(rows.len() as i64),
                            Value::List(ids.into()),
                            kernels::path_value(p, &closed, &slots),
                        ];
                        r.emit(&mut rows, row)?;
                    }
                    for &w in &succ[v] {
                        if w > s && !blocked[w] {
                            tasks.push(Task::Search(w));
                        }
                    }
                }
                Task::Unblock(v) => {
                    blocked[v] = false;
                    path.pop();
                }
            }
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// MAGE's distance in metres between two points (`distance_calculator.py`
/// `calculate_distance_between_points`, metric `m`).
pub(crate) fn metres(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let r = 6371e3;
    let rad = std::f64::consts::PI / 180.00;
    let (phi1, phi2) = (lat1 * rad, lat2 * rad);
    let d_phi = (lat2 - lat1) * rad;
    let d_lambda = (lng2 - lng1) * rad;
    let a = (d_phi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (d_lambda / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}

/// The distance matrix of the projected nodes `points`.
fn distances<I: Ix>(p: &Projection<I>, points: &[usize]) -> Result<Vec<Vec<f64>>, Error> {
    let (lat, lng) = (&p.node_props[0], &p.node_props[1]);
    if points.iter().any(|v| lat[*v].is_nan() || lng[*v].is_nan()) {
        return Err(invalid("every point needs a numeric lat and lng property"));
    }
    let k = points.len();
    let mut dm = vec![vec![0.0; k]; k];
    for i in 0..k {
        for j in i + 1..k {
            let d = metres(
                lat[points[i]],
                lng[points[i]],
                lat[points[j]],
                lng[points[j]],
            );
            dm[i][j] = d;
            dm[j][i] = d;
        }
    }
    Ok(dm)
}

fn run_tsp(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => tsp(p, r))
}

fn tsp<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let points: Vec<usize> = match &r.args[0] {
        Value::List(l) => l
            .iter()
            .map(|v| match v {
                Value::Node(_) => node_index(p, v, "a point"),
                _ => Err(Error::new(
                    codes::C22G03,
                    "invalid value type: points must be a list of nodes",
                )),
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(Error::new(
                codes::C22G03,
                "invalid value type: points must be a list of nodes",
            ));
        }
    };
    let method = match &r.args[1] {
        Value::String(s) => s.to_lowercase(),
        Value::Null => "1.5_approx".to_string(),
        _ => {
            return Err(Error::new(
                codes::C22G03,
                "invalid value type: method must be a string",
            ));
        }
    };
    let k = points.len();
    r.mem.grow(k.saturating_mul(k).saturating_mul(16))?;
    let dm = distances(p, &points)?;
    let order = match method.as_str() {
        "2_approx" => two_approx(&dm),
        "greedy" => greedy(&dm),
        _ => christofides(&dm),
    };
    let node = |i: usize| Value::Node(p.ids[points[i]]);
    let sources: Vec<Value> = order[..order.len().saturating_sub(1)]
        .iter()
        .map(|i| node(*i))
        .collect();
    let destinations: Vec<Value> = order.iter().skip(1).map(|i| node(*i)).collect();
    let mut rows = Vec::new();
    r.emit(
        &mut rows,
        vec![
            Value::List(sources.into()),
            Value::List(destinations.into()),
        ],
    )?;
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// MAGE's `solve_greedy`: from point 0, the nearest unvisited point each
/// time, then back to 0.
pub(crate) fn greedy(dm: &[Vec<f64>]) -> Vec<usize> {
    let k = dm.len();
    if k == 0 {
        return Vec::new();
    }
    let mut path = vec![0];
    let mut visited = vec![false; k];
    visited[0] = true;
    while path.len() != k {
        let last = path[path.len() - 1];
        let mut best: Option<(usize, f64)> = None;
        for (i, &d) in dm[last].iter().enumerate() {
            if last != i && !visited[i] && best.is_none_or(|(_, b)| b > d) {
                best = Some((i, d));
            }
        }
        let (i, _) = best.expect("an unvisited point");
        visited[i] = true;
        path.push(i);
    }
    path.push(0);
    path
}

/// A minimum spanning tree of the complete graph `dm` by Kruskal's
/// algorithm (NetworkX's `minimum_spanning_tree` default), edges in the
/// order they join the tree.
fn mst(dm: &[Vec<f64>]) -> Vec<(usize, usize)> {
    let k = dm.len();
    let mut edges: Vec<(f64, usize, usize)> = Vec::new();
    for (i, row) in dm.iter().enumerate() {
        for (j, d) in row.iter().enumerate().skip(i + 1) {
            edges.push((*d, i, j));
        }
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))));
    let mut root: Vec<usize> = (0..k).collect();
    fn find(root: &mut [usize], mut x: usize) -> usize {
        while root[x] != x {
            root[x] = root[root[x]];
            x = root[x];
        }
        x
    }
    let mut tree = Vec::new();
    for (_, i, j) in edges {
        let (a, b) = (find(&mut root, i), find(&mut root, j));
        if a != b {
            root[a] = b;
            tree.push((i, j));
        }
    }
    tree
}

/// MAGE's `solve_2_approx`: the depth-first preorder of a minimum
/// spanning tree from point 0, closed.
pub(crate) fn two_approx(dm: &[Vec<f64>]) -> Vec<usize> {
    let k = dm.len();
    if k <= 1 {
        return vec![0; k.min(1) * 2];
    }
    let mut adj = vec![Vec::new(); k];
    for (i, j) in mst(dm) {
        adj[i].push(j);
        adj[j].push(i);
    }
    let mut path = Vec::new();
    let mut seen = vec![false; k];
    let mut stack = vec![0];
    while let Some(v) = stack.pop() {
        if seen[v] {
            continue;
        }
        seen[v] = true;
        path.push(v);
        for w in adj[v].iter().rev() {
            if !seen[*w] {
                stack.push(*w);
            }
        }
    }
    path.push(path[0]);
    path
}

/// Christofides' tour (module documentation): a minimum spanning tree, a
/// minimum-weight perfect matching of its odd-degree vertices, an Euler
/// circuit of their union from the first tree edge's first end, shortcut.
pub(crate) fn christofides(dm: &[Vec<f64>]) -> Vec<usize> {
    let k = dm.len();
    if k <= 2 {
        let mut p: Vec<usize> = (0..k).collect();
        if k > 0 {
            p.push(0);
        }
        return p;
    }
    let tree = mst(dm);
    let mut degree = vec![0usize; k];
    for (i, j) in &tree {
        degree[*i] += 1;
        degree[*j] += 1;
    }
    let odd: Vec<usize> = (0..k).filter(|v| degree[*v] % 2 == 1).collect();
    let mut edges = tree.clone();
    edges.extend(min_matching(dm, &odd));
    // Hierholzer's circuit over the multigraph `edges`.
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); k];
    for (e, (i, j)) in edges.iter().enumerate() {
        adj[*i].push((*j, e));
        adj[*j].push((*i, e));
    }
    let mut used = vec![false; edges.len()];
    let mut next = vec![0usize; k];
    let start = edges[0].0;
    let mut stack = vec![start];
    let mut circuit = Vec::new();
    while let Some(&v) = stack.last() {
        while next[v] < adj[v].len() && used[adj[v][next[v]].1] {
            next[v] += 1;
        }
        if next[v] == adj[v].len() {
            circuit.push(v);
            stack.pop();
        } else {
            let (w, e) = adj[v][next[v]];
            used[e] = true;
            stack.push(w);
        }
    }
    circuit.reverse();
    // MAGE's `get_hamiltonian_circuit`: first visits, closed.
    let mut seen = vec![false; k];
    let mut path: Vec<usize> = circuit
        .into_iter()
        .filter(|v| !std::mem::replace(&mut seen[*v], true))
        .collect();
    path.push(path[0]);
    path
}

/// A minimum-weight perfect matching of `odd` (an even number of points):
/// exact by dynamic programming over subsets up to 20 points, else
/// greedy by increasing distance (module documentation).
fn min_matching(dm: &[Vec<f64>], odd: &[usize]) -> Vec<(usize, usize)> {
    let q = odd.len();
    if q > 20 {
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for a in 0..q {
            for b in a + 1..q {
                pairs.push((dm[odd[a]][odd[b]], a, b));
            }
        }
        pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));
        let mut taken = vec![false; q];
        let mut out = Vec::new();
        for (_, a, b) in pairs {
            if !taken[a] && !taken[b] {
                taken[a] = true;
                taken[b] = true;
                out.push((odd[a], odd[b]));
            }
        }
        return out;
    }
    let full = (1usize << q) - 1;
    let mut best = vec![f64::INFINITY; full + 1];
    let mut choice = vec![(0usize, 0usize); full + 1];
    best[0] = 0.0;
    for mask in 1..=full {
        if mask.count_ones() % 2 == 1 {
            continue;
        }
        let a = mask.trailing_zeros() as usize;
        for b in a + 1..q {
            if mask & (1 << b) == 0 {
                continue;
            }
            let rest = mask & !(1 << a) & !(1 << b);
            let c = best[rest] + dm[odd[a]][odd[b]];
            if c < best[mask] {
                best[mask] = c;
                choice[mask] = (a, b);
            }
        }
    }
    let mut out = Vec::new();
    let mut mask = full;
    while mask != 0 {
        let (a, b) = choice[mask];
        out.push((odd[a], odd[b]));
        mask &= !(1 << a) & !(1 << b);
    }
    out
}

fn run_vrp(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => vrp(p, r))
}

/// The length of a route that starts and ends at the depot (index 0).
#[cfg(test)]
pub(crate) fn route_length(dm: &[Vec<f64>], route: &[usize]) -> f64 {
    let mut prev = 0;
    let mut total = 0.0;
    for v in route.iter().chain(std::iter::once(&0)) {
        total += dm[prev][*v];
        prev = *v;
    }
    total
}

/// 2-opt on one route (customers between two depot visits): the first
/// improving reversal is applied until none improves.
pub(crate) fn two_opt(dm: &[Vec<f64>], route: &mut [usize]) {
    let k = route.len();
    loop {
        let mut improved = false;
        for i in 0..k {
            for j in i + 1..k {
                let a = if i == 0 { 0 } else { route[i - 1] };
                let d = if j + 1 == k { 0 } else { route[j + 1] };
                let (b, c) = (route[i], route[j]);
                let delta = dm[a][c] + dm[b][d] - dm[a][b] - dm[c][d];
                if delta < -1e-9 {
                    route[i..=j].reverse();
                    improved = true;
                }
            }
        }
        if !improved {
            return;
        }
    }
}

/// Clarke-Wright savings then 2-opt (module documentation): routes of
/// indexes into `dm`, depot 0 left out.
pub(crate) fn savings(dm: &[Vec<f64>], vehicles: usize) -> Vec<Vec<usize>> {
    let k = dm.len();
    let mut routes: Vec<Option<Vec<usize>>> = (0..k).map(|i| (i > 0).then(|| vec![i])).collect();
    let mut of: Vec<usize> = (0..k).collect();
    let mut s: Vec<(f64, usize, usize)> = Vec::new();
    for i in 1..k {
        for j in i + 1..k {
            s.push((dm[0][i] + dm[0][j] - dm[i][j], i, j));
        }
    }
    s.sort_by(|x, y| y.0.total_cmp(&x.0).then((x.1, x.2).cmp(&(y.1, y.2))));
    let mut count = k.saturating_sub(1);
    for pass in 0..2 {
        for &(saving, i, j) in &s {
            if pass == 0 && saving <= 0.0 {
                break;
            }
            if pass == 1 && count <= vehicles {
                break;
            }
            let (ri, rj) = (of[i], of[j]);
            if ri == rj {
                continue;
            }
            let (a, b) = (
                routes[ri].as_ref().expect("a route"),
                routes[rj].as_ref().expect("a route"),
            );
            let i_end = a.first() == Some(&i) || a.last() == Some(&i);
            let j_end = b.first() == Some(&j) || b.last() == Some(&j);
            if !i_end || !j_end {
                continue;
            }
            let mut a = routes[ri].take().expect("a route");
            let mut b = routes[rj].take().expect("a route");
            // Join as … i, j …
            if a.last() != Some(&i) {
                a.reverse();
            }
            if b.first() != Some(&j) {
                b.reverse();
            }
            a.extend(b);
            for v in &a {
                of[*v] = ri;
            }
            routes[ri] = Some(a);
            count -= 1;
        }
    }
    let mut out: Vec<Vec<usize>> = routes.into_iter().flatten().collect();
    for route in &mut out {
        two_opt(dm, route);
    }
    out
}

fn vrp<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let depot = node_index(p, &r.args[0], "depot_node")?;
    let vehicles = match &r.args[1] {
        Value::Null => 1,
        Value::Int(v) if *v >= 1 => usize::try_from(*v).unwrap_or(usize::MAX),
        Value::Int(_) => return Err(invalid("number_of_vehicles must be greater than 0")),
        _ => {
            return Err(Error::new(
                codes::C22G03,
                "invalid value type: number_of_vehicles must be an integer",
            ));
        }
    };
    let n = p.node_count();
    r.mem.grow(n.saturating_mul(n).saturating_mul(16))?;
    // Index 0 is the depot, then the other nodes in index order.
    let mut points = vec![depot];
    points.extend((0..n).filter(|v| *v != depot));
    let dm = distances(p, &points)?;
    let routes = savings(&dm, vehicles);
    let mut rows = Vec::new();
    for route in routes {
        let mut prev = 0;
        for v in route.iter().chain(std::iter::once(&0)) {
            r.emit(
                &mut rows,
                vec![
                    Value::Node(p.ids[points[prev]]),
                    Value::Node(p.ids[points[*v]]),
                ],
            )?;
            prev = *v;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}
