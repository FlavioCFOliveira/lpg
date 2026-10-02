#![forbid(unsafe_code)]
//! Graph storage layout microbenchmark for the lpg technical specification.
//!
//! Usage: layout-bench <zipf|uniform> <online|bulk> <nodes> <edges> <workdir> [scale]
//! `scale` (default 1.0) multiplies the operation counts.

mod adj;
mod blocks;
mod btree;
mod csr;
mod generator;
mod large;
mod pcsr;
mod pool;
mod records;
mod util;

use adj::{Adj, AdjA, AdjB, CompTree, IN, LIVE, OUT};
use blocks::Blocks;
use btree::FixedTree;
use csr::Csr;
use generator::{Graph, Sampler};
use pcsr::PCsr;
use pool::{PAGE, Pool, Stats};
use records::Recs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use util::{Rng, mix};

enum Lay {
    A(AdjA),
    B(AdjB),
    C(Blocks),
    D(Recs),
    E(Csr),
    F(PCsr),
}

struct Arm {
    name: &'static str,
    pool: Pool,
    lay: Lay,
}

impl Arm {
    fn insert(&mut self, s: u64, d: u64, t: u32, e: u64, c: u64) {
        let p = &mut self.pool;
        match &mut self.lay {
            Lay::A(x) => x.insert(p, s, d, t, e, c),
            Lay::B(x) => x.insert(p, s, d, t, e, c),
            Lay::C(x) => x.insert(p, s, d, t, e, c),
            Lay::D(x) => x.insert(p, s, d, t, e, c),
            Lay::E(_) | Lay::F(_) => {}
        }
    }
    fn delete(&mut self, e: u64, ts: u64) -> bool {
        let p = &mut self.pool;
        match &mut self.lay {
            Lay::A(x) => x.delete(p, e, ts),
            Lay::B(x) => x.delete(p, e, ts),
            Lay::C(x) => x.delete(p, e, ts),
            Lay::D(x) => x.delete(p, e, ts),
            Lay::E(_) | Lay::F(_) => true,
        }
    }
    /// Internal node handle for a logical node id (D: slot via the id map).
    #[inline(always)]
    fn handle(&mut self, id: u64) -> u64 {
        match &mut self.lay {
            Lay::D(x) => x.slot_of(&mut self.pool, id) as u64,
            _ => id,
        }
    }
    /// Expands a handle; f(edge id, neighbour handle).
    #[inline(always)]
    fn expand_h(&mut self, h: u64, dir: u8, ty: Option<u32>, snap: u64, mut f: impl FnMut(u64, u64)) {
        let p = &mut self.pool;
        match &mut self.lay {
            Lay::A(x) => x.expand(p, h, dir, ty, snap, f),
            Lay::B(x) => x.expand(p, h, dir, ty, snap, f),
            Lay::C(x) => x.expand(p, h, dir, ty, snap, f),
            Lay::D(x) => x.expand_slot(p, h as u32, dir == OUT, ty, snap, |e, s| f(e, s as u64)),
            Lay::E(x) => x.expand(h, dir, ty, f),
            Lay::F(x) => x.expand(p, h, dir, ty, f),
        }
    }
    /// 1-hop from a logical id returning logical neighbour ids.
    #[inline(always)]
    fn hop1(&mut self, id: u64, ty: Option<u32>, snap: u64, tmp: &mut Vec<(u64, u32)>) -> (u64, u64) {
        let (mut n, mut chk) = (0u64, 0u64);
        if let Lay::D(x) = &mut self.lay {
            let p = &mut self.pool;
            let s = x.slot_of(p, id);
            tmp.clear();
            x.expand_slot(p, s, true, ty, snap, |e, v| tmp.push((e, v)));
            for &(e, v) in tmp.iter() {
                let lid = x.lid(p, v);
                n += 1;
                chk = chk.wrapping_add(mix(e ^ lid << 32));
            }
            return (n, chk);
        }
        self.expand_h(id, OUT, ty, snap, |e, v| {
            n += 1;
            chk = chk.wrapping_add(mix(e ^ v << 32));
        });
        (n, chk)
    }
}

struct Scratch {
    seen: Vec<u32>,
    epoch: u32,
    fr: Vec<u64>,
    nx: Vec<u64>,
    tmp: Vec<(u64, u32)>,
}

/// Depth-limited distinct expansion over OUT edges; returns (reached nodes, edges scanned).
fn khop(arm: &mut Arm, id: u64, k: u32, snap: u64, sc: &mut Scratch) -> (u64, u64) {
    sc.epoch += 1;
    let ep = sc.epoch;
    let h = arm.handle(id);
    sc.fr.clear();
    sc.fr.push(h);
    sc.seen[h as usize] = ep;
    let (mut reached, mut edges) = (1u64, 0u64);
    let mut depth = 0;
    while depth < k && !sc.fr.is_empty() {
        sc.nx.clear();
        for i in 0..sc.fr.len() {
            let u = sc.fr[i];
            let (seen, nx) = (&mut sc.seen, &mut sc.nx);
            arm.expand_h(u, OUT, None, snap, |_, v| {
                edges += 1;
                if seen[v as usize] != ep {
                    seen[v as usize] = ep;
                    nx.push(v);
                }
            });
        }
        reached += sc.nx.len() as u64;
        std::mem::swap(&mut sc.fr, &mut sc.nx);
        depth += 1;
    }
    (reached, edges)
}

#[derive(Clone, Copy)]
enum Kind {
    Hop1(Option<u32>),
    KHop(u32),
}

#[derive(Clone)]
struct OpSet {
    name: String,
    kind: Kind,
    starts: Vec<u64>,
    /// Layout D is not run on this set (its per-operation cost exceeds the time budget).
    skip_d: bool,
}

#[derive(Clone, Default)]
struct Meas {
    ns: Vec<u64>,
    total_ns: u64,
    edges: u64,
    res: Vec<(u64, u64)>,
    st: Stats,
    io: u64,
    done: usize,
}

fn run_set(arm: &mut Arm, set: &OpSet, snap: u64, budget: Option<Duration>, sc: &mut Scratch) -> Meas {
    let mut m = Meas::default();
    let st0 = arm.pool.st;
    let io0 = util::io_read_bytes();
    // The host clock source is HPET (TSC marked unstable), so an Instant::now()
    // pair costs ~2.8 us. 1-hop and 2-hop are therefore timed as a batch; only
    // 3-hop and BFS operations (milliseconds) get per-operation timings.
    let t_all = Instant::now();
    match set.kind {
        Kind::Hop1(_) | Kind::KHop(2) => {
            for (i, &s) in set.starts.iter().enumerate() {
                arm.pool.begin_op();
                let r = match set.kind {
                    Kind::Hop1(ty) => arm.hop1(s, ty, snap, &mut sc.tmp),
                    Kind::KHop(k) => khop(arm, s, k, snap, sc),
                };
                m.edges += if let Kind::Hop1(_) = set.kind { r.0 } else { r.1 };
                m.res.push(r);
                m.done += 1;
                if i % 16 == 15 {
                    if let Some(b) = budget {
                        if t_all.elapsed() > b {
                            break;
                        }
                    }
                }
            }
            m.total_ns = t_all.elapsed().as_nanos() as u64;
        }
        Kind::KHop(k) => {
            for &s in &set.starts {
                arm.pool.begin_op();
                let t = Instant::now();
                let r = khop(arm, s, k, snap, sc);
                let ns = t.elapsed().as_nanos() as u64;
                m.ns.push(ns);
                m.total_ns += ns;
                m.edges += r.1;
                m.res.push(r);
                m.done += 1;
                if let Some(b) = budget {
                    if t_all.elapsed() > b {
                        break;
                    }
                }
            }
        }
    }
    m.st = arm.pool.st.since(&st0);
    m.io = util::io_read_bytes() - io0;
    m
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    if v.is_empty() {
        return f64::NAN;
    }
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}
fn fmt_pct(ms: &[Meas], p: f64) -> String {
    if ms.iter().any(|m| m.ns.is_empty()) {
        return "n/a".into();
    }
    let mut v: Vec<f64> = ms.iter().map(|m| pct(&m.ns, p) as f64).collect();
    format!("{:.0}", median(&mut v))
}
fn pct(v: &[u64], p: f64) -> u64 {
    let mut s = v.to_vec();
    s.sort_unstable();
    if s.is_empty() {
        return 0;
    }
    s[((p * (s.len() - 1) as f64).round() as usize).min(s.len() - 1)]
}

/// Runs `sets` for `arms` in interleaved rounds (first round = warm-up, discarded
/// unless `rounds` == 1). Prints one table per set.
fn bench(title: &str, arms: &mut [Arm], order_extra_dup: bool, sets_per_round: &dyn Fn(usize) -> Vec<OpSet>, rounds: usize, warmup: bool, budget: Option<Duration>, snap: u64, sc: &mut Scratch, expect: &dyn Fn(&OpSet, usize) -> Option<u64>) {
    println!("\n### {title}\n");
    let na = arms.len();
    let slots = na + order_extra_dup as usize; // extra slot = A repeated
    let names: Vec<String> = (0..slots).map(|i| if i < na { arms[i].name.to_string() } else { format!("{}(rep)", arms[0].name) }).collect();
    let nsets = sets_per_round(0).len();
    let mut acc: Vec<Vec<Vec<Meas>>> = vec![vec![Vec::new(); slots]; nsets];
    let total_rounds = rounds + warmup as usize;
    for r in 0..total_rounds {
        let sets = sets_per_round(r);
        for (si, set) in sets.iter().enumerate() {
            let mut order: Vec<usize> = (0..na).collect();
            order.rotate_left(r % na);
            if order_extra_dup {
                // Put the repeat of A at the opposite position of A.
                let pa = order.iter().position(|&x| x == 0).unwrap_or(0);
                order.insert((pa + na / 2 + 1).min(order.len()), na);
            }
            let mut round_res: Vec<(usize, Meas)> = Vec::new();
            for &slot in &order {
                let ai = if slot == na { 0 } else { slot };
                if set.skip_d && matches!(arms[ai].lay, Lay::D(_)) {
                    continue;
                }
                let m = run_set(&mut arms[ai], set, snap, budget, sc);
                round_res.push((slot, m));
            }
            // Correctness: identical per-op results across arms (common prefix).
            let (s0, m0) = &round_res[0];
            for (s, m) in &round_res[1..] {
                let k = m.done.min(m0.done);
                if m.res[..k] != m0.res[..k] {
                    let i = (0..k).find(|&i| m.res[i] != m0.res[i]).unwrap_or(0);
                    panic!("result mismatch in {} op {} ({}): {} {:?} vs {} {:?}", set.name, i, set.starts[i], names[*s0], m0.res[i], names[*s], m.res[i]);
                }
            }
            for (i, r0) in m0.res.iter().enumerate() {
                if let Some(e) = expect(set, i) {
                    assert_eq!(r0.0, e, "expected edge count mismatch in {} op {}", set.name, i);
                }
            }
            if r > 0 || !warmup {
                for (slot, m) in round_res {
                    acc[si][slot].push(m);
                }
            }
        }
    }
    let sets = sets_per_round(0);
    for (si, set) in sets.iter().enumerate() {
        println!("\n{} ({} starts/round, {} measured rounds{})\n", set.name, set.starts.len(), rounds, if budget.is_some() { ", time-budgeted" } else { "" });
        println!("| layout | ns/op median [min, max] | ns/edge | p50 ns | p99 ns | edges/op | page fetches/op | distinct pages/op | misses/op | seq miss % | us/miss | device KiB/op | ops done |");
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for slot in 0..slots {
            let ms = &acc[si][slot];
            if ms.is_empty() {
                println!("| {} | not run | - | - | - | - | - | - | - | - | - | - | 0 |", names[slot]);
                continue;
            }
            let mut per: Vec<f64> = ms.iter().map(|m| m.total_ns as f64 / m.done.max(1) as f64).collect();
            let mn = per.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = per.iter().cloned().fold(0.0, f64::max);
            let med = median(&mut per);
            let mut pe: Vec<f64> = ms.iter().map(|m| m.total_ns as f64 / m.edges.max(1) as f64).collect();
            let ops: u64 = ms.iter().map(|m| m.done as u64).sum();
            let edges: u64 = ms.iter().map(|m| m.edges).sum();
            let st = ms.iter().fold(Stats::default(), |a, m| Stats {
                fetches: a.fetches + m.st.fetches,
                distinct: a.distinct + m.st.distinct,
                misses: a.misses + m.st.misses,
                dirtied: a.dirtied + m.st.dirtied,
                writebacks: a.writebacks + m.st.writebacks,
                read_ns: a.read_ns + m.st.read_ns,
                seqmiss: a.seqmiss + m.st.seqmiss,
            });
            let io: u64 = ms.iter().map(|m| m.io).sum();
            let o = ops.max(1) as f64;
            let is_e = matches!(arms[if slot == na { 0 } else { slot }].lay, Lay::E(_));
            let f = |x: u64| if is_e { "-".to_string() } else { format!("{:.2}", x as f64 / o) };
            let usmiss = if st.misses > 0 { format!("{:.1}", st.read_ns as f64 / st.misses as f64 / 1000.0) } else { "-".into() };
            let iochk = if st.misses > 0 { format!(" (io/miss {:.2})", io as f64 / (st.misses as f64 * PAGE as f64)) } else { String::new() };
            println!(
                "| {} | {:.0} [{:.0}, {:.0}] | {:.1} | {} | {} | {:.1} | {} | {} | {} | {} | {}{} | {} | {} |",
                names[slot],
                med,
                mn,
                mx,
                median(&mut pe),
                fmt_pct(ms, 0.5),
                fmt_pct(ms, 0.99),
                edges as f64 / o,
                f(st.fetches),
                f(st.distinct),
                f(st.misses),
                if st.misses > 0 { format!("{:.1}", st.seqmiss as f64 * 100.0 / st.misses as f64) } else { "-".into() },
                usmiss,
                iochk,
                if is_e { "-".to_string() } else { format!("{:.1}", io as f64 / o / 1024.0) },
                ops
            );
        }
    }
}

fn sizes(arms: &mut [Arm], m: u64, nodes: u64) {
    println!("\n### Space\n");
    println!("| layout | adjacency pages | adjacency B/edge | other pages | total B/edge | detail |");
    println!("|---|---|---|---|---|---|");
    let be = |p: u64| p as f64 * PAGE as f64 / m as f64;
    for a in arms.iter_mut() {
        let np = a.pool.npages() as u64;
        match &mut a.lay {
            Lay::A(x) => {
                let adj = x.t.pages;
                println!("| {} | {} | {:.1} | {} | {:.1} | leaves {}, height {}, entries {}, leaf fill {:.2}; edge table {} pages |", a.name, adj, be(adj), np - adj, be(np), x.t.leaves, x.t.height, x.t.entries, x.t.entries as f64 * adj::AE as f64 / (x.t.leaves as f64 * (PAGE - btree::HDR) as f64), x.et.pages);
            }
            Lay::B(x) => {
                let adj = x.t.pages;
                let used = x.t.used_bytes(&mut a.pool);
                println!("| {} | {} | {:.1} | {} | {:.1} | leaves {}, height {}, entries {}, leaf bytes used {:.1} B/edge ({:.1} B/entry), leaf fill {:.2}; edge table {} pages |", a.name, adj, be(adj), np - adj, be(np), x.t.leaves, x.t.height, x.t.entries, used as f64 / m as f64, used as f64 / x.t.entries.max(1) as f64, used as f64 / (x.t.leaves as f64 * PAGE as f64), x.et.pages);
            }
            Lay::C(x) => {
                let adj = x.pages_dir + x.pages_blk + x.pages_chain;
                let (used, reserved) = x.payload(&mut a.pool, nodes);
                println!("| {} | {} | {:.1} | {} | {:.1} | dir {}, block slabs {}, chain {}; entry bytes {:.1} B/edge, block bytes reserved {:.1} B/edge; relocations {}, entries copied {}; edge table {} pages |", a.name, adj, be(adj), np - adj, be(np), x.pages_dir, x.pages_blk, x.pages_chain, used as f64 / m as f64, reserved as f64 / m as f64, x.relocations, x.copied_entries, x.et.pages);
            }
            Lay::D(x) => {
                let (nn, rr, gg) = x.pages();
                let adj = nn + rr + gg;
                println!("| {} | {} | {:.1} | {} | {:.1} | node recs {}, rel recs {}, groups {}; node map {} pages, edge map {} pages; densified {} |", a.name, adj, be(adj), np - adj, be(np), nn, rr, gg, x.nmap.pages, x.emap.pages, x.densified);
            }
            Lay::E(x) => {
                println!("| {} | - | {:.1} | - | {:.1} | in-memory arrays, {} bytes |", a.name, x.bytes() as f64 / m as f64, x.bytes() as f64 / m as f64, x.bytes());
            }
            Lay::F(x) => {
                println!("| {} | {} | {:.1} | 0 | {:.1} | offset pages {}, entry pages {}, entry stream {:.1} B/edge; read-only snapshot, no stamps |", a.name, x.pages(), be(x.pages()), be(np), x.off_pages, x.ent_pages, x.ent_bytes as f64 / m as f64);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("large") {
        large::run(&args);
        return;
    }
    if args.len() < 6 {
        eprintln!("usage: layout-bench <zipf|uniform> <online|bulk> <nodes> <edges> <workdir> [scale]");
        std::process::exit(2);
    }
    let alpha = if args[1] == "zipf" { 0.8 } else { 0.0 };
    let online = args[2] == "online";
    let n: u64 = args[3].parse().expect("nodes");
    let m: u64 = args[4].parse().expect("edges");
    let dir = PathBuf::from(&args[5]);
    let scale: f64 = args.get(6).map(|s| s.parse().expect("scale")).unwrap_or(1.0);
    let sc_n = |x: usize| ((x as f64 * scale) as usize).max(1);
    std::fs::create_dir_all(&dir).expect("workdir");

    println!("# layout-bench {} {} n={} m={} alpha={} scale={}", args[1], args[2], n, m, alpha, scale);
    {
        let t = Instant::now();
        let mut x = 0u64;
        for _ in 0..100_000 {
            x = x.wrapping_add(Instant::now().elapsed().as_nanos() as u64);
        }
        println!("clock: Instant::now() pair costs {:.0} ns (checksum {})", t.elapsed().as_nanos() as f64 / 100_000.0, x % 7);
    }
    let t = Instant::now();
    let mut smp = Sampler::new(n, alpha, 0x5EED_0001 ^ (alpha.to_bits()));
    let g: Graph = generator::generate(&mut smp, m);
    let mut outdeg = vec![0u32; n as usize];
    let mut indeg = vec![0u32; n as usize];
    for i in 0..m as usize {
        outdeg[g.src[i] as usize] += 1;
        indeg[g.dst[i] as usize] += 1;
    }
    let maxo = outdeg.iter().max().copied().unwrap_or(0);
    let maxi = indeg.iter().max().copied().unwrap_or(0);
    let zero = outdeg.iter().filter(|&&d| d == 0).count();
    let over50 = (0..n as usize).filter(|&i| outdeg[i] + indeg[i] > 50).count();
    let mut sorted_deg: Vec<u32> = outdeg.clone();
    sorted_deg.sort_unstable();
    println!("graph generated in {:.1}s: max out-degree {}, max in-degree {}, nodes with out-degree 0: {}, nodes with total degree > 50: {}, out-degree p50 {} p90 {} p99 {} p99.9 {}", t.elapsed().as_secs_f64(), maxo, maxi, zero, over50, sorted_deg[n as usize / 2], sorted_deg[n as usize * 9 / 10], sorted_deg[n as usize * 99 / 100], sorted_deg[n as usize * 999 / 1000]);

    // ---------------------------------------------------------------- build
    let mk = |name: &str| Pool::create(&dir.join(format!("{name}.pages")));
    let mut arms: Vec<Arm> = Vec::new();
    println!("\n### Build ({})\n", if online { "online inserts in random order, created_ts = edge id + 1" } else { "sorted bulk load, created_ts = 0 (frozen)" });
    println!("| layout | seconds | edges/s | page fetches/edge | pages dirtied/edge | extra | RSS after (MiB) |");
    println!("|---|---|---|---|---|---|---|");
    let report = |name: &str, secs: f64, st: Stats, extra: String| {
        println!("| {} | {:.2} | {:.0} | {:.2} | {:.2} | {} | {} |", name, secs, m as f64 / secs, st.fetches as f64 / m as f64, st.dirtied as f64 / m as f64, extra, util::rss_kib() / 1024);
    };
    // Sorted order for bulk: (src, type, dst, eid).
    let order: Vec<u32> = if online {
        Vec::new()
    } else {
        let mut o: Vec<u32> = (0..m as u32).collect();
        o.sort_unstable_by_key(|&i| (g.src[i as usize], g.ty[i as usize], g.dst[i as usize], i));
        o
    };
    for name in ["A", "B", "C", "D"] {
        let mut pool = mk(name);
        let t = Instant::now();
        let lay = if online {
            let mut lay = match name {
                "A" => Lay::A(AdjA::new(&mut pool)),
                "B" => Lay::B(AdjB::new(&mut pool)),
                "C" => Lay::C(Blocks::new(&mut pool, n)),
                _ => Lay::D(Recs::new(&mut pool, n)),
            };
            let st0 = pool.st;
            let t = Instant::now();
            let mut arm = Arm { name: "", pool, lay };
            for i in 0..m as usize {
                arm.pool.begin_op();
                arm.insert(g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32, i as u64, i as u64 + 1);
            }
            let secs = t.elapsed().as_secs_f64();
            let st = arm.pool.st.since(&st0);
            let extra = match &arm.lay {
                Lay::D(x) => format!("existing records rewritten/edge {:.2}, densified nodes {}", x.rewrites as f64 / m as f64, x.densified),
                Lay::C(x) => format!("relocations/edge {:.3}, entries copied/edge {:.2}", x.relocations as f64 / m as f64, x.copied_entries as f64 / m as f64),
                _ => String::new(),
            };
            report(name, secs, st, extra);
            pool = arm.pool;
            lay = arm.lay;
            lay
        } else {
            let st0 = pool.st;
            let lay = match name {
                "A" | "B" => {
                    let mut ents: Vec<Adj> = Vec::with_capacity(2 * m as usize);
                    for i in 0..m as usize {
                        let (s, d, ty) = (g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32);
                        ents.push(Adj { node: s, dir: OUT, ty, nbr: d, eid: i as u64, cts: 0, dts: LIVE });
                        ents.push(Adj { node: d, dir: IN, ty, nbr: s, eid: i as u64, cts: 0, dts: LIVE });
                    }
                    ents.sort_unstable();
                    let mut et_it = (0..m as usize).map(|i| adj::enc_edge(i as u64, g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32).to_vec());
                    if name == "A" {
                        let mut it = ents.iter().map(|a| adj::enc_a(a).to_vec());
                        let t = FixedTree::bulk(&mut pool, adj::AE, adj::KLEN, &mut it, 0.9);
                        let et = FixedTree::bulk(&mut pool, adj::EE, 8, &mut et_it, 1.0);
                        Lay::A(AdjA { t, et })
                    } else {
                        let mut it = ents.iter().copied();
                        let t = CompTree::bulk(&mut pool, &mut it, 0.9);
                        let et = FixedTree::bulk(&mut pool, adj::EE, 8, &mut et_it, 1.0);
                        Lay::B(AdjB { t, et })
                    }
                }
                "C" => {
                    let mut out: Vec<(u32, Vec<[u8; blocks::CE]>)> = Vec::new();
                    let mut inn: Vec<Vec<[u8; blocks::CE]>> = vec![Vec::new(); n as usize];
                    for &i in &order {
                        let i = i as usize;
                        let (s, d, ty) = (g.src[i], g.dst[i], g.ty[i] as u32);
                        if out.last().is_none_or(|x| x.0 != s) {
                            out.push((s, Vec::new()));
                        }
                        if let Some(l) = out.last_mut() {
                            l.1.push(Blocks::entry(ty, d as u64, i as u64, 0));
                        }
                        inn[d as usize].push(Blocks::entry(ty, s as u64, i as u64, 0));
                    }
                    let inn: Vec<(u32, Vec<[u8; blocks::CE]>)> = inn.into_iter().enumerate().filter(|x| !x.1.is_empty()).map(|(k, v)| (k as u32, v)).collect();
                    let mut b = Blocks::bulk(&mut pool, n, &out, &inn);
                    let mut et_it = (0..m as usize).map(|i| adj::enc_edge(i as u64, g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32).to_vec());
                    b.et = FixedTree::bulk(&mut pool, adj::EE, 8, &mut et_it, 1.0);
                    Lay::C(b)
                }
                _ => {
                    let mut r = Recs::new(&mut pool, n);
                    let mut pairs: Vec<(u64, u32)> = Vec::with_capacity(m as usize);
                    for &i in &order {
                        let i = i as usize;
                        let slot = r.insert_raw(&mut pool, g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32, i as u64, 0);
                        pairs.push((i as u64, slot));
                    }
                    pairs.sort_unstable();
                    r.finish_emap(&mut pool, &pairs);
                    Lay::D(r)
                }
            };
            let secs = t.elapsed().as_secs_f64();
            let st = pool.st.since(&st0);
            let extra = match &lay {
                Lay::D(x) => format!("existing records rewritten/edge {:.2}, densified nodes {}", x.rewrites as f64 / m as f64, x.densified),
                _ => "includes sort".to_string(),
            };
            report(name, secs, st, extra);
            lay
        };
        let _ = t;
        let aname: &'static str = match name {
            "A" => "A btree-fixed",
            "B" => "B btree-compressed",
            "C" => "C node-blocks",
            _ => "D record-chains",
        };
        arms.push(Arm { name: aname, pool, lay });
    }

    // ---------------------------------------------------------------- projection E
    let snap = if online { m } else { 0 };
    println!("\n### CSR projection (E) built by one sequential pass, all pages resident\n");
    println!("| source | seconds | edges/s | page fetches | digest |");
    println!("|---|---|---|---|---|");
    let mut csr_keep: Option<Csr> = None;
    let mut digests = Vec::new();
    for ai in 0..3 {
        let arm = &mut arms[ai];
        let st0 = arm.pool.st;
        let t = Instant::now();
        let mut c = Csr::builder(n as usize);
        match &mut arm.lay {
            Lay::A(x) => x.scan_all(&mut arm.pool, snap, |v, d, ty, nb, e| c.push(v, d, ty, nb, e)),
            Lay::B(x) => x.scan_all(&mut arm.pool, snap, |v, d, ty, nb, e| c.push(v, d, ty, nb, e)),
            Lay::C(x) => {
                for v in 0..n {
                    for d in 0..2u8 {
                        x.scan_node(&mut arm.pool, v, d, snap, |ty, nb, e| c.push(v, d, ty, nb, e));
                    }
                }
            }
            _ => {}
        }
        c.finish();
        let secs = t.elapsed().as_secs_f64();
        let dg = c.digest();
        digests.push(dg);
        println!("| {} | {:.2} | {:.0} | {} | {:016x} |", arm.name, secs, m as f64 / secs, arm.pool.st.since(&st0).fetches, dg);
        assert_eq!(c.edges(), m, "projection edge count");
        if csr_keep.is_none() {
            csr_keep = Some(c);
        }
    }
    assert!(digests.iter().all(|&d| d == digests[0]), "projection digests differ");
    let csr = csr_keep.unwrap_or_else(|| Csr::builder(n as usize));
    println!("\n### Paged CSR (F fixed, G compressed) written from the projection\n");
    println!("| layout | seconds | pages |");
    println!("|---|---|---|");
    for (nm, comp, aname) in [("F", false, "F csr-paged"), ("G", true, "G csr-paged-compressed")] {
        let mut pool = mk(nm);
        let t = Instant::now();
        let x = PCsr::build(&mut pool, &csr, comp);
        println!("| {} | {:.2} | {} |", aname, t.elapsed().as_secs_f64(), x.pages());
        arms.push(Arm { name: aname, pool, lay: Lay::F(x) });
    }
    arms.push(Arm { name: "E csr-memory", pool: mk("E"), lay: Lay::E(csr) });
    for a in arms.iter_mut() {
        a.pool.flush_all();
    }
    sizes(&mut arms, m, n);
    println!("\nRSS after build: {} MiB, peak {} MiB", util::rss_kib() / 1024, util::hwm_kib() / 1024);

    // ---------------------------------------------------------------- warm reads
    let mut sc = Scratch { seen: vec![0; n as usize], epoch: 0, fr: Vec::new(), nx: Vec::new(), tmp: Vec::new() };
    let src = g.src.clone();
    let mk_sets = |seed: u64, big: bool| -> Vec<OpSet> {
        let mut r = Rng(seed);
        let uni = |k: usize, r: &mut Rng| (0..k).map(|_| r.below(n)).collect::<Vec<u64>>();
        let dw = |k: usize, r: &mut Rng| (0..k).map(|_| src[r.below(m) as usize] as u64).collect::<Vec<u64>>();
        let (h1, h2, h2d, h3, bfs) = if big { (sc_n(20000), sc_n(1000), sc_n(200), sc_n(50), 2) } else { (sc_n(2000), sc_n(100), sc_n(30), sc_n(10), 1) };
        let u1 = uni(h1, &mut r);
        vec![
            OpSet { name: "1-hop OUT, all types, uniform start".into(), kind: Kind::Hop1(None), starts: u1, skip_d: false },
            // Own sample: reusing the previous set's starts would find its pages cached.
            OpSet { name: "1-hop OUT, type = 2, uniform start".into(), kind: Kind::Hop1(Some(2)), starts: uni(h1, &mut r), skip_d: false },
            OpSet { name: "1-hop OUT, all types, degree-weighted start".into(), kind: Kind::Hop1(None), starts: dw(h1, &mut r), skip_d: false },
            OpSet { name: "2-hop OUT (distinct), uniform start".into(), kind: Kind::KHop(2), starts: uni(h2, &mut r), skip_d: false },
            OpSet { name: "2-hop OUT (distinct), degree-weighted start".into(), kind: Kind::KHop(2), starts: dw(h2d, &mut r), skip_d: false },
            OpSet { name: "3-hop OUT (distinct), uniform start".into(), kind: Kind::KHop(3), starts: uni(h3, &mut r), skip_d: false },
            OpSet { name: "BFS OUT (unbounded depth), degree-weighted start".into(), kind: Kind::KHop(u32::MAX), starts: dw(bfs, &mut r), skip_d: false },
        ]
    };
    let od = outdeg.clone();
    let expect = move |set: &OpSet, i: usize| -> Option<u64> {
        match set.kind {
            Kind::Hop1(None) => Some(od[set.starts[i] as usize] as u64),
            _ => None,
        }
    };
    let warm_sets = mk_sets(0xA11CE, true);
    let ws = warm_sets.clone();
    // LB_RAM: misses at every cache ratio are served from RAM (OS page cache, file
    // pre-read in full); the all-resident phase is skipped (identical to the disk run).
    let ram = std::env::var_os("LB_RAM").is_some();
    if !ram {
        bench("Warm: all pages resident", &mut arms, true, &|_| ws.clone(), 3, true, Some(Duration::from_secs(4)), snap, &mut sc, &expect);
    }
    if std::env::var_os("LB_WARM_ONLY").is_some() {
        return;
    }

    // ---------------------------------------------------------------- constrained reads
    // A-D: MVCC layouts (modifiable); F, G: paged read-only CSR. The frame budget
    // is a fraction of the smallest MVCC layout, the same for every arm.
    let mvcc = 4;
    let paged = 6;
    let min_pages = arms[..mvcc].iter().map(|a| a.pool.npages()).min().unwrap_or(1) as usize;
    // 10 % is the primary case; OS-cache (buffered) misses are measured only there.
    let plan: [(f64, &[bool]); 3] = if ram { [(0.25, &[false]), (0.10, &[false]), (0.01, &[false])] } else { [(0.25, &[true]), (0.10, &[true, false]), (0.01, &[true])] };
    for (frac, modes) in plan {
        let cap = ((min_pages as f64) * frac) as usize;
        println!("\n## Constrained cache: {} frames = {:.1} MiB = {:.0}% of the smallest MVCC layout (A-D)", cap, cap as f64 * PAGE as f64 / 1048576.0, frac * 100.0);
        for a in arms[..paged].iter_mut() {
            println!("- {}: cache = {:.1}% of its {} pages", a.name, cap as f64 / a.pool.npages() as f64 * 100.0, a.pool.npages());
            a.pool.set_capacity(cap);
        }
        // Warm the constrained cache with a separate sample (not measured).
        let warm_c = |r: usize| -> Vec<OpSet> {
            let mut rr = Rng(0xC0FFEE + r as u64);
            vec![OpSet { name: "constrained warm-up: 1-hop".into(), kind: Kind::Hop1(None), starts: (0..sc_n(5000)).map(|_| rr.below(n)).collect(), skip_d: false }]
        };
        let (arms_p, _) = arms.split_at_mut(paged);
        for &direct in modes {
            for a in arms_p.iter_mut() {
                a.pool.set_capacity(cap);
                a.pool.set_direct(direct);
            }
            bench(&format!("constrained {:.0}% warm-up (discarded)", frac * 100.0), arms_p, false, &warm_c, 1, false, Some(Duration::from_secs(20)), snap, &mut sc, &expect);
            if direct {
                // Device reads: 1-hop and 2-hop only (3-hop/BFS on record chains would take ~10^3 s per op).
                let per_round = |r: usize| {
                    let mut v = mk_sets(0xBEEF00 + r as u64, false).into_iter().filter(|x| matches!(x.kind, Kind::Hop1(_) | Kind::KHop(2))).collect::<Vec<_>>();
                    v[4].skip_d = true; // 2-hop degree-weighted: D took 18.6 s/op at 25 % (P4)
                    v
                };
                bench(&format!("Constrained {:.0}%, misses read from the device (pread, O_DIRECT)", frac * 100.0), arms_p, false, &per_round, 3, false, Some(Duration::from_secs(5)), snap, &mut sc, &expect);
            } else {
                let per_round = |r: usize| {
                    let mut v = mk_sets(0xBEEF00 + r as u64, false);
                    v[5].skip_d = true; // 3-hop: D took 7.1 s/op at 25 % (P4)
                    v[6].skip_d = true; // BFS: D took 57 s/op at 25 % (P4)
                    v
                };
                bench(&format!("Constrained {:.0}%, misses served by the OS page cache (buffered pread)", frac * 100.0), arms_p, false, &per_round, 3, false, Some(Duration::from_secs(8)), snap, &mut sc, &expect);
            }
        }
        for a in arms_p.iter_mut() {
            a.pool.set_direct(true);
        }
    }
    for a in arms[..paged].iter_mut() {
        a.pool.set_capacity(usize::MAX);
        a.pool.preload();
    }

    // ---------------------------------------------------------------- inserts and deletes
    let rounds = 3;
    let k = sc_n(50000);
    let snap_old = snap;
    let mut next_eid = m;
    let mut ts = m + 1;
    let mut inserted: Vec<(u32, u32, u8, u64)> = Vec::new();
    println!("\n### Online single-edge insert on the built graph (all resident; {} rounds x {} edges, interleaved)\n", rounds, k);
    println!("| layout | ns/insert median [min, max] | page fetches/insert | pages dirtied/insert | extra |");
    println!("|---|---|---|---|---|");
    let mut ins_acc: Vec<Vec<(f64, u64, u64, Stats, u64)>> = vec![Vec::new(); mvcc];
    for r in 0..rounds {
        let batch: Vec<(u32, u32, u8, u64)> = (0..k).map(|_| {
            let (s, d, t) = smp.edge();
            let e = next_eid;
            next_eid += 1;
            (s, d, t, e)
        }).collect();
        let mut order: Vec<usize> = (0..mvcc).collect();
        order.rotate_left(r % mvcc);
        for &ai in &order {
            let a = &mut arms[ai];
            let st0 = a.pool.st;
            let rw0 = if let Lay::D(x) = &a.lay { x.rewrites } else { 0 };
            let t0 = Instant::now();
            for (j, &(s, d, t, e)) in batch.iter().enumerate() {
                a.pool.begin_op();
                a.insert(s as u64, d as u64, t as u32, e, ts + j as u64);
            }
            let tot = t0.elapsed().as_nanos() as u64;
            let rw = if let Lay::D(x) = &a.lay { x.rewrites - rw0 } else { 0 };
            ins_acc[ai].push((tot as f64 / k as f64, 0, 0, a.pool.st.since(&st0), rw));
        }
        ts += k as u64;
        inserted.extend(batch);
    }
    for ai in 0..mvcc {
        let v = &ins_acc[ai];
        let mut per: Vec<f64> = v.iter().map(|x| x.0).collect();
        let (mn, mx) = (per.iter().cloned().fold(f64::INFINITY, f64::min), per.iter().cloned().fold(0.0, f64::max));
        let f: u64 = v.iter().map(|x| x.3.fetches).sum();
        let d: u64 = v.iter().map(|x| x.3.dirtied).sum();
        let rw: u64 = v.iter().map(|x| x.4).sum();
        let tot = (rounds * k) as f64;
        let extra = if matches!(arms[ai].lay, Lay::D(_)) { format!("existing records rewritten/insert {:.2}", rw as f64 / tot) } else { String::new() };
        println!("| {} | {:.0} [{:.0}, {:.0}] | {:.2} | {:.2} | {} |", arms[ai].name, median(&mut per), mn, mx, f as f64 / tot, d as f64 / tot, extra);
    }

    // Deletes of random original edges.
    let mut rng = Rng(0xDE1E7E);
    let mut del: Vec<u64> = (0..m).collect();
    for i in 0..(rounds * k).min(m as usize) {
        let j = i + rng.below(m - i as u64) as usize;
        del.swap(i, j);
    }
    del.truncate((rounds * k).min(m as usize));
    println!("\n### Delete (set deleted_ts) of random existing edges by edge id (all resident; {} rounds x {} edges, interleaved)\n", rounds, k);
    println!("| layout | ns/delete median [min, max] | page fetches/delete | pages dirtied/delete |");
    println!("|---|---|---|---|");
    let mut del_acc: Vec<Vec<(f64, u64, u64, Stats)>> = vec![Vec::new(); mvcc];
    for r in 0..rounds {
        let chunk = &del[r * k..(r + 1) * k];
        let mut order: Vec<usize> = (0..mvcc).collect();
        order.rotate_left(r % mvcc);
        for &ai in &order {
            let a = &mut arms[ai];
            let st0 = a.pool.st;
            let t0 = Instant::now();
            let mut ok = true;
            for (j, &e) in chunk.iter().enumerate() {
                a.pool.begin_op();
                ok &= a.delete(e, ts + j as u64);
            }
            let tot = t0.elapsed().as_nanos() as u64;
            assert!(ok, "{} failed to delete an edge", a.name);
            del_acc[ai].push((tot as f64 / k as f64, 0, 0, a.pool.st.since(&st0)));
        }
        ts += k as u64;
    }
    for ai in 0..mvcc {
        let v = &del_acc[ai];
        let mut per: Vec<f64> = v.iter().map(|x| x.0).collect();
        let (mn, mx) = (per.iter().cloned().fold(f64::INFINITY, f64::min), per.iter().cloned().fold(0.0, f64::max));
        let f: u64 = v.iter().map(|x| x.3.fetches).sum();
        let d: u64 = v.iter().map(|x| x.3.dirtied).sum();
        let tot = (rounds * k) as f64;
        println!("| {} | {:.0} [{:.0}, {:.0}] | {:.2} | {:.2} |", arms[ai].name, median(&mut per), mn, mx, f as f64 / tot, d as f64 / tot);
    }

    // ---------------------------------------------------------------- MVCC visibility control
    let snap_new = ts;
    let mut ins_cnt = vec![0u32; n as usize];
    for &(s, _, _, _) in &inserted {
        ins_cnt[s as usize] += 1;
    }
    let mut del_cnt = vec![0u32; n as usize];
    for &e in &del {
        del_cnt[g.src[e as usize] as usize] += 1;
    }
    let mut starts: Vec<u64> = del.iter().take(2000).map(|&e| g.src[e as usize] as u64).collect();
    starts.extend(inserted.iter().take(1000).map(|x| x.0 as u64));
    println!("\n### MVCC visibility control (A-D must agree and match the generator's counts)\n");
    println!("| layout | edges seen at snapshot before inserts/deletes | edges seen at latest snapshot |");
    println!("|---|---|---|");
    let exp_old: u64 = starts.iter().map(|&s| outdeg[s as usize] as u64).sum();
    let exp_new: u64 = starts.iter().map(|&s| (outdeg[s as usize] - del_cnt[s as usize] + ins_cnt[s as usize]) as u64).sum();
    let mut prev: Option<(Vec<(u64, u64)>, Vec<(u64, u64)>)> = None;
    for a in arms[..mvcc].iter_mut() {
        let old: Vec<(u64, u64)> = starts.iter().map(|&s| a.hop1(s, None, snap_old, &mut sc.tmp)).collect();
        let new: Vec<(u64, u64)> = starts.iter().map(|&s| a.hop1(s, None, snap_new, &mut sc.tmp)).collect();
        let so: u64 = old.iter().map(|x| x.0).sum();
        let sn: u64 = new.iter().map(|x| x.0).sum();
        println!("| {} | {} | {} |", a.name, so, sn);
        assert_eq!(so, exp_old, "{}: old snapshot count", a.name);
        assert_eq!(sn, exp_new, "{}: new snapshot count", a.name);
        if let Some((po, pn)) = &prev {
            assert!(po == &old && pn == &new, "{}: visibility results differ across layouts", a.name);
        }
        prev = Some((old, new));
    }
    println!("\nexpected from generator: {} / {} (difference proves the stamps are checked)", exp_old, exp_new);
    sizes(&mut arms, m, n);
    println!("\nRSS end: {} MiB, peak {} MiB", util::rss_kib() / 1024, util::hwm_kib() / 1024);
    for name in ["A", "B", "C", "D", "E", "F", "G"] {
        let _ = std::fs::remove_file(dir.join(format!("{name}.pages")));
    }
}
