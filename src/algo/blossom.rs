//! Maximum-weight matching of a general graph by Edmonds' blossom
//! algorithm with dual variables (Galil's `O(n³)` formulation), for
//! ALG-187's minimum-weight perfect matching (rmp #68).
//!
//! **Origin.** rustworkx-core @ `25398ab47f66`
//! (<https://github.com/Qiskit/rustworkx>, Apache-2.0, see
//! `LICENSE-APACHE`; the rustworkx contributors, the file carries no
//! copyright line), `rustworkx-core/src/max_weight_matching.rs`
//! (`max_weight_matching`, `slack`, `blossom_leaves`, `assign_label`,
//! `scan_blossom`, `add_blossom`, `expand_blossom`, `augment_blossom`,
//! `augment_matching`), itself a port of Joris van Rantwijk's
//! `mwmatching.py` as NetworkX's `max_weight_matching` keeps it. This file
//! was modified: the shared state is one struct instead of function
//! arguments; Python's negative list indexes are one helper ([`at`])
//! instead of inline arithmetic; the mate map is a vector, so the result
//! is in node order instead of a hash set's; the insertion-ordered map of
//! `add_blossom` is two vectors; `verify_optimum` is not kept (the tests
//! check optimality against exhaustive search and NetworkX); and in
//! `augment_blossom` a sub-blossom is recognised by `index >= n`, as
//! NetworkX's `isinstance(t, Blossom)`, where rustworkx tests `> n` and
//! misses the blossom numbered `n`.
//!
//! Weights are integers (`i128`): dual variables are kept doubled, so
//! every quantity stays an integer, as in rustworkx.

/// Python's `v[i]`: a negative `i` counts from the end.
fn at(v: &[usize], i: i128) -> usize {
    let len = v.len() as i128;
    v[if i < 0 {
        (len + i) as usize
    } else {
        i as usize
    }]
}

/// The state of one run (module documentation).
struct State<'a> {
    n: usize,
    edges: &'a [(usize, usize, i128)],
    endpoints: Vec<usize>,
    neighbor_endpoints: Vec<Vec<usize>>,
    mate: Vec<Option<usize>>,
    labels: Vec<Option<usize>>,
    label_ends: Vec<Option<usize>>,
    in_blossoms: Vec<usize>,
    blossom_parents: Vec<Option<usize>>,
    blossom_children: Vec<Vec<usize>>,
    blossom_base: Vec<Option<usize>>,
    blossom_endpoints: Vec<Vec<usize>>,
    best_edge: Vec<Option<usize>>,
    blossom_best_edges: Vec<Vec<usize>>,
    unused_blossoms: Vec<usize>,
    dual_var: Vec<i128>,
    allowed_edge: Vec<bool>,
    queue: Vec<usize>,
}

impl State<'_> {
    /// Twice the slack of edge `k` (not valid inside blossoms).
    fn slack(&self, k: usize) -> i128 {
        let (i, j, w) = self.edges[k];
        self.dual_var[i] + self.dual_var[j] - 2 * w
    }

    /// The leaf vertices of blossom `b`.
    fn leaves(&self, b: usize) -> Vec<usize> {
        if b < self.n {
            return vec![b];
        }
        let mut out = Vec::new();
        for c in &self.blossom_children[b] {
            if *c < self.n {
                out.push(*c);
            } else {
                out.extend(self.leaves(*c));
            }
        }
        out
    }

    fn assign_label(&mut self, w: usize, t: usize, p: Option<usize>) {
        let b = self.in_blossoms[w];
        debug_assert!(self.labels[w] == Some(0) && self.labels[b] == Some(0));
        self.labels[w] = Some(t);
        self.labels[b] = Some(t);
        self.label_ends[b] = p;
        self.label_ends[w] = p;
        self.best_edge[w] = None;
        self.best_edge[b] = None;
        if t == 1 {
            let leaves = self.leaves(b);
            self.queue.extend(leaves);
        } else if t == 2 {
            let base = self.blossom_base[b].expect("a base");
            let m = self.mate[base].expect("a matched base");
            self.assign_label(self.endpoints[m], 1, Some(m ^ 1));
        }
    }

    /// The base of the blossom closing at edge `v`-`w`, or `None` when
    /// the edge joins two trees (an augmenting path).
    fn scan_blossom(&mut self, v: usize, w: usize) -> Option<usize> {
        let mut v = Some(v);
        let mut w = Some(w);
        let mut path = Vec::new();
        let mut base = None;
        while let Some(vv) = v {
            let mut b = self.in_blossoms[vv];
            if self.labels[b].is_none_or(|l| l & 4 > 0) {
                base = self.blossom_base[b];
                break;
            }
            debug_assert!(self.labels[b] == Some(1));
            path.push(b);
            self.labels[b] = Some(5);
            match self.label_ends[b] {
                None => v = None,
                Some(le) => {
                    let t = self.endpoints[le];
                    b = self.in_blossoms[t];
                    debug_assert!(self.labels[b] == Some(2));
                    v = Some(self.endpoints[self.label_ends[b].expect("a T label end")]);
                }
            }
            if w.is_some() {
                std::mem::swap(&mut v, &mut w);
            }
        }
        for b in path {
            self.labels[b] = Some(1);
        }
        base
    }

    fn add_blossom(&mut self, base: usize, k: usize) {
        let (mut v, mut w, _) = self.edges[k];
        let bb = self.in_blossoms[base];
        let mut bv = self.in_blossoms[v];
        let mut bw = self.in_blossoms[w];
        let b = self.unused_blossoms.pop().expect("a free blossom");
        self.blossom_base[b] = Some(base);
        self.blossom_parents[b] = None;
        self.blossom_parents[bb] = Some(b);
        self.blossom_children[b].clear();
        self.blossom_endpoints[b].clear();
        while bv != bb {
            self.blossom_parents[bv] = Some(b);
            self.blossom_children[b].push(bv);
            let le = self.label_ends[bv].expect("a label end");
            self.blossom_endpoints[b].push(le);
            v = self.endpoints[le];
            bv = self.in_blossoms[v];
        }
        self.blossom_children[b].push(bb);
        self.blossom_children[b].reverse();
        self.blossom_endpoints[b].reverse();
        self.blossom_endpoints[b].push(2 * k);
        while bw != bb {
            self.blossom_parents[bw] = Some(b);
            self.blossom_children[b].push(bw);
            let le = self.label_ends[bw].expect("a label end");
            self.blossom_endpoints[b].push(le ^ 1);
            w = self.endpoints[le];
            bw = self.in_blossoms[w];
        }
        debug_assert!(self.labels[bb] == Some(1));
        self.labels[b] = Some(1);
        self.label_ends[b] = self.label_ends[bb];
        self.dual_var[b] = 0;
        for node in self.leaves(b) {
            if self.labels[self.in_blossoms[node]] == Some(2) {
                self.queue.push(node);
            }
            self.in_blossoms[node] = b;
        }
        // The least-slack edge to each other S-blossom, in insertion order.
        let mut best_to: Vec<Option<usize>> = vec![None; 2 * self.n];
        let mut order: Vec<usize> = Vec::new();
        for bv in self.blossom_children[b].clone() {
            let lists: Vec<Vec<usize>> = if self.blossom_best_edges[bv].is_empty() {
                self.leaves(bv)
                    .into_iter()
                    .map(|node| {
                        self.neighbor_endpoints[node]
                            .iter()
                            .map(|p| p / 2)
                            .collect()
                    })
                    .collect()
            } else {
                vec![self.blossom_best_edges[bv].clone()]
            };
            for list in lists {
                for k in list {
                    let (mut i, mut j, _) = self.edges[k];
                    if self.in_blossoms[j] == b {
                        std::mem::swap(&mut i, &mut j);
                    }
                    let bj = self.in_blossoms[j];
                    if bj != b
                        && self.labels[bj] == Some(1)
                        && best_to[bj].is_none_or(|e| self.slack(k) < self.slack(e))
                    {
                        if best_to[bj].is_none() {
                            order.push(bj);
                        }
                        best_to[bj] = Some(k);
                    }
                }
            }
            self.blossom_best_edges[bv].clear();
            self.best_edge[bv] = None;
        }
        self.blossom_best_edges[b] = order.iter().map(|x| best_to[*x].expect("set")).collect();
        self.best_edge[b] = None;
        for k in self.blossom_best_edges[b].clone() {
            if self.best_edge[b].is_none_or(|e| self.slack(k) < self.slack(e)) {
                self.best_edge[b] = Some(k);
            }
        }
    }

    fn expand_blossom(&mut self, b: usize, end_stage: bool) {
        for s in self.blossom_children[b].clone() {
            self.blossom_parents[s] = None;
            if s < self.n {
                self.in_blossoms[s] = s;
            } else if end_stage && self.dual_var[s] == 0 {
                self.expand_blossom(s, end_stage);
            } else {
                for v in self.leaves(s) {
                    self.in_blossoms[v] = s;
                }
            }
        }
        if !end_stage && self.labels[b] == Some(2) {
            let le = self.label_ends[b].expect("a label end");
            let entry = self.in_blossoms[self.endpoints[le ^ 1]];
            let i = self.blossom_children[b]
                .iter()
                .position(|x| *x == entry)
                .expect("the entry child");
            let (mut j, step, trick): (i128, i128, usize) = if i & 1 != 0 {
                (i as i128 - self.blossom_children[b].len() as i128, 1, 0)
            } else {
                (i as i128, -1, 1)
            };
            let mut p = le;
            while j != 0 {
                self.labels[self.endpoints[p ^ 1]] = Some(0);
                let e = at(&self.blossom_endpoints[b], j - trick as i128);
                self.labels[self.endpoints[e ^ trick ^ 1]] = Some(0);
                self.assign_label(self.endpoints[p ^ 1], 2, Some(p));
                self.allowed_edge[e / 2] = true;
                j += step;
                p = at(&self.blossom_endpoints[b], j - trick as i128) ^ trick;
                self.allowed_edge[p / 2] = true;
                j += step;
            }
            let bv = at(&self.blossom_children[b], j);
            self.labels[self.endpoints[p ^ 1]] = Some(2);
            self.labels[bv] = Some(2);
            self.label_ends[self.endpoints[p ^ 1]] = Some(p);
            self.label_ends[bv] = Some(p);
            self.best_edge[bv] = None;
            j += step;
            while at(&self.blossom_children[b], j) != entry {
                let bv = at(&self.blossom_children[b], j);
                if self.labels[bv] == Some(1) {
                    j += step;
                    continue;
                }
                let mut v = 0;
                for leaf in self.leaves(bv) {
                    v = leaf;
                    if self.labels[v] != Some(0) {
                        break;
                    }
                }
                if self.labels[v] != Some(0) {
                    debug_assert!(self.labels[v] == Some(2));
                    debug_assert!(self.in_blossoms[v] == bv);
                    self.labels[v] = Some(0);
                    let base = self.blossom_base[bv].expect("a base");
                    let m = self.mate[base].expect("a matched base");
                    self.labels[self.endpoints[m]] = Some(0);
                    self.assign_label(v, 2, self.label_ends[v]);
                }
                j += step;
            }
        }
        self.labels[b] = None;
        self.label_ends[b] = None;
        self.blossom_children[b].clear();
        self.blossom_endpoints[b].clear();
        self.blossom_base[b] = None;
        self.best_edge[b] = None;
        self.unused_blossoms.push(b);
    }

    /// Swaps the matched and unmatched edges along the path from vertex
    /// `v` to the base of blossom `b`, and makes `v` the base.
    fn augment_blossom(&mut self, b: usize, v: usize) {
        let mut t = v;
        while self.blossom_parents[t] != Some(b) {
            t = self.blossom_parents[t].expect("a parent");
        }
        if t >= self.n {
            self.augment_blossom(t, v);
        }
        let i = self.blossom_children[b]
            .iter()
            .position(|x| *x == t)
            .expect("the child");
        let (mut j, step, trick): (i128, i128, usize) = if i & 1 != 0 {
            (i as i128 - self.blossom_children[b].len() as i128, 1, 0)
        } else {
            (i as i128, -1, 1)
        };
        while j != 0 {
            j += step;
            let t = at(&self.blossom_children[b], j);
            let p = at(&self.blossom_endpoints[b], j - trick as i128) ^ trick;
            if t >= self.n {
                self.augment_blossom(t, self.endpoints[p]);
            }
            j += step;
            let t = at(&self.blossom_children[b], j);
            if t >= self.n {
                self.augment_blossom(t, self.endpoints[p ^ 1]);
            }
            self.mate[self.endpoints[p]] = Some(p ^ 1);
            self.mate[self.endpoints[p ^ 1]] = Some(p);
        }
        self.blossom_children[b].rotate_left(i);
        self.blossom_endpoints[b].rotate_left(i);
        self.blossom_base[b] = self.blossom_base[self.blossom_children[b][0]];
        debug_assert!(self.blossom_base[b] == Some(v));
    }

    /// Swaps the matched and unmatched edges along the augmenting path
    /// through edge `k`.
    fn augment_matching(&mut self, k: usize) {
        let (v, w, _) = self.edges[k];
        for (s0, p0) in [(v, 2 * k + 1), (w, 2 * k)] {
            let (mut s, mut p) = (s0, p0);
            loop {
                let bs = self.in_blossoms[s];
                debug_assert!(self.labels[bs] == Some(1));
                if bs >= self.n {
                    self.augment_blossom(bs, s);
                }
                self.mate[s] = Some(p);
                let Some(le) = self.label_ends[bs] else {
                    break;
                };
                let t = self.endpoints[le];
                let bt = self.in_blossoms[t];
                debug_assert!(self.labels[bt] == Some(2));
                let lt = self.label_ends[bt].expect("a T label end");
                s = self.endpoints[lt];
                let j = self.endpoints[lt ^ 1];
                debug_assert!(self.blossom_base[bt] == Some(t));
                if bt >= self.n {
                    self.augment_blossom(bt, j);
                }
                self.mate[j] = Some(lt);
                p = lt ^ 1;
            }
        }
    }
}

/// A maximum-weight matching of the graph of `n` vertices and `edges`
/// (`(u, v, weight)`, `u ≠ v`), of maximum cardinality first when
/// `max_cardinality`: each vertex's mate.
pub(crate) fn max_weight_matching(
    n: usize,
    edges: &[(usize, usize, i128)],
    max_cardinality: bool,
) -> Vec<Option<usize>> {
    let m = edges.len();
    if m == 0 {
        return vec![None; n];
    }
    let max_weight = edges.iter().map(|e| e.2).max().unwrap_or(0).max(0);
    let mut endpoints = Vec::with_capacity(2 * m);
    for (u, v, _) in edges {
        endpoints.push(*u);
        endpoints.push(*v);
    }
    let mut neighbor_endpoints = vec![Vec::new(); n];
    for (k, (u, v, _)) in edges.iter().enumerate() {
        neighbor_endpoints[*u].push(2 * k + 1);
        neighbor_endpoints[*v].push(2 * k);
    }
    let mut blossom_base: Vec<Option<usize>> = (0..n).map(Some).collect();
    blossom_base.extend(std::iter::repeat_n(None, n));
    let mut dual_var = vec![max_weight; n];
    dual_var.extend(std::iter::repeat_n(0, n));
    let mut s = State {
        n,
        edges,
        endpoints,
        neighbor_endpoints,
        mate: vec![None; n],
        labels: Vec::new(),
        label_ends: Vec::new(),
        in_blossoms: (0..n).collect(),
        blossom_parents: vec![None; 2 * n],
        blossom_children: vec![Vec::new(); 2 * n],
        blossom_base,
        blossom_endpoints: vec![Vec::new(); 2 * n],
        best_edge: Vec::new(),
        blossom_best_edges: vec![Vec::new(); 2 * n],
        unused_blossoms: (n..2 * n).collect(),
        dual_var,
        allowed_edge: Vec::new(),
        queue: Vec::with_capacity(n),
    };
    for _ in 0..n {
        s.labels = vec![Some(0); 2 * n];
        s.label_ends = vec![None; 2 * n];
        s.best_edge = vec![None; 2 * n];
        for b in n..2 * n {
            s.blossom_best_edges[b].clear();
        }
        s.allowed_edge = vec![false; m];
        s.queue.clear();
        for v in 0..n {
            if s.mate[v].is_none() && s.labels[s.in_blossoms[v]] == Some(0) {
                s.assign_label(v, 1, None);
            }
        }
        let mut augmented = false;
        loop {
            while !augmented {
                let Some(v) = s.queue.pop() else {
                    break;
                };
                debug_assert!(s.labels[s.in_blossoms[v]] == Some(1));
                for p in s.neighbor_endpoints[v].clone() {
                    let k = p / 2;
                    let w = s.endpoints[p];
                    let mut kslack = 0;
                    if s.in_blossoms[v] == s.in_blossoms[w] {
                        continue;
                    }
                    if !s.allowed_edge[k] {
                        kslack = s.slack(k);
                        if kslack <= 0 {
                            s.allowed_edge[k] = true;
                        }
                    }
                    if s.allowed_edge[k] {
                        if s.labels[s.in_blossoms[w]] == Some(0) {
                            s.assign_label(w, 2, Some(p ^ 1));
                        } else if s.labels[s.in_blossoms[w]] == Some(1) {
                            match s.scan_blossom(v, w) {
                                Some(base) => s.add_blossom(base, k),
                                None => {
                                    s.augment_matching(k);
                                    augmented = true;
                                    break;
                                }
                            }
                        } else if s.labels[w] == Some(0) {
                            debug_assert!(s.labels[s.in_blossoms[w]] == Some(2));
                            s.labels[w] = Some(2);
                            s.label_ends[w] = Some(p ^ 1);
                        }
                    } else if s.labels[s.in_blossoms[w]] == Some(1) {
                        let b = s.in_blossoms[v];
                        if s.best_edge[b].is_none_or(|e| kslack < s.slack(e)) {
                            s.best_edge[b] = Some(k);
                        }
                    } else if s.labels[w] == Some(0)
                        && s.best_edge[w].is_none_or(|e| kslack < s.slack(e))
                    {
                        s.best_edge[w] = Some(k);
                    }
                }
            }
            if augmented {
                break;
            }
            // The dual change: type 1 to 4, as in the reference.
            let mut kind = 0;
            let mut delta = 0i128;
            let mut delta_edge = None;
            let mut delta_blossom = None;
            if !max_cardinality {
                kind = 1;
                delta = *s.dual_var[..n].iter().min().expect("a vertex");
            }
            for v in 0..n {
                if s.labels[s.in_blossoms[v]] == Some(0)
                    && let Some(e) = s.best_edge[v]
                {
                    let d = s.slack(e);
                    if kind == 0 || d < delta {
                        delta = d;
                        kind = 2;
                        delta_edge = Some(e);
                    }
                }
            }
            for b in 0..2 * n {
                if s.blossom_parents[b].is_none()
                    && s.labels[b] == Some(1)
                    && let Some(e) = s.best_edge[b]
                {
                    let kslack = s.slack(e);
                    debug_assert!(kslack % 2 == 0);
                    let d = kslack / 2;
                    if kind == 0 || d < delta {
                        delta = d;
                        kind = 3;
                        delta_edge = Some(e);
                    }
                }
            }
            for b in n..2 * n {
                if s.blossom_base[b].is_some()
                    && s.blossom_parents[b].is_none()
                    && s.labels[b] == Some(2)
                    && (kind == 0 || s.dual_var[b] < delta)
                {
                    delta = s.dual_var[b];
                    kind = 4;
                    delta_blossom = Some(b);
                }
            }
            if kind == 0 {
                debug_assert!(max_cardinality);
                kind = 1;
                delta = (*s.dual_var[..n].iter().min().expect("a vertex")).max(0);
            }
            for v in 0..n {
                match s.labels[s.in_blossoms[v]] {
                    Some(1) => s.dual_var[v] -= delta,
                    Some(2) => s.dual_var[v] += delta,
                    _ => {}
                }
            }
            for b in n..2 * n {
                if s.blossom_base[b].is_some() && s.blossom_parents[b].is_none() {
                    match s.labels[b] {
                        Some(1) => s.dual_var[b] += delta,
                        Some(2) => s.dual_var[b] -= delta,
                        _ => {}
                    }
                }
            }
            match kind {
                1 => break,
                2 => {
                    let e = delta_edge.expect("an edge");
                    s.allowed_edge[e] = true;
                    let (mut i, mut j, _) = edges[e];
                    if s.labels[s.in_blossoms[i]] == Some(0) {
                        std::mem::swap(&mut i, &mut j);
                    }
                    debug_assert!(s.labels[s.in_blossoms[i]] == Some(1));
                    s.queue.push(i);
                }
                3 => {
                    let e = delta_edge.expect("an edge");
                    s.allowed_edge[e] = true;
                    let (i, _, _) = edges[e];
                    debug_assert!(s.labels[s.in_blossoms[i]] == Some(1));
                    s.queue.push(i);
                }
                _ => s.expand_blossom(delta_blossom.expect("a blossom"), false),
            }
        }
        if !augmented {
            break;
        }
        for b in n..2 * n {
            if s.blossom_parents[b].is_none()
                && s.blossom_base[b].is_some()
                && s.labels[b] == Some(1)
                && s.dual_var[b] == 0
            {
                s.expand_blossom(b, true);
            }
        }
    }
    s.mate.iter().map(|p| p.map(|p| s.endpoints[p])).collect()
}

#[cfg(test)]
mod tests {
    use super::max_weight_matching;

    /// The pairs of a matching, the smaller vertex first.
    fn pairs(mate: &[Option<usize>]) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = mate
            .iter()
            .enumerate()
            .filter_map(|(a, b)| b.filter(|b| a < *b).map(|b| (a, b)))
            .collect();
        out.sort_unstable();
        out
    }

    /// NetworkX's blossom cases (`algorithms/tests/test_matching.py`,
    /// NetworkX 3.7, BSD-3-Clause, licence text in `matching.rs`): each
    /// graph with its maximum-weight matching; they reach S- and
    /// T-blossoms, nested blossoms, relabelling and expansion during a
    /// stage, and recursive expansion.
    #[test]
    fn networkx_blossom_cases() {
        type Case<'a> = (&'a [(usize, usize, i128)], &'a [(usize, usize)]);
        let cases: &[Case<'_>] = &[
            (
                &[(1, 2, 8), (1, 3, 9), (2, 3, 10), (3, 4, 7)],
                &[(1, 2), (3, 4)],
            ),
            (
                &[
                    (1, 2, 8),
                    (1, 3, 9),
                    (2, 3, 10),
                    (3, 4, 7),
                    (1, 6, 5),
                    (4, 5, 6),
                ],
                &[(1, 6), (2, 3), (4, 5)],
            ),
            (
                &[
                    (1, 2, 9),
                    (1, 3, 8),
                    (2, 3, 10),
                    (1, 4, 5),
                    (4, 5, 4),
                    (1, 6, 3),
                ],
                &[(1, 6), (2, 3), (4, 5)],
            ),
            (
                &[
                    (1, 2, 9),
                    (1, 3, 8),
                    (2, 3, 10),
                    (1, 4, 5),
                    (4, 5, 3),
                    (3, 6, 4),
                ],
                &[(1, 2), (3, 6), (4, 5)],
            ),
            (
                &[
                    (1, 2, 9),
                    (1, 3, 9),
                    (2, 3, 10),
                    (2, 4, 8),
                    (3, 5, 8),
                    (4, 5, 10),
                    (5, 6, 6),
                ],
                &[(1, 3), (2, 4), (5, 6)],
            ),
            (
                &[
                    (1, 2, 10),
                    (1, 7, 10),
                    (2, 3, 12),
                    (3, 4, 20),
                    (3, 5, 20),
                    (4, 5, 25),
                    (5, 6, 10),
                    (6, 7, 10),
                    (7, 8, 8),
                ],
                &[(1, 2), (3, 4), (5, 6), (7, 8)],
            ),
            (
                &[
                    (1, 2, 8),
                    (1, 3, 8),
                    (2, 3, 10),
                    (2, 4, 12),
                    (3, 5, 12),
                    (4, 5, 14),
                    (4, 6, 12),
                    (5, 7, 12),
                    (6, 7, 14),
                    (7, 8, 12),
                ],
                &[(1, 2), (3, 5), (4, 6), (7, 8)],
            ),
            (
                &[
                    (1, 2, 23),
                    (1, 5, 22),
                    (1, 6, 15),
                    (2, 3, 25),
                    (3, 4, 22),
                    (4, 5, 25),
                    (4, 8, 14),
                    (5, 7, 13),
                ],
                &[(1, 6), (2, 3), (4, 8), (5, 7)],
            ),
            (
                &[
                    (1, 2, 19),
                    (1, 3, 20),
                    (1, 8, 8),
                    (2, 3, 25),
                    (2, 4, 18),
                    (3, 5, 18),
                    (4, 5, 13),
                    (4, 7, 7),
                    (5, 6, 7),
                ],
                &[(1, 8), (2, 3), (4, 7), (5, 6)],
            ),
            (
                &[
                    (1, 2, 45),
                    (1, 5, 45),
                    (2, 3, 50),
                    (3, 4, 45),
                    (4, 5, 50),
                    (1, 6, 30),
                    (3, 9, 35),
                    (4, 8, 35),
                    (5, 7, 26),
                    (9, 10, 5),
                ],
                &[(1, 6), (2, 3), (4, 8), (5, 7), (9, 10)],
            ),
            (
                &[
                    (1, 2, 45),
                    (1, 5, 45),
                    (2, 3, 50),
                    (3, 4, 45),
                    (4, 5, 50),
                    (1, 6, 30),
                    (3, 9, 35),
                    (4, 8, 26),
                    (5, 7, 40),
                    (9, 10, 5),
                ],
                &[(1, 6), (2, 3), (4, 8), (5, 7), (9, 10)],
            ),
            (
                &[
                    (1, 2, 45),
                    (1, 5, 45),
                    (2, 3, 50),
                    (3, 4, 45),
                    (4, 5, 50),
                    (1, 6, 30),
                    (3, 9, 35),
                    (4, 8, 28),
                    (5, 7, 26),
                    (9, 10, 5),
                ],
                &[(1, 6), (2, 3), (4, 8), (5, 7), (9, 10)],
            ),
            (
                &[
                    (1, 2, 45),
                    (1, 7, 45),
                    (2, 3, 50),
                    (3, 4, 45),
                    (4, 5, 95),
                    (4, 6, 94),
                    (5, 6, 94),
                    (6, 7, 50),
                    (1, 8, 30),
                    (3, 11, 35),
                    (5, 9, 36),
                    (7, 10, 26),
                    (11, 12, 5),
                ],
                &[(1, 8), (2, 3), (4, 6), (5, 9), (7, 10), (11, 12)],
            ),
            (
                &[
                    (1, 2, 40),
                    (1, 3, 40),
                    (2, 3, 60),
                    (2, 4, 55),
                    (3, 5, 55),
                    (4, 5, 50),
                    (1, 8, 15),
                    (5, 7, 30),
                    (7, 6, 10),
                    (8, 10, 10),
                    (4, 9, 30),
                ],
                &[(1, 2), (3, 5), (4, 9), (6, 7), (8, 10)],
            ),
        ];
        for (edges, answer) in cases {
            let n = edges.iter().map(|e| e.0.max(e.1)).max().unwrap() + 1;
            let got = pairs(&max_weight_matching(n, edges, false));
            assert_eq!(got, answer.to_vec(), "{edges:?}");
        }
        // NetworkX's maximum-cardinality case (gh-8062): with weights
        // 1000, 2 and 3000 on a path, the maximum-weight matching takes
        // one edge, the maximum-cardinality one two.
        let path = [(1, 2, 1000), (2, 3, 2), (3, 4, 3000)];
        assert_eq!(
            pairs(&max_weight_matching(5, &path, false)),
            vec![(1, 2), (3, 4)]
        );
        let path = [(1, 2, 1), (2, 3, 10), (3, 4, 1)];
        assert_eq!(pairs(&max_weight_matching(5, &path, false)), vec![(2, 3)]);
        assert_eq!(
            pairs(&max_weight_matching(5, &path, true)),
            vec![(1, 2), (3, 4)]
        );
        assert!(pairs(&max_weight_matching(3, &[], true)).is_empty());
    }
}
