//! Large-scale point: bulk load of the leading layouts with a bounded build
//! cache, then reads with the cache at 10 % and 1 % of the smallest MVCC layout,
//! misses read from the device (O_DIRECT). No all-resident phase (the data does
//! not fit in RAM next to the other arms), no inserts or deletes.
//!
//! Usage: layout-bench large <zipf|uniform> <nodes> <edges> <workdir> <arms, e.g. ABFG> <build cache MiB>

use crate::adj::{self, Adj, AdjA, AdjB, CompTree, IN, LIVE, OUT};
use crate::btree::FixedTree;
use crate::csr::Csr;
use crate::generator::{self, Sampler};
use crate::pcsr::PCsr;
use crate::pool::{PAGE, Pool};
use crate::util::{self, Rng};
use crate::{Arm, Kind, Lay, OpSet, Scratch, bench, sizes};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn run(args: &[String]) {
    let alpha = if args[2] == "zipf" { 0.8 } else { 0.0 };
    let n: u64 = args[3].parse().expect("nodes");
    let m: u64 = args[4].parse().expect("edges");
    let dir = PathBuf::from(&args[5]);
    let want = args[6].clone();
    let build_mib: usize = args[7].parse().expect("build cache MiB");
    let build_frames = build_mib * 1048576 / PAGE;
    std::fs::create_dir_all(&dir).expect("workdir");
    println!("# layout-bench large {} bulk n={} m={} alpha={} arms={} build cache {} MiB ({} frames per arm)", args[2], n, m, alpha, want, build_mib, build_frames);

    let t = Instant::now();
    let mut smp = Sampler::new(n, alpha, 0x5EED_0001 ^ (alpha.to_bits()));
    let g = generator::generate(&mut smp, m);
    drop(smp);
    let mut outdeg = vec![0u32; n as usize];
    for &s in &g.src {
        outdeg[s as usize] += 1;
    }
    let mut sd = outdeg.clone();
    sd.sort_unstable();
    println!("graph generated in {:.1}s: max out-degree {}, out-degree p50 {} p99 {} p99.9 {}", t.elapsed().as_secs_f64(), sd[sd.len() - 1], sd[n as usize / 2], sd[n as usize * 99 / 100], sd[n as usize * 999 / 1000]);
    drop(sd);

    let t = Instant::now();
    let mut o_out: Vec<u32> = (0..m as u32).collect();
    o_out.sort_unstable_by_key(|&i| (g.src[i as usize], g.ty[i as usize], g.dst[i as usize], i));
    let mut o_in: Vec<u32> = (0..m as u32).collect();
    o_in.sort_unstable_by_key(|&i| (g.dst[i as usize], g.ty[i as usize], g.src[i as usize], i));
    let sort_s = t.elapsed().as_secs_f64();
    println!("sort of edge indexes by (src, type, dst, id) and (dst, type, src, id): {:.1}s (shared by all layouts, not included below)", sort_s);


    println!("\n### Bulk load (bounded build cache, dirty pages written back with O_DIRECT)\n");
    println!("| layout | seconds | edges/s | pages | GiB written | peak RSS during build (MiB) | RSS before (MiB) |");
    println!("|---|---|---|---|---|---|---|");
    let mut arms: Vec<Arm> = Vec::new();
    let mk = |name: &str| {
        let mut p = Pool::create(&dir.join(format!("{name}.pages")));
        p.set_capacity(build_frames);
        p
    };
    let mut csr: Option<Csr> = None;
    for name in ["A", "B", "F", "G"] {
        if !want.contains(name) {
            continue;
        }
        let mut pool = mk(name);
        let rss0 = util::rss_kib();
        util::reset_hwm();
        let w0 = util::io_write_bytes();
        let t = Instant::now();
        let (lay, aname): (Lay, &'static str) = match name {
            "A" => {
                let mut it = merged_iter(&g, &o_out, &o_in).map(|a| adj::enc_a(&a).to_vec());
                let t = FixedTree::bulk(&mut pool, adj::AE, adj::KLEN, &mut it, 0.9);
                let mut eit = (0..m as usize).map(|i| adj::enc_edge(i as u64, g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32).to_vec());
                let et = FixedTree::bulk(&mut pool, adj::EE, 8, &mut eit, 1.0);
                (Lay::A(AdjA { t, et }), "A btree-fixed")
            }
            "B" => {
                let mut it = merged_iter(&g, &o_out, &o_in);
                let t = CompTree::bulk(&mut pool, &mut it, 0.9);
                let mut eit = (0..m as usize).map(|i| adj::enc_edge(i as u64, g.src[i] as u64, g.dst[i] as u64, g.ty[i] as u32).to_vec());
                let et = FixedTree::bulk(&mut pool, adj::EE, 8, &mut eit, 1.0);
                (Lay::B(AdjB { t, et }), "B btree-compressed")
            }
            _ => {
                if csr.is_none() {
                    let mut c = Csr::builder(n as usize);
                    for &e in &o_out {
                        let e = e as usize;
                        c.push(g.src[e] as u64, OUT, g.ty[e] as u32, g.dst[e] as u64, e as u64);
                    }
                    for &e in &o_in {
                        let e = e as usize;
                        c.push(g.dst[e] as u64, IN, g.ty[e] as u32, g.src[e] as u64, e as u64);
                    }
                    c.finish();
                    csr = Some(c);
                }
                let comp = name == "G";
                let x = PCsr::build(&mut pool, csr.as_ref().unwrap(), comp);
                (Lay::F(x), if comp { "G csr-paged-compressed" } else { "F csr-paged" })
            }
        };
        pool.flush_all();
        let secs = t.elapsed().as_secs_f64();
        let wb = util::io_write_bytes() - w0;
        println!("| {} | {:.1} | {:.0} | {} | {:.2} | {} | {} |", aname, secs, m as f64 / secs, pool.npages(), wb as f64 / (1u64 << 30) as f64, util::hwm_kib() / 1024, rss0 / 1024);
        arms.push(Arm { name: aname, pool, lay });
    }
    drop(csr);
    println!("\n(F and G time includes building the in-memory CSR from the sorted indexes once, charged to the first of them.)");
    sizes(&mut arms, m, n);
    println!("\nRSS after build: {} MiB", util::rss_kib() / 1024);

    // ---------------------------------------------------------------- constrained device reads
    let mut sc = Scratch { seen: vec![0; n as usize], epoch: 0, fr: Vec::new(), nx: Vec::new(), tmp: Vec::new() };
    let src = &g.src;
    let mk_sets = |seed: u64| -> Vec<OpSet> {
        let mut r = Rng(seed);
        let uni = |k: usize, r: &mut Rng| (0..k).map(|_| r.below(n)).collect::<Vec<u64>>();
        let dw = |k: usize, r: &mut Rng| (0..k).map(|_| src[r.below(m) as usize] as u64).collect::<Vec<u64>>();
        vec![
            OpSet { name: "1-hop OUT, all types, uniform start".into(), kind: Kind::Hop1(None), starts: uni(2000, &mut r), skip_d: false },
            OpSet { name: "1-hop OUT, type = 2, uniform start".into(), kind: Kind::Hop1(Some(2)), starts: uni(2000, &mut r), skip_d: false },
            OpSet { name: "1-hop OUT, all types, degree-weighted start".into(), kind: Kind::Hop1(None), starts: dw(2000, &mut r), skip_d: false },
            OpSet { name: "2-hop OUT (distinct), uniform start".into(), kind: Kind::KHop(2), starts: uni(100, &mut r), skip_d: false },
            OpSet { name: "2-hop OUT (distinct), degree-weighted start".into(), kind: Kind::KHop(2), starts: dw(30, &mut r), skip_d: false },
        ]
    };
    let od = outdeg.clone();
    let expect = move |set: &OpSet, i: usize| -> Option<u64> {
        match set.kind {
            Kind::Hop1(None) => Some(od[set.starts[i] as usize] as u64),
            _ => None,
        }
    };
    let mvcc_min = arms.iter().filter(|a| matches!(a.lay, Lay::A(_) | Lay::B(_))).map(|a| a.pool.npages()).min().unwrap_or_else(|| arms.iter().map(|a| a.pool.npages()).min().unwrap_or(1)) as usize;
    for frac in [0.10f64, 0.01] {
        let cap = ((mvcc_min as f64) * frac) as usize;
        println!("\n## Constrained cache: {} frames = {:.1} MiB = {:.0}% of the smallest MVCC layout", cap, cap as f64 * PAGE as f64 / 1048576.0, frac * 100.0);
        for a in arms.iter_mut() {
            println!("- {}: cache = {:.2}% of its {} pages", a.name, cap as f64 / a.pool.npages() as f64 * 100.0, a.pool.npages());
            a.pool.set_capacity(cap);
            a.pool.set_direct(true);
        }
        let warm_c = |r: usize| -> Vec<OpSet> {
            let mut rr = Rng(0xC0FFEE + r as u64);
            vec![OpSet { name: "constrained warm-up: 1-hop".into(), kind: Kind::Hop1(None), starts: (0..5000).map(|_| rr.below(n)).collect(), skip_d: false }]
        };
        for direct in [false, true] {
            for a in arms.iter_mut() {
                a.pool.set_capacity(cap);
                a.pool.set_direct(direct);
            }
            bench(&format!("constrained {:.0}% warm-up (discarded)", frac * 100.0), &mut arms, false, &warm_c, 1, false, Some(Duration::from_secs(20)), 0, &mut sc, &expect);
            let what = if direct { "misses read from the device (pread, O_DIRECT)" } else { "misses served by the OS page cache (buffered pread)" };
            bench(&format!("Constrained {:.0}%, {}", frac * 100.0, what), &mut arms, false, &|r| mk_sets(0xBEEF00 + r as u64), 3, false, Some(Duration::from_secs(5)), 0, &mut sc, &expect);
        }
    }
    println!("\nRSS end: {} MiB, peak since last reset {} MiB", util::rss_kib() / 1024, util::hwm_kib() / 1024);
    for name in ["A", "B", "F", "G"] {
        let _ = std::fs::remove_file(dir.join(format!("{name}.pages")));
    }
}

/// Adjacency entries in key order (node, dir, type, neighbour, edge id): merge of the two orders.
fn merged_iter<'a>(g: &'a generator::Graph, o_out: &'a [u32], o_in: &'a [u32]) -> impl Iterator<Item = Adj> + 'a {
    let (mut i, mut j) = (0usize, 0usize);
    std::iter::from_fn(move || {
        let take_out = if i < o_out.len() && j < o_in.len() { g.src[o_out[i] as usize] <= g.dst[o_in[j] as usize] } else { i < o_out.len() };
        if take_out {
            let e = o_out[i] as usize;
            i += 1;
            Some(Adj { node: g.src[e] as u64, dir: OUT, ty: g.ty[e] as u32, nbr: g.dst[e] as u64, eid: e as u64, cts: 0, dts: LIVE })
        } else if j < o_in.len() {
            let e = o_in[j] as usize;
            j += 1;
            Some(Adj { node: g.dst[e] as u64, dir: IN, ty: g.ty[e] as u32, nbr: g.src[e] as u64, eid: e as u64, cts: 0, dts: LIVE })
        } else {
            None
        }
    })
}
