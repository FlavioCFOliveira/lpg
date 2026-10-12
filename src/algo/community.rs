//! The community-detection family (rmp #66): ALG-061 Louvain, ALG-062
//! Leiden, ALG-063 label propagation, ALG-064 SLLPA, ALG-065 modularity
//! optimisation, ALG-078 approximate maximum k-cut and ALG-082 partition
//! quality (modularity, conductance). ALG-076, 077, 080 and 081 are in
//! [`super::clustering`].
//!
//! ALG-061 to 065 and 078 are parity entries (TS-25): verified by
//! determinism and a quality measure, not by equality with GDS, whose
//! parallel versions update shared state without order. ALG-082 equals
//! GDS and NetworkX.
//!
//! **Shared kernels.**
//!
//! * [`Net`]: the weighted, undirected graph of a run. Each edge the run's
//!   orientation shows from a node `u` to a node `v ≠ u` adds its weight
//!   (1 without `relationshipWeightProperty`) to `A[u][v]` and `A[v][u]`;
//!   a self-loop adds twice its weight to `A[u][u]` under `NATURAL` and
//!   `REVERSE`, its weight per appearance under `UNDIRECTED`, which shows
//!   it twice, so a loop counts 2 in a degree as in NetworkX. Under
//!   `UNDIRECTED` every weight is doubled, which changes no modularity.
//!   Parallel edges add up. `k[v] = Σ A[v][·]`, `2m = Σ k`.
//! * **Modularity** `Q = Σ_c (in_c / 2m − γ (tot_c / 2m)²)`, with `in_c`
//!   the sum of `A` inside community `c` and `tot_c` its degree sum
//!   (NetworkX `community/quality.py` `modularity`, BSD-3-Clause, ported).
//! * **Colour classes** ([`Net::colours`]): a greedy colouring in index
//!   order, so that two nodes of one class are never adjacent.
//! * **Local moving** ([`local_move`]): rounds over the colour classes in
//!   colour order. For each class, every node of the class computes, from
//!   the state before the class, the community that maximises `k_{v,c} −
//!   γ k_v tot_c / 2m` (its own community after leaving it included); a
//!   tie goes to the lower community index and staying wins a tie. The
//!   moves of the class are then applied in node order. Nodes of one class
//!   share no edge, so a node's gain does not depend on another's move of
//!   the same class; the classes are computed by blocks and applied in
//!   node order, so every run is the same at every `concurrency`. A run
//!   stops after a round without a move, after a round whose modularity
//!   gain is below `tolerance`, or after `maxIterations` rounds. This is
//!   the colouring method of Lu, Halappanavar and Kalyanaraman, "Parallel
//!   heuristics for scalable community detection" (Parallel Computing 47,
//!   2015), which GDS's modularity optimisation also applies
//!   (`modularityoptimization/ModularityOptimization.java`, colouring by
//!   `K1ColoringStub.java`; studied).
//!
//! **Entries.**
//!
//! * **061 Louvain**: local moving from singletons, then aggregation of
//!   each community into one node, level after level, until a level moves
//!   nothing, gains less than `tolerance` in modularity, or `maxLevels`
//!   levels ran.
//! * **062 Leiden** (Traag, Waltman and van Eck, Scientific Reports 9,
//!   5233, 2019): local moving, then a refinement of each community, then
//!   aggregation by the refined communities, whose nodes start in their
//!   unrefined community. The refinement is network_partitions'
//!   `subnetwork_clustering` (below), one community at a time, with the
//!   generator `ChaCha8Rng::seed_from_u64(randomSeed)`, stream `level <<
//!   40 | community`, so a community draws the same numbers whichever
//!   block refines it. `theta` is its randomness, `gamma` the resolution.
//! * **063 label propagation**: each node takes the label of greatest
//!   weight among its out-neighbours' (edge weight times the neighbour's
//!   `nodeWeightProperty`), the lowest label on a tie, as GDS's
//!   `labelpropagation/ComputeStepConsumer.java` tallies votes (studied);
//!   a node without neighbours keeps its label. Rounds run over the colour
//!   classes like local moving. Labels start as the node's element
//!   identifier, or its `seedProperty`.
//! * **064 SLLPA** (Xie, Szymanski and Liu, "SLPA", 2011; GDS
//!   `sllpa/SpeakerListenerLPAComputation.java`, studied): each node
//!   has `maxIterations` memory slots, all its own label at first. In
//!   superstep *s* = 1, 2, … the colour classes listen in turn: each node
//!   hears one label per incoming edge, drawn uniformly from the slots
//!   the speaker has written so far, and writes the most frequent into
//!   slot *s* (a tie drawn at random, where GDS takes the lowest label).
//!   GDS runs the computation asynchronously (`isAsynchronous` in
//!   `SpeakerListenerLPAConfig`), so a speaker may already have spoken
//!   in the same superstep; the colour classes give lpg that order
//!   deterministically. The draws come from the listener's block
//!   generator. The
//!   communities of a node are the labels whose share of its slots
//!   exceeds `minAssociationStrength`, the strongest first, then by
//!   element identifier.
//! * **065 modularity optimisation**: one level of local moving.
//! * **078 approximate maximum k-cut** (GDS `approxmaxkcut/*`, studied:
//!   GRASP with variable neighbourhood search; Feo and Resende, "Greedy
//!   randomized adaptive search procedures", J. Global Optimization 6,
//!   1995): `iterations` restarts, each a greedy randomised construction
//!   (the nodes in a random order, each placed in the community holding
//!   the least weight of its placed neighbours, the lowest on a tie)
//!   followed by local search in colour-class rounds, where a node moves
//!   to the community that most increases the cut weight, as long as no
//!   community falls below `minCommunitySize`. Nodes of one class share
//!   no edge, so their moves are those of a sequential search. Then
//!   Fiduccia-Mattheyses passes (Fiduccia and Mattheyses, "A linear-time
//!   heuristic for improving network partitions", DAC 1982), on one
//!   worker: each node moves once, the move of greatest gain first (the
//!   lowest node on a tie) even when it loses, and the best prefix of the
//!   moves is kept, while a pass gains. With
//!   `vnsMaxNeighborhoodOrder` above 0, the solution is perturbed by
//!   moving 1, 2, … nodes at random and searched again, keeping a strict
//!   improvement. The best cut of all restarts wins, the first on a tie.
//!   The order and the perturbations draw from stream 0. GDS places the
//!   nodes uniformly at random instead (`PlaceNodesRandomly.java`); the
//!   greedy construction is the classic GRASP one.
//! * **082** (GDS `modularity/ModularityCalculator.java`,
//!   `RelationshipCountCollector.java`, `conductance/Conductance.java`;
//!   studied): over the outgoing edges in the run's orientation, community
//!   `c`'s modularity is `(e_c − K_c² / m) / m`, where `e_c` sums the
//!   weights of edges inside `c`, `K_c` those of edges leaving a node of
//!   `c`, `m` all; its conductance is `ext_c / (ext_c + int_c)` for the
//!   communities with an edge, a negative or absent community skipped.
//!
//! **Community identifiers.** With `consecutiveIds`, communities are
//! numbered 0, 1, … in the order of their first node; otherwise a
//! community is named by the element identifier of its first node.
//! `minCommunitySize` leaves out the nodes of smaller communities.
//!
//! **Origin.** The refinement is copied from network_partitions
//! (graspologic-native) @ `773181488b4e`
//! (<https://github.com/graspologic-org/graspologic-native>, MIT),
//! `packages/network_partitions/src/leiden/subnetwork.rs`
//! (`subnetwork_clustering`, `node_can_move`, `best_cluster_for_node`,
//! `approximate_exponent`) and `quality_value_increment.rs`
//! (`as_reference_impl`), modified: lpg's arrays and generator. Its
//! licence:
//!
//! > MIT License
//! >
//! > Copyright (c) Microsoft Corporation.
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining
//! > a copy of this software and associated documentation files (the
//! > "Software"), to deal in the Software without restriction, including
//! > without limitation the rights to use, copy, modify, merge, publish,
//! > distribute, sublicense, and/or sell copies of the Software, and to
//! > permit persons to whom the Software is furnished to do so, subject to
//! > the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be
//! > included in all copies or substantial portions of the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
//! > EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! > MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
//! > NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
//! > BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
//! > ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
//! > CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! > SOFTWARE.
//!
//! The modularity formula is ported from NetworkX @ `6da4704cbf32`
//! `networkx/algorithms/community/quality.py` (BSD-3-Clause). Its
//! licence (`LICENSE.txt`):
//!
//! > Copyright (c) 2004-2026, NetworkX Developers
//! > Aric Hagberg <hagberg@lanl.gov>
//! > Dan Schult <dschult@colgate.edu>
//! > Pieter Swart <swart@lanl.gov>
//! > All rights reserved.
//! >
//! > Redistribution and use in source and binary forms, with or without
//! > modification, are permitted provided that the following conditions are
//! > met:
//! >
//! > * Redistributions of source code must retain the above copyright
//! > notice, this list of conditions and the following disclaimer.
//! >
//! > * Redistributions in binary form must reproduce the above
//! > copyright notice, this list of conditions and the following
//! > disclaimer in the documentation and/or other materials provided
//! > with the distribution.
//! >
//! > * Neither the name of the NetworkX Developers nor the names of its
//! > contributors may be used to endorse or promote products derived
//! > from this software without specific prior written permission.
//! >
//! > THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
//! > "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
//! > LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
//! > A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
//! > OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//! > SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
//! > LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
//! > DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
//! > THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
//! > (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
//! > OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::collections::BTreeMap;
use std::sync::Arc;

use rand::RngExt;

use super::config::{Def, Key, Ty};
use super::projection::{Ix, Orientation, Projection, with_projection};
use super::sched::{Sched, rng};
use super::{Entry, Output, Run, WriteKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// The weighted undirected graph of a community run (module
/// documentation): neighbours other than the node itself, sorted, with
/// merged weights.
#[derive(Debug, Clone)]
pub(crate) struct Net {
    row: Vec<usize>,
    col: Vec<usize>,
    w: Vec<f64>,
    /// `A[v][v]`.
    selfw: Vec<f64>,
    /// `k[v]`.
    pub(crate) k: Vec<f64>,
    /// `2m`.
    pub(crate) two_m: f64,
}

impl Net {
    /// The net of a projection (module documentation).
    pub(crate) fn of<I: Ix>(p: &Projection<I>, weighted: bool) -> Net {
        let n = p.node_count();
        let loop_factor = if p.orientation == Orientation::Undirected {
            1.0
        } else {
            2.0
        };
        let mut lists: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        let mut selfw = vec![0.0; n];
        for u in 0..n {
            for (v, slot) in p.out(u) {
                let v = v.index();
                let w = if weighted { p.edge_prop(0, slot) } else { 1.0 };
                if u == v {
                    selfw[u] += loop_factor * w;
                } else {
                    lists[u].push((v, w));
                    lists[v].push((u, w));
                }
            }
        }
        Net::from_lists(lists, selfw)
    }

    fn from_lists(mut lists: Vec<Vec<(usize, f64)>>, selfw: Vec<f64>) -> Net {
        let n = lists.len();
        let mut row = Vec::with_capacity(n + 1);
        let mut col = Vec::new();
        let mut w = Vec::new();
        row.push(0);
        for l in &mut lists {
            l.sort_by_key(|x| x.0);
            let mut last = usize::MAX;
            for (v, x) in l.drain(..) {
                if v == last {
                    *w.last_mut().expect("a previous entry") += x;
                } else {
                    col.push(v);
                    w.push(x);
                    last = v;
                }
            }
            row.push(col.len());
        }
        let k: Vec<f64> = (0..n)
            .map(|v| w[row[v]..row[v + 1]].iter().sum::<f64>() + selfw[v])
            .collect();
        let two_m = k.iter().sum();
        Net {
            row,
            col,
            w,
            selfw,
            k,
            two_m,
        }
    }

    /// The number of nodes.
    pub(crate) fn len(&self) -> usize {
        self.k.len()
    }

    /// The neighbours of `v` with their weights.
    pub(crate) fn adj(&self, v: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let r = self.row[v]..self.row[v + 1];
        self.col[r.clone()]
            .iter()
            .copied()
            .zip(self.w[r].iter().copied())
    }

    /// `A[u][v]` for all pairs, as a dense matrix (spectral clustering).
    pub(crate) fn weight(&self, u: usize, v: usize) -> f64 {
        if u == v {
            return self.selfw[u];
        }
        let r = self.row[u]..self.row[u + 1];
        match self.col[r.clone()].binary_search(&v) {
            Ok(i) => self.w[r.start + i],
            Err(_) => 0.0,
        }
    }

    /// The net of `part`'s `count` communities: each community one node.
    fn aggregate(&self, part: &[usize], count: usize) -> Net {
        let mut lists: Vec<Vec<(usize, f64)>> = vec![Vec::new(); count];
        let mut selfw = vec![0.0; count];
        for u in 0..self.len() {
            let cu = part[u];
            selfw[cu] += self.selfw[u];
            for (v, w) in self.adj(u) {
                let cv = part[v];
                if cu == cv {
                    selfw[cu] += w;
                } else {
                    lists[cu].push((cv, w));
                }
            }
        }
        Net::from_lists(lists, selfw)
    }

    /// The greedy colour classes (module documentation), each sorted.
    fn colours(&self) -> Vec<Vec<usize>> {
        let n = self.len();
        let mut colour = vec![usize::MAX; n];
        let mut classes: Vec<Vec<usize>> = Vec::new();
        let mut used: Vec<bool> = Vec::new();
        for v in 0..n {
            used.clear();
            for (u, _) in self.adj(v) {
                if colour[u] != usize::MAX {
                    if used.len() <= colour[u] {
                        used.resize(colour[u] + 1, false);
                    }
                    used[colour[u]] = true;
                }
            }
            let c = used.iter().position(|x| !x).unwrap_or(used.len());
            colour[v] = c;
            if classes.len() <= c {
                classes.push(Vec::new());
            }
            classes[c].push(v);
        }
        classes
    }

    /// The bytes of the arrays.
    fn bytes(&self) -> usize {
        self.row.len() * 8 + self.col.len() * 8 + self.w.len() * 8 + self.k.len() * 16
    }

    /// Modularity of `part` (module documentation).
    pub(crate) fn modularity(&self, part: &[usize], gamma: f64) -> f64 {
        if self.two_m == 0.0 {
            return 0.0;
        }
        let mut inside: BTreeMap<usize, f64> = BTreeMap::new();
        let mut tot: BTreeMap<usize, f64> = BTreeMap::new();
        for u in 0..self.len() {
            *tot.entry(part[u]).or_default() += self.k[u];
            let mut x = self.selfw[u];
            for (v, w) in self.adj(u) {
                if part[v] == part[u] {
                    x += w;
                }
            }
            *inside.entry(part[u]).or_default() += x;
        }
        tot.iter()
            .map(|(c, t)| inside[c] / self.two_m - gamma * (t / self.two_m) * (t / self.two_m))
            .sum()
    }
}

/// Renumbers `part` to `0..count` in the order of first appearance;
/// returns `count`.
fn renumber(part: &mut [usize]) -> usize {
    let mut map: BTreeMap<usize, usize> = BTreeMap::new();
    for c in part.iter_mut() {
        let next = map.len();
        *c = *map.entry(*c).or_insert(next);
    }
    map.len()
}

/// The best community of `v` (module documentation): `None` to stay.
fn best_move(net: &Net, part: &[usize], tot: &[f64], v: usize, gamma: f64) -> Option<usize> {
    let own = part[v];
    let kv = net.k[v];
    let mut links: Vec<(usize, f64)> = net.adj(v).map(|(u, w)| (part[u], w)).collect();
    links.sort_by_key(|x| x.0);
    let scale = gamma * kv / net.two_m;
    let own_link: f64 = links.iter().filter(|x| x.0 == own).map(|x| x.1).sum();
    let stay = own_link - scale * (tot[own] - kv);
    let mut best = (stay, None);
    let mut i = 0;
    while i < links.len() {
        let c = links[i].0;
        let mut x = 0.0;
        while i < links.len() && links[i].0 == c {
            x += links[i].1;
            i += 1;
        }
        if c == own {
            continue;
        }
        let gain = x - scale * tot[c];
        if gain > best.0 {
            best = (gain, Some(c));
        }
    }
    best.1
}

/// Local moving (module documentation) on `part`, whose communities are
/// node indexes of `net`. Returns the rounds run, whether any node moved,
/// and whether the run stopped by itself before `max_rounds`.
fn local_move(
    net: &Net,
    sched: &Sched,
    part: &mut [usize],
    gamma: f64,
    max_rounds: usize,
    tolerance: f64,
) -> Result<(usize, bool, bool), Error> {
    let n = net.len();
    let mut tot = vec![0.0; n];
    for v in 0..n {
        tot[part[v]] += net.k[v];
    }
    let classes = net.colours();
    let mut q = net.modularity(part, gamma);
    let mut moved_any = false;
    for round in 1..=max_rounds {
        let mut moved = 0usize;
        for class in &classes {
            let (snapshot, tot_ref) = (&*part, &tot);
            let moves = sched.map(|_, range| {
                let lo = class.partition_point(|v| *v < range.start);
                let hi = class.partition_point(|v| *v < range.end);
                class[lo..hi]
                    .iter()
                    .filter_map(|v| best_move(net, snapshot, tot_ref, *v, gamma).map(|c| (*v, c)))
                    .collect::<Vec<_>>()
            })?;
            for (v, c) in moves.into_iter().flatten() {
                tot[part[v]] -= net.k[v];
                tot[c] += net.k[v];
                part[v] = c;
                moved += 1;
            }
        }
        if moved == 0 {
            return Ok((round, moved_any, true));
        }
        moved_any = true;
        let next = net.modularity(part, gamma);
        let gain = next - q;
        q = next;
        if gain < tolerance {
            return Ok((round, moved_any, true));
        }
    }
    Ok((max_rounds, moved_any, false))
}

const TOLERANCE: Key = Key::new(
    "tolerance",
    Ty::Float {
        min: 0.0,
        max: f64::INFINITY,
        max_open: false,
    },
    Def::Float(1.0e-4),
);
const SEED_PROPERTY: Key = Key::new("seedProperty", Ty::Str, Def::Absent);
const CONSECUTIVE: Key = Key::new("consecutiveIds", Ty::Bool, Def::Bool(false));
const MIN_SIZE: Key = Key::new("minCommunitySize", Ty::Int { min: 0 }, Def::Absent);
const FORCE_SEED: Key = Key::new("forceSeedOptimization", Ty::Bool, Def::Bool(false));
const WEIGHT: Key = Key::new("relationshipWeightProperty", Ty::Str, Def::Absent);
const MAX_ITERATIONS: Key = Key::new("maxIterations", Ty::Int { min: 1 }, Def::Int(10));
const MAX_LEVELS: Key = Key::new("maxLevels", Ty::Int { min: 1 }, Def::Int(10));
const INTERMEDIATE: Key = Key::new("includeIntermediateCommunities", Ty::Bool, Def::Bool(false));

/// The stream columns of Louvain and Leiden.
const LEVEL_ROWS: &[&str] = &["nodeId", "intermediateCommunityIds", "communityId"];

/// ALG-061: `gds.louvain.stream`, `gds.louvain.write`.
pub(crate) const LOUVAIN: Entry = Entry {
    algs: &[61],
    stream: "gds.louvain.stream",
    write: Some("gds.louvain.write"),
    keys: &[
        MAX_ITERATIONS,
        MAX_LEVELS,
        TOLERANCE,
        INTERMEDIATE,
        SEED_PROPERTY,
        CONSECUTIVE,
        MIN_SIZE,
        FORCE_SEED,
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["seedProperty"],
    stream_columns: LEVEL_ROWS,
    write_columns: &[
        "modularity",
        "modularities",
        "ranLevels",
        "communityCount",
        "communityDistribution",
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "configuration",
    ],
    run: run_louvain,
    write_kind: WriteKind::NodeProp { id: 0, value: 2 },
    ..Entry::BASE
};

/// ALG-062: `gds.leiden.stream`, `gds.leiden.write`.
pub(crate) const LEIDEN: Entry = Entry {
    algs: &[62],
    stream: "gds.leiden.stream",
    write: Some("gds.leiden.write"),
    keys: &[
        MAX_LEVELS,
        Key::new(
            "gamma",
            Ty::Float {
                min: 0.0,
                max: f64::INFINITY,
                max_open: false,
            },
            Def::Float(1.0),
        ),
        Key::new(
            "theta",
            Ty::Float {
                min: 0.0,
                max: f64::INFINITY,
                max_open: false,
            },
            Def::Float(0.01),
        ),
        TOLERANCE,
        INTERMEDIATE,
        SEED_PROPERTY,
        CONSECUTIVE,
        MIN_SIZE,
        FORCE_SEED,
        WEIGHT,
    ],
    parallel: true,
    randomised: true,
    undirected: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["seedProperty"],
    stream_columns: LEVEL_ROWS,
    write_columns: &[
        "ranLevels",
        "didConverge",
        "nodeCount",
        "communityCount",
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "communityDistribution",
        "modularities",
        "modularity",
        "configuration",
    ],
    run: run_leiden,
    write_kind: WriteKind::NodeProp { id: 0, value: 2 },
    ..Entry::BASE
};

/// ALG-063: `gds.labelPropagation.stream`, `.write`.
pub(crate) const LABEL_PROPAGATION: Entry = Entry {
    algs: &[63],
    stream: "gds.labelPropagation.stream",
    write: Some("gds.labelPropagation.write"),
    keys: &[
        MAX_ITERATIONS,
        Key::new("nodeWeightProperty", Ty::Str, Def::Absent),
        SEED_PROPERTY,
        CONSECUTIVE,
        MIN_SIZE,
        FORCE_SEED,
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["seedProperty", "nodeWeightProperty"],
    stream_columns: &["nodeId", "communityId"],
    write_columns: &[
        "ranIterations",
        "didConverge",
        "communityCount",
        "communityDistribution",
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "configuration",
    ],
    run: run_label_propagation,
    ..Entry::BASE
};

/// ALG-064: `gds.sllpa.stream`, `gds.sllpa.write`.
pub(crate) const SLLPA: Entry = Entry {
    algs: &[64],
    stream: "gds.sllpa.stream",
    write: Some("gds.sllpa.write"),
    keys: &[
        Key::new("maxIterations", Ty::Int { min: 1 }, Def::Required),
        Key::new(
            "minAssociationStrength",
            Ty::Float {
                min: 0.0,
                max: 1.0,
                max_open: false,
            },
            Def::Float(0.2),
        ),
        Key::new("partitioning", Ty::Str, Def::Str("RANGE")),
        Key::new("mutateProperty", Ty::Str, Def::Absent),
    ],
    parallel: true,
    randomised: true,
    stream_columns: &["nodeId", "values"],
    write_columns: &[
        "ranIterations",
        "didConverge",
        "preProcessingMillis",
        "computeMillis",
        "writeMillis",
        "nodePropertiesWritten",
        "configuration",
    ],
    run: run_sllpa,
    write_kind: WriteKind::NodeField {
        id: 0,
        map: 1,
        field: "communityIds",
    },
    ..Entry::BASE
};

/// ALG-065: `gds.modularityOptimization.stream`, `.write`.
pub(crate) const MODULARITY_OPTIMIZATION: Entry = Entry {
    algs: &[65],
    stream: "gds.modularityOptimization.stream",
    write: Some("gds.modularityOptimization.write"),
    keys: &[
        MAX_ITERATIONS,
        TOLERANCE,
        SEED_PROPERTY,
        CONSECUTIVE,
        MIN_SIZE,
        FORCE_SEED,
        Key::new("batchSize", Ty::Int { min: 1 }, Def::Int(10_000)),
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["seedProperty"],
    stream_columns: &["nodeId", "communityId"],
    write_columns: &[
        "preProcessingMillis",
        "computeMillis",
        "postProcessingMillis",
        "writeMillis",
        "nodes",
        "didConverge",
        "ranIterations",
        "modularity",
        "communityCount",
        "communityDistribution",
        "configuration",
    ],
    run: run_modularity_optimization,
    ..Entry::BASE
};

/// ALG-078: `gds.maxkcut.stream` (no write mode).
pub(crate) const MAX_K_CUT: Entry = Entry {
    algs: &[78],
    stream: "gds.maxkcut.stream",
    keys: &[
        Key::new("k", Ty::Int { min: 2 }, Def::Int(2)),
        Key::new("iterations", Ty::Int { min: 1 }, Def::Int(8)),
        Key::new("vnsMaxNeighborhoodOrder", Ty::Int { min: 0 }, Def::Int(0)),
        MIN_SIZE,
        WEIGHT,
    ],
    parallel: true,
    randomised: true,
    edge_props: &["relationshipWeightProperty"],
    stream_columns: &["nodeId", "communityId"],
    run: run_max_k_cut,
    ..Entry::BASE
};

/// ALG-082: `gds.modularity.stream`.
pub(crate) const MODULARITY: Entry = Entry {
    algs: &[82],
    stream: "gds.modularity.stream",
    keys: &[
        Key::new("communityProperty", Ty::Str, Def::Required),
        WEIGHT,
    ],
    parallel: true,
    edge_props: &["relationshipWeightProperty"],
    node_props: &["communityProperty"],
    stream_columns: &["communityId", "modularity"],
    run: run_modularity,
    ..Entry::BASE
};

/// ALG-082: `gds.conductance.stream`.
pub(crate) const CONDUCTANCE: Entry = Entry {
    stream: "gds.conductance.stream",
    stream_columns: &["community", "conductance"],
    run: run_conductance,
    ..MODULARITY
};

/// The weighted flag of a run.
fn weighted(r: &Run<'_, '_>) -> bool {
    r.cfg.str("relationshipWeightProperty").is_some()
}

/// The initial partition: singletons, or the nodes grouped by the value
/// of `seedProperty` (the first node property projected) when given; a
/// NaN seed keeps its node alone.
fn initial<I: Ix>(p: &Projection<I>, r: &Run<'_, '_>) -> Vec<usize> {
    let n = p.node_count();
    if !r.cfg.has("seedProperty") {
        return (0..n).collect();
    }
    let seeds = &p.node_props[0];
    let mut first: BTreeMap<u64, usize> = BTreeMap::new();
    (0..n)
        .map(|v| {
            let s = seeds[v];
            if s.is_nan() {
                v
            } else {
                *first.entry(s.to_bits()).or_insert(v)
            }
        })
        .collect()
}

/// The community identifiers of `labels` (module documentation): the
/// first node's element identifier, or `0, 1, …` with `consecutiveIds`.
fn ids<I: Ix>(p: &Projection<I>, labels: &[usize], consecutive: bool) -> Vec<i64> {
    let mut first: BTreeMap<usize, i64> = BTreeMap::new();
    labels
        .iter()
        .enumerate()
        .map(|(v, c)| {
            let next = first.len() as i64;
            *first.entry(*c).or_insert(if consecutive {
                next
            } else {
                p.ids[v].cast_signed()
            })
        })
        .collect()
}

/// `communityDistribution`: the community sizes' `min`, `max`, `mean`
/// and nearest-rank percentiles (GDS `HistogramUtils`, keys studied).
fn size_distribution(labels: &[usize]) -> Value {
    let mut counts: BTreeMap<usize, i64> = BTreeMap::new();
    for c in labels {
        *counts.entry(*c).or_default() += 1;
    }
    let mut xs: Vec<i64> = counts.into_values().collect();
    if xs.is_empty() {
        return Value::Record(Vec::new().into());
    }
    xs.sort_unstable();
    let at = |q: f64| {
        let rank = (q / 100.0 * xs.len() as f64).ceil() as usize;
        Value::Int(xs[rank.clamp(1, xs.len()) - 1])
    };
    let f = |k: &str, v: Value| (Arc::<str>::from(k), v);
    Value::Record(
        vec![
            f("min", Value::Int(xs[0])),
            f(
                "mean",
                Value::Float(xs.iter().sum::<i64>() as f64 / xs.len() as f64),
            ),
            f("max", Value::Int(xs[xs.len() - 1])),
            f("p50", at(50.0)),
            f("p75", at(75.0)),
            f("p90", at(90.0)),
            f("p95", at(95.0)),
            f("p99", at(99.0)),
            f("p999", at(99.9)),
        ]
        .into(),
    )
}

/// The node indexes `minCommunitySize` keeps.
fn kept(r: &Run<'_, '_>, labels: &[usize]) -> Vec<bool> {
    let min = match r.cfg.get("minCommunitySize") {
        Value::Int(m) => usize::try_from(*m).unwrap_or(usize::MAX),
        _ => 0,
    };
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for c in labels {
        *counts.entry(*c).or_default() += 1;
    }
    labels.iter().map(|c| counts[c] >= min).collect()
}

fn count(labels: &[usize]) -> i64 {
    let mut v = labels.to_vec();
    v.sort_unstable();
    v.dedup();
    v.len() as i64
}

/// The rows of a partition: `(nodeId, communityId)`.
fn partition_rows<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    labels: &[usize],
) -> Result<Vec<Vec<Value>>, Error> {
    let ids = ids(p, labels, r.cfg.bool("consecutiveIds"));
    let keep = kept(r, labels);
    let mut rows = Vec::new();
    for v in 0..labels.len() {
        if keep[v] {
            r.emit(
                &mut rows,
                vec![Value::Int(p.ids[v].cast_signed()), Value::Int(ids[v])],
            )?;
        }
    }
    Ok(rows)
}

/// What a level-based run (Louvain, Leiden) computed.
struct Levels {
    /// The community of each original node after each level.
    levels: Vec<Vec<usize>>,
    modularities: Vec<f64>,
    converged: bool,
}

/// The rows and summary of a level-based run.
fn level_output<I: Ix>(
    p: &Projection<I>,
    r: &mut Run<'_, '_>,
    run: Levels,
) -> Result<Output, Error> {
    let n = p.node_count();
    let last: Vec<usize> = run
        .levels
        .last()
        .cloned()
        .unwrap_or_else(|| (0..n).collect());
    let consecutive = r.cfg.bool("consecutiveIds");
    let level_ids: Vec<Vec<i64>> = run.levels.iter().map(|l| ids(p, l, consecutive)).collect();
    let final_ids = ids(p, &last, consecutive);
    let keep = kept(r, &last);
    let intermediate = r.cfg.bool("includeIntermediateCommunities");
    let mut rows = Vec::new();
    for v in 0..n {
        if !keep[v] {
            continue;
        }
        let inter = if intermediate {
            Value::List(
                level_ids
                    .iter()
                    .map(|l| Value::Int(l[v]))
                    .collect::<Vec<_>>()
                    .into(),
            )
        } else {
            Value::Null
        };
        r.emit(
            &mut rows,
            vec![
                Value::Int(p.ids[v].cast_signed()),
                inter,
                Value::Int(final_ids[v]),
            ],
        )?;
    }
    let q = run.modularities.last().copied().unwrap_or(0.0);
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("modularity", Value::Float(q)),
            (
                "modularities",
                Value::List(
                    run.modularities
                        .iter()
                        .map(|x| Value::Float(*x))
                        .collect::<Vec<_>>()
                        .into(),
                ),
            ),
            ("ranLevels", Value::Int(run.levels.len() as i64)),
            ("communityCount", Value::Int(count(&last))),
            ("communityDistribution", size_distribution(&last)),
            ("didConverge", Value::Bool(run.converged)),
            ("nodeCount", Value::Int(n as i64)),
        ],
    })
}

fn run_louvain(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => louvain(p, r))
}

fn louvain<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let net = Net::of(p, weighted(r));
    r.mem.grow(3 * net.bytes() + n * 24)?;
    let max_levels = usize::try_from(r.cfg.int("maxLevels")).unwrap_or(usize::MAX);
    let rounds = usize::try_from(r.cfg.int("maxIterations")).unwrap_or(usize::MAX);
    let tolerance = r.cfg.float("tolerance");
    let mut part = initial(p, r);
    let mut node_of: Vec<usize> = (0..n).collect();
    let mut net = net;
    let mut levels = Vec::new();
    let mut modularities = Vec::new();
    let mut converged = false;
    let mut sched = r.sched.resized(net.len());
    while levels.len() < max_levels {
        r.mem.check_now()?;
        let (_, moved, _) = local_move(&net, &sched, &mut part, 1.0, rounds, tolerance)?;
        let c = renumber(&mut part);
        if !moved && !levels.is_empty() {
            converged = true;
            break;
        }
        let q = net.modularity(&part, 1.0);
        let membership: Vec<usize> = node_of.iter().map(|v| part[*v]).collect();
        let gain = q - modularities.last().copied().unwrap_or(f64::NEG_INFINITY);
        levels.push(membership);
        modularities.push(q);
        if c == net.len() || gain < tolerance {
            converged = true;
            break;
        }
        for v in &mut node_of {
            *v = part[*v];
        }
        net = net.aggregate(&part, c);
        part = (0..c).collect();
        sched = r.sched.resized(c);
    }
    level_output(
        p,
        r,
        Levels {
            levels,
            modularities,
            converged,
        },
    )
}

fn run_leiden(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => leiden(p, r))
}

fn leiden<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let mut net = Net::of(p, weighted(r));
    r.mem.grow(3 * net.bytes() + n * 32)?;
    let max_levels = usize::try_from(r.cfg.int("maxLevels")).unwrap_or(usize::MAX);
    let gamma = r.cfg.float("gamma");
    let theta = r.cfg.float("theta");
    let tolerance = r.cfg.float("tolerance");
    let seed = r.cfg.seed();
    let mut part = initial(p, r);
    let mut node_of: Vec<usize> = (0..n).collect();
    let mut levels = Vec::new();
    let mut modularities = Vec::new();
    let mut converged = false;
    let mut sched = r.sched.resized(net.len());
    let rounds = usize::MAX;
    while levels.len() < max_levels {
        r.mem.check_now()?;
        let (_, moved, _) = local_move(&net, &sched, &mut part, gamma, rounds, 0.0)?;
        let c = renumber(&mut part);
        if !moved && !levels.is_empty() {
            converged = true;
            break;
        }
        let q = net.modularity(&part, gamma);
        let gain = q - modularities.last().copied().unwrap_or(f64::NEG_INFINITY);
        levels.push(node_of.iter().map(|v| part[*v]).collect::<Vec<usize>>());
        modularities.push(q);
        if gain < tolerance && levels.len() > 1 {
            converged = true;
            break;
        }
        let refined = refine(&net, &sched, &part, c, gamma, theta, seed, levels.len() - 1)?;
        let mut refined = refined;
        let rc = renumber(&mut refined);
        if rc == net.len() {
            converged = true;
            break;
        }
        let mut next = vec![0usize; rc];
        for v in 0..net.len() {
            next[refined[v]] = part[v];
        }
        for v in &mut node_of {
            *v = refined[*v];
        }
        net = net.aggregate(&refined, rc);
        part = next;
        sched = r.sched.resized(rc);
    }
    level_output(
        p,
        r,
        Levels {
            levels,
            modularities,
            converged,
        },
    )
}

/// The refined partition of `part`'s `count` communities (module
/// documentation): each community refined alone, by blocks of
/// communities; returns a label per node of `net`.
#[allow(clippy::too_many_arguments, reason = "the parts of one refinement")]
fn refine(
    net: &Net,
    sched: &Sched,
    part: &[usize],
    count: usize,
    gamma: f64,
    theta: f64,
    seed: u64,
    level: usize,
) -> Result<Vec<usize>, Error> {
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); count];
    for v in 0..net.len() {
        members[part[v]].push(v);
    }
    let res = gamma / net.two_m;
    let by_community = sched.resized(count);
    let members = &members;
    let parts = by_community.map(|_, range| {
        range
            .map(|c| {
                let mut g = rng(seed, 0);
                g.set_stream(((level as u64) << 40) | c as u64);
                subnetwork_clustering(net, part, &members[c], res, theta, &mut g)
            })
            .collect::<Vec<_>>()
    })?;
    let mut out = vec![0usize; net.len()];
    for (c, local) in parts.into_iter().flatten().enumerate() {
        for (i, v) in members[c].iter().enumerate() {
            // Labels unique across communities: the first member's index
            // in `net` of the refined cluster.
            out[*v] = members[c][local[i]];
        }
    }
    Ok(out)
}

/// network_partitions' `subnetwork_clustering` on the members of one
/// community (module documentation): the cluster of each member, as an
/// index into `members`.
fn subnetwork_clustering(
    net: &Net,
    part: &[usize],
    members: &[usize],
    res: f64,
    randomness: f64,
    rng: &mut rand::rngs::ChaCha8Rng,
) -> Vec<usize> {
    let len = members.len();
    let mut clustering: Vec<usize> = (0..len).collect();
    if len == 1 {
        return clustering;
    }
    let local = |v: usize| members.binary_search(&v).ok();
    let c0 = part[members[0]];
    let adj: Vec<Vec<(usize, f64)>> = members
        .iter()
        .map(|v| {
            net.adj(*v)
                .filter(|(u, _)| part[*u] == c0)
                .filter_map(|(u, w)| local(u).map(|i| (i, w)))
                .collect()
        })
        .collect();
    let mut order: Vec<usize> = (0..len).collect();
    for i in 0..len {
        let j = rng.random_range(0..len);
        order.swap(i, j);
    }
    let node_weight: Vec<f64> = members.iter().map(|v| net.k[*v]).collect();
    let mut cluster_weights = node_weight.clone();
    let mut external = node_weight.clone();
    let total: f64 = node_weight.iter().sum();
    let mut singleton = vec![true; len];
    let mut nb_weights = vec![0.0f64; len];
    let mut neighbouring: Vec<usize> = Vec::new();
    let mut summed: Vec<f64> = Vec::new();
    for &node in &order {
        let threshold = cluster_weights[node] * (total - cluster_weights[node]) * res;
        if !(singleton[node] && external[node] >= threshold) {
            continue;
        }
        for c in &neighbouring {
            nb_weights[*c] = 0.0;
        }
        neighbouring.clear();
        neighbouring.push(node);
        summed.clear();
        cluster_weights[node] = 0.0;
        external[node] = 0.0;
        for (u, w) in &adj[node] {
            let c = clustering[*u];
            if nb_weights[c] == 0.0 {
                neighbouring.push(c);
            }
            nb_weights[c] += w;
        }
        // best_cluster_for_node.
        let mut best = node;
        let mut max_qvi = 0.0f64;
        let mut total_adjusted = 0.0f64;
        for &c in &neighbouring {
            if external[c] >= cluster_weights[c] * (total - cluster_weights[c]) * res {
                let qvi = nb_weights[c] - node_weight[node] * cluster_weights[c] * res;
                if qvi > max_qvi {
                    best = c;
                    max_qvi = qvi;
                }
                if qvi >= 0.0 {
                    total_adjusted += approximate_exponent(qvi / randomness);
                }
            }
            if !total_adjusted.is_nan() {
                summed.push(total_adjusted);
            }
            nb_weights[c] = 0.0;
        }
        let chosen = if total_adjusted.is_finite() {
            let x = total_adjusted * rng.random::<f64>();
            let at = match summed.binary_search_by(|probe| probe.total_cmp(&x)) {
                Ok(i) | Err(i) => i,
            };
            neighbouring[at.min(neighbouring.len() - 1)]
        } else {
            best
        };
        cluster_weights[chosen] += node_weight[node];
        for (u, w) in &adj[node] {
            if clustering[*u] == chosen {
                external[chosen] -= w;
            } else {
                external[chosen] += w;
            }
        }
        if chosen != node {
            clustering[node] = chosen;
            singleton[chosen] = false;
        }
    }
    // Labels as the first member of each cluster.
    let mut first: BTreeMap<usize, usize> = BTreeMap::new();
    clustering
        .iter()
        .enumerate()
        .map(|(i, c)| *first.entry(*c).or_insert(i))
        .collect()
}

/// network_partitions' `approximate_exponent`.
fn approximate_exponent(x: f64) -> f64 {
    if x < -256.0 {
        0.0
    } else {
        let mut y = 1.0 + x / 256.0;
        for _ in 0..8 {
            y *= y;
        }
        y
    }
}

fn run_modularity_optimization(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => {
        let n = p.node_count();
        let net = Net::of(p, weighted(r));
        r.mem.grow(2 * net.bytes() + n * 16)?;
        let mut part = initial(p, r);
        let rounds = usize::try_from(r.cfg.int("maxIterations")).unwrap_or(usize::MAX);
        let tolerance = r.cfg.float("tolerance");
        let sched = r.sched.resized(n);
        let (ran, _, stopped) = local_move(&net, &sched, &mut part, 1.0, rounds, tolerance)?;
        let q = net.modularity(&part, 1.0);
        let rows = partition_rows(p, r, &part)?;
        Ok(Output::Rows {
            rows,
            extra: vec![
                ("nodes", Value::Int(n as i64)),
                ("didConverge", Value::Bool(stopped)),
                ("ranIterations", Value::Int(ran as i64)),
                ("modularity", Value::Float(q)),
                ("communityCount", Value::Int(count(&part))),
                ("communityDistribution", size_distribution(&part)),
            ],
        })
    })
}

fn run_label_propagation(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => label_propagation(p, r))
}

fn label_propagation<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let w = weighted(r);
    let net = Net::of(p, false);
    r.mem.grow(net.bytes() + n * 24)?;
    let classes = net.colours();
    drop(net);
    let seeded = r.cfg.has("seedProperty");
    let node_weight: Option<usize> = r.cfg.has("nodeWeightProperty").then(|| usize::from(seeded));
    // Labels are element identifiers (or seed values) as `u64` bits of
    // an `i64`, compared as `i64`.
    let mut label: Vec<i64> = (0..n)
        .map(|v| {
            if seeded {
                let s = p.node_props[0][v];
                if s.is_nan() {
                    p.ids[v].cast_signed()
                } else {
                    s as i64
                }
            } else {
                p.ids[v].cast_signed()
            }
        })
        .collect();
    let rounds = r.cfg.int("maxIterations").cast_unsigned();
    let sched = r.sched;
    let mut ran = 0u64;
    let mut converged = false;
    while ran < rounds {
        r.mem.check_now()?;
        ran += 1;
        let round = ran.rotate_left(48);
        let mut changed = 0usize;
        for class in &classes {
            let current = &label;
            let updates = sched.map(|_, range| {
                let lo = class.partition_point(|v| *v < range.start);
                let hi = class.partition_point(|v| *v < range.end);
                let mut out = Vec::new();
                for &v in &class[lo..hi] {
                    let mut votes: Vec<(i64, f64)> = p
                        .out(v)
                        .map(|(u, slot)| {
                            let u = u.index();
                            let ew = if w { p.edge_prop(0, slot) } else { 1.0 };
                            let nw = node_weight.map_or(1.0, |i| p.node_props[i][u]);
                            (current[u], ew * nw)
                        })
                        .collect();
                    if votes.is_empty() {
                        continue;
                    }
                    votes.sort_by_key(|x| x.0);
                    let mut tally: Vec<(i64, f64)> = Vec::new();
                    for (l, x) in votes {
                        match tally.last_mut() {
                            Some(t) if t.0 == l => t.1 += x,
                            _ => tally.push((l, x)),
                        }
                    }
                    let max = tally.iter().map(|t| t.1).fold(f64::NEG_INFINITY, f64::max);
                    let best: Vec<i64> = tally.iter().filter(|t| t.1 == max).map(|t| t.0).collect();
                    if best.contains(&current[v]) {
                        continue;
                    }
                    let pick = best
                        .iter()
                        .copied()
                        .min_by_key(|l| {
                            (
                                mix(l.cast_unsigned() ^ (v as u64).rotate_left(32) ^ round),
                                *l,
                            )
                        })
                        .expect("a label");
                    out.push((v, pick));
                }
                out
            })?;
            for (v, l) in updates.into_iter().flatten() {
                label[v] = l;
                changed += 1;
            }
        }
        if changed == 0 {
            converged = true;
            break;
        }
    }
    let mut dense: BTreeMap<i64, usize> = BTreeMap::new();
    let parts: Vec<usize> = label
        .iter()
        .map(|l| {
            let next = dense.len();
            *dense.entry(*l).or_insert(next)
        })
        .collect();
    let consecutive = r.cfg.bool("consecutiveIds");
    let keep = kept(r, &parts);
    let consecutive_ids = ids(p, &parts, true);
    let mut rows = Vec::new();
    for v in 0..n {
        if keep[v] {
            let id = if consecutive {
                consecutive_ids[v]
            } else {
                label[v]
            };
            r.emit(
                &mut rows,
                vec![Value::Int(p.ids[v].cast_signed()), Value::Int(id)],
            )?;
        }
    }
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("ranIterations", Value::Int(ran as i64)),
            ("didConverge", Value::Bool(converged)),
            ("communityCount", Value::Int(count(&parts))),
            ("communityDistribution", size_distribution(&parts)),
        ],
    })
}

/// SplitMix64's finaliser (Steele, Lea and Flood, "Fast splittable
/// pseudorandom number generators", 2014): the tie-break order of label
/// propagation.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn run_sllpa(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => sllpa(p, r))
}

fn sllpa<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let steps = usize::try_from(r.cfg.int("maxIterations")).unwrap_or(usize::MAX);
    let min_strength = r.cfg.float("minAssociationStrength");
    let seed = r.cfg.seed();
    r.mem.grow(
        n.saturating_mul(steps)
            .saturating_mul(2 * size_of::<usize>()),
    )?;
    // Each node's memory: one label (a node index) per superstep, its
    // own until a superstep writes it.
    let mut memory: Vec<Vec<usize>> = (0..n).map(|v| vec![v; steps]).collect();
    let sched = r.sched;
    let blocks = n.div_ceil(super::sched::block_size(n));
    let gens: Vec<std::sync::Mutex<rand::rngs::ChaCha8Rng>> = (0..blocks)
        .map(|b| std::sync::Mutex::new(rng(seed, b)))
        .collect();
    let classes = Net::of(p, false).colours();
    // The slots each node has written.
    let mut written = vec![1usize; n];
    for step in 1..steps {
        r.mem.check_now()?;
        for class in &classes {
            let (mem_ref, done) = (&memory, &written);
            let heard = sched.map(|b, range| {
                let mut g = gens[b]
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let lo = class.partition_point(|v| *v < range.start);
                let hi = class.partition_point(|v| *v < range.end);
                class[lo..hi]
                    .iter()
                    .map(|&v| {
                        // One label per incoming edge, drawn from the
                        // slots the speaker has written.
                        let mut ls: Vec<usize> = p
                            .inc(v)
                            .map(|(u, _)| {
                                let u = u.index();
                                mem_ref[u][g.random_range(0..done[u])]
                            })
                            .collect();
                        if ls.is_empty() {
                            return (v, None);
                        }
                        ls.sort_unstable();
                        let mut best: Vec<usize> = Vec::new();
                        let mut most = 0;
                        let mut i = 0;
                        while i < ls.len() {
                            let j = i + ls[i..].partition_point(|x| *x == ls[i]);
                            if j - i > most {
                                most = j - i;
                                best.clear();
                            }
                            if j - i == most {
                                best.push(ls[i]);
                            }
                            i = j;
                        }
                        // A tie is drawn from the listener's generator.
                        let l = if best.len() == 1 {
                            best[0]
                        } else {
                            best[g.random_range(0..best.len())]
                        };
                        (v, Some(l))
                    })
                    .collect::<Vec<_>>()
            })?;
            for (v, l) in heard.into_iter().flatten() {
                if let Some(l) = l {
                    memory[v][step] = l;
                }
                written[v] = step + 1;
            }
        }
    }
    let mut rows = Vec::new();
    let key = Arc::<str>::from("communityIds");
    for (v, slots) in memory.iter().enumerate() {
        let mut m = slots.clone();
        m.sort_unstable();
        let mut kept: Vec<(usize, usize)> = Vec::new();
        let mut i = 0;
        while i < m.len() {
            let j = i + m[i..].partition_point(|x| *x == m[i]);
            if (j - i) as f64 / m.len() as f64 > min_strength {
                kept.push((j - i, m[i]));
            }
            i = j;
        }
        // The strongest label first, then by index.
        kept.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let ids: Vec<Value> = kept
            .iter()
            .map(|(_, l)| Value::Int(p.ids[*l].cast_signed()))
            .collect();
        let values = Value::Record(vec![(key.clone(), Value::List(ids.into()))].into());
        r.emit(&mut rows, vec![Value::Int(p.ids[v].cast_signed()), values])?;
    }
    Ok(Output::Rows {
        rows,
        extra: vec![
            ("ranIterations", Value::Int(steps as i64)),
            ("didConverge", Value::Bool(false)),
        ],
    })
}

fn run_max_k_cut(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => max_k_cut(p, r))
}

/// The weight of the edges of `net` whose ends are in different
/// communities, each pair once.
fn cut(net: &Net, part: &[usize]) -> f64 {
    let mut x = 0.0;
    for u in 0..net.len() {
        for (v, w) in net.adj(u) {
            if u < v && part[u] != part[v] {
                x += w;
            }
        }
    }
    x
}

/// One Fiduccia-Mattheyses pass over a k-cut (module documentation):
/// every node moves once, the best move first even when it loses, then
/// the best prefix of the moves is kept; returns its gain (0 when no
/// prefix gains, and nothing changes).
fn refine_cut(net: &Net, part: &mut [usize], k: usize, min: usize) -> f64 {
    use std::collections::BinaryHeap;
    let n = net.len();
    let mut sizes = vec![0usize; k];
    for c in part.iter() {
        sizes[*c] += 1;
    }
    // The best move of `v`: (gain, target community).
    let best = |part: &[usize], v: usize| -> (f64, usize) {
        let mut to = vec![0.0f64; k];
        for (u, w) in net.adj(v) {
            to[part[u]] += w;
        }
        let own = part[v];
        let mut b = (f64::NEG_INFINITY, own);
        for (c, x) in to.iter().enumerate() {
            if c != own && to[own] - x > b.0 {
                b = (to[own] - x, c);
            }
        }
        b
    };
    #[derive(PartialEq)]
    struct Move(f64, usize, usize, u64);
    impl Eq for Move {}
    impl PartialOrd for Move {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Move {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            self.0.total_cmp(&o.0).then(o.1.cmp(&self.1))
        }
    }
    let mut version = vec![0u64; n];
    let mut locked = vec![false; n];
    let mut heap: BinaryHeap<Move> = (0..n)
        .map(|v| {
            let (g, c) = best(part, v);
            Move(g, v, c, 0)
        })
        .collect();
    let mut moves: Vec<(usize, usize)> = Vec::new();
    let (mut total, mut best_total, mut best_len) = (0.0f64, 0.0f64, 0usize);
    while let Some(Move(g, v, c, ver)) = heap.pop() {
        if locked[v] || ver != version[v] || c == part[v] || !g.is_finite() {
            continue;
        }
        let own = part[v];
        if sizes[own] <= min {
            locked[v] = true;
            continue;
        }
        sizes[own] -= 1;
        sizes[c] += 1;
        part[v] = c;
        locked[v] = true;
        moves.push((v, own));
        total += g;
        if total > best_total {
            best_total = total;
            best_len = moves.len();
        }
        for (u, _) in net.adj(v) {
            if !locked[u] {
                version[u] += 1;
                let (g, c) = best(part, u);
                heap.push(Move(g, u, c, version[u]));
            }
        }
    }
    for (v, own) in moves.drain(best_len..).rev() {
        part[v] = own;
    }
    best_total
}

/// Local search of a k-cut (module documentation).
fn cut_search(
    net: &Net,
    sched: &Sched,
    classes: &[Vec<usize>],
    part: &mut [usize],
    k: usize,
    min: usize,
) -> Result<(), Error> {
    let mut sizes = vec![0usize; k];
    for c in part.iter() {
        sizes[*c] += 1;
    }
    loop {
        let mut moved = 0;
        for class in classes {
            let snapshot = &*part;
            let moves = sched.map(|_, range| {
                let lo = class.partition_point(|v| *v < range.start);
                let hi = class.partition_point(|v| *v < range.end);
                let mut out = Vec::new();
                for &v in &class[lo..hi] {
                    let mut to = vec![0.0f64; k];
                    for (u, w) in net.adj(v) {
                        to[snapshot[u]] += w;
                    }
                    let own = snapshot[v];
                    let mut best = (0.0, own);
                    for (c, x) in to.iter().enumerate() {
                        let gain = to[own] - x;
                        if c != own && gain > best.0 {
                            best = (gain, c);
                        }
                    }
                    if best.1 != own {
                        out.push((v, best.1));
                    }
                }
                out
            })?;
            for (v, c) in moves.into_iter().flatten() {
                let own = part[v];
                if sizes[own] <= min {
                    continue;
                }
                sizes[own] -= 1;
                sizes[c] += 1;
                part[v] = c;
                moved += 1;
            }
        }
        if moved == 0 {
            return Ok(());
        }
    }
}

fn max_k_cut<I: Ix>(p: &Projection<I>, r: &mut Run<'_, '_>) -> Result<Output, Error> {
    let n = p.node_count();
    let k = usize::try_from(r.cfg.int("k")).unwrap_or(usize::MAX);
    let restarts = usize::try_from(r.cfg.int("iterations")).unwrap_or(usize::MAX);
    let order = usize::try_from(r.cfg.int("vnsMaxNeighborhoodOrder")).unwrap_or(0);
    let min = match r.cfg.get("minCommunitySize") {
        Value::Int(m) => usize::try_from(*m).unwrap_or(usize::MAX),
        _ => 0,
    };
    if k.saturating_mul(min) > n {
        return Err(Error::new(
            codes::C22N11,
            "invalid argument: k communities of minCommunitySize nodes need more nodes",
        ));
    }
    let seed = r.cfg.seed();
    let net = Net::of(p, weighted(r));
    r.mem.grow(2 * net.bytes() + n * 32)?;
    let classes = net.colours();
    let sched = r.sched;
    // The construction and the perturbations are sequential: one
    // generator, stream 0 (TS-25 Randomness, one worker).
    let mut g = rng(seed, 0);
    let mut best: Option<(f64, Vec<usize>)> = None;
    for _ in 0..restarts {
        r.mem.check_now()?;
        // GRASP construction: nodes in a random order, each into the
        // community holding the least weight of its placed neighbours.
        let mut perm: Vec<usize> = (0..n).collect();
        for i in 0..n {
            let j = g.random_range(i..n);
            perm.swap(i, j);
        }
        let mut part = vec![usize::MAX; n];
        for &v in &perm {
            let mut to = vec![0.0f64; k];
            for (u, w) in net.adj(v) {
                if part[u] != usize::MAX {
                    to[part[u]] += w;
                }
            }
            let mut c = 0;
            for (i, x) in to.iter().enumerate() {
                if *x < to[c] {
                    c = i;
                }
            }
            part[v] = c;
        }
        // Meet `minCommunitySize`: move nodes, from the largest
        // communities, in node order, into the smallest.
        let mut sizes = vec![0usize; k];
        for c in &part {
            sizes[*c] += 1;
        }
        for slot in part.iter_mut() {
            let (small, s) = sizes
                .iter()
                .enumerate()
                .min_by_key(|(c, s)| (**s, *c))
                .map(|(c, s)| (c, *s))
                .unwrap_or((0, 0));
            if s >= min {
                break;
            }
            let own = *slot;
            if sizes[own] > min && own != small {
                sizes[own] -= 1;
                sizes[small] += 1;
                *slot = small;
            }
        }
        cut_search(&net, sched, &classes, &mut part, k, min)?;
        while refine_cut(&net, &mut part, k, min) > 0.0 {}
        let mut value = cut(&net, &part);
        for o in 1..=order {
            let mut trial = part.clone();
            let mut sizes = vec![0usize; k];
            for c in &trial {
                sizes[*c] += 1;
            }
            for _ in 0..o {
                let v = g.random_range(0..n);
                let c = g.random_range(0..k);
                if sizes[trial[v]] > min {
                    sizes[trial[v]] -= 1;
                    sizes[c] += 1;
                    trial[v] = c;
                }
            }
            cut_search(&net, sched, &classes, &mut trial, k, min)?;
            while refine_cut(&net, &mut trial, k, min) > 0.0 {}
            let x = cut(&net, &trial);
            if x > value {
                value = x;
                part = trial;
            }
        }
        if best.as_ref().is_none_or(|(b, _)| value > *b) {
            best = Some((value, part));
        }
    }
    let part = best.map(|b| b.1).unwrap_or_default();
    let mut rows = Vec::new();
    for (id, c) in p.ids.iter().zip(&part) {
        r.emit(
            &mut rows,
            vec![Value::Int(id.cast_signed()), Value::Int(*c as i64)],
        )?;
    }
    Ok(Output::Rows {
        rows,
        extra: Vec::new(),
    })
}

/// The community of each node for ALG-082: the property's value as an
/// integer, `None` for NaN or a negative value.
fn communities<I: Ix>(p: &Projection<I>) -> Vec<Option<i64>> {
    p.node_props[0]
        .iter()
        .map(|x| (!x.is_nan() && *x >= 0.0).then_some(*x as i64))
        .collect()
}

/// Per community: the weight of its inside edges and of all edges
/// leaving its nodes.
type Counts = BTreeMap<i64, (f64, f64)>;

/// [`Counts`] by blocks merged in block order, and the total weight.
fn counts<I: Ix>(
    p: &Projection<I>,
    r: &Run<'_, '_>,
    comm: &[Option<i64>],
) -> Result<(Counts, f64), Error> {
    let w = weighted(r);
    let parts = r.sched.map(|_, range| {
        let mut m: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
        let mut total = 0.0;
        for v in range {
            let Some(c) = comm[v] else { continue };
            let e = m.entry(c).or_default();
            for (u, slot) in p.out(v) {
                let x = if w { p.edge_prop(0, slot) } else { 1.0 };
                if comm[u.index()] == Some(c) {
                    e.0 += x;
                }
                e.1 += x;
                total += x;
            }
        }
        (m, total)
    })?;
    let mut out: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
    let mut total = 0.0;
    for (m, t) in parts {
        total += t;
        for (c, (i, a)) in m {
            let e = out.entry(c).or_default();
            e.0 += i;
            e.1 += a;
        }
    }
    Ok((out, total))
}

fn run_modularity(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => {
        let comm = communities(p);
        let (m, total) = counts(p, r, &comm)?;
        let mut rows = Vec::new();
        for (c, (inside, all)) in m {
            let q = (inside - all * all * (1.0 / total)) / total;
            r.emit(&mut rows, vec![Value::Int(c), Value::Float(q)])?;
        }
        Ok(Output::Rows { rows, extra: Vec::new() })
    })
}

fn run_conductance(r: &mut Run<'_, '_>) -> Result<Output, Error> {
    with_projection!(r.graph, p => {
        let comm = communities(p);
        let (m, _) = counts(p, r, &comm)?;
        let mut rows = Vec::new();
        for (c, (inside, all)) in m {
            let external = all - inside;
            if all > 0.0 {
                r.emit(
                    &mut rows,
                    vec![Value::Int(c), Value::Float(external / (external + inside))],
                )?;
            }
        }
        Ok(Output::Rows { rows, extra: Vec::new() })
    })
}
