//! `lab::btree`: the B+tree (task #14).
//!
//! Each distribution (Zipf 0.8, uniform) builds one database under
//! `$LPG_LAB_EXT4/btree-<dist>`: M-LAYOUT's graph of 10^6 nodes and 10^7
//! edges, each edge an OUT entry at its source and an IN entry at its
//! target, so 2×10^7 adjacency-shaped keys `(u64, u8, u32, u64, u64)`
//! (29 bytes, big-endian, empty values), inserted in random (generation)
//! order, one edge per batch, with the whole tree in cache. The database is
//! then written and synced, and reused by the other workloads.
//!
//! - [`btree`]: builds both databases if missing, then per cache share
//!   (100 %, 10 %, 1 % of the data pages) measures, with the OS cache warm
//!   and in cold mode (OS cache evicted before each round): point `get` of
//!   existing keys, prefix scans of a `(u64, u8, u32)` prefix of an existing
//!   key, and `put` of new keys, one per batch. It also reports the shape
//!   of each tree against M-LAYOUT layout B, and in-leaf search costs.
//! - [`btree_fixed`]: a fixed run for `perf` and heaptrack:
//!   `LPG_BTREE_DIST` (`zipf`, `uniform`), `LPG_BTREE_SHARE` (100, 10, 1),
//!   `LPG_BTREE_MODE` (`get`, `scan`, `put`), `LPG_PERF_N` operations (0
//!   gives the setup to subtract). Puts run only at 100 %, where no frame is
//!   evicted, so the database on disk is not changed.

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::generator::{Sampler, ZIPF_ALPHA};
use super::pool::cap_for;
use super::{
    Measured, Probe, env_dir, evict_from_os_cache, measure, median_range, write_table, write_text,
};
use crate::obs::recording::RecordingSink;
use crate::storage::btree::node::{self, Found};
use crate::storage::btree::{Store, Tree, Write};
use crate::storage::engine::{CONTROL, Engine, data_name};
use crate::storage::page::PAGE_SIZE;
use crate::storage::{FsStorage, Storage};

const NODES: u64 = 1_000_000;
const EDGES: u64 = 10_000_000;
/// Every `SAMPLE`-th edge is kept for the read workloads.
const SAMPLE: u64 = 10;
/// Build cap: the whole tree stays in the pool.
const BUILD_CAP: u64 = 3 << 30;

fn key(node: u64, dir: u8, ty: u32, nbr: u64, edge: u64) -> [u8; 29] {
    let mut k = [0u8; 29];
    k[..8].copy_from_slice(&node.to_be_bytes());
    k[8] = dir;
    k[9..13].copy_from_slice(&ty.to_be_bytes());
    k[13..21].copy_from_slice(&nbr.to_be_bytes());
    k[21..].copy_from_slice(&edge.to_be_bytes());
    k
}

fn sampler(dist: &str) -> Sampler {
    match dist {
        "zipf" => Sampler::zipf(NODES, ZIPF_ALPHA, 14),
        "uniform" => Sampler::uniform(NODES, 14),
        _ => panic!("unknown distribution {dist}"),
    }
}

/// The two entries of edge `e` drawn from `s`: OUT at the source, IN at
/// the target.
fn edge_keys(s: &mut Sampler, e: u64) -> [[u8; 29]; 2] {
    let (src, _, ty, dst, _) = s.adj_key(e);
    [key(src, 0, ty, dst, e), key(dst, 1, ty, src, e)]
}

fn dir_of(dist: &str) -> PathBuf {
    env_dir("LPG_LAB_EXT4").join(format!("btree-{dist}"))
}

fn sink() -> Arc<RecordingSink> {
    Arc::new(RecordingSink::default())
}

/// Builds the database of `dist` if missing; returns its build line.
fn build(dist: &str) -> Option<String> {
    let dir = dir_of(dist);
    if FsStorage.open(&dir.join(CONTROL)).is_ok() {
        return None;
    }
    match rustix::fs::mkdir(&dir, rustix::fs::Mode::from_raw_mode(0o755)) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(e) => panic!("mkdir {}: {e}", dir.display()),
    }
    let e = Engine::create(&dir, Arc::new(FsStorage), BUILD_CAP, sink()).expect("create");
    let s = e.store();
    let tree = s.create_tree().expect("tree");
    let mut smp = sampler(dist);
    let p0 = Probe::now();
    let t = Instant::now();
    for edge in 0..EDGES {
        let [a, b] = edge_keys(&mut smp, edge);
        s.apply(&[
            Write {
                tree,
                key: &a,
                value: Some(&[]),
            },
            Write {
                tree,
                key: &b,
                value: Some(&[]),
            },
        ])
        .expect("put");
    }
    let secs = t.elapsed().as_secs_f64();
    let p1 = Probe::now();
    let t = Instant::now();
    let written = e.pool().write_dirty().expect("write");
    e.data().sync().expect("sync");
    let flush = t.elapsed().as_secs_f64();
    Some(format!(
        "| {dist} | {} | {secs:.1} | {:.0} | {:.0} | {written} pages, {flush:.1} s | {} |",
        2 * EDGES,
        2.0 * EDGES as f64 / secs,
        secs * 1e9 / (2 * EDGES) as f64,
        p1.hwm_kib - p0.hwm_kib.min(p1.hwm_kib),
    ))
}

/// The keys kept for the read workloads: both entries of every
/// `SAMPLE`-th edge.
fn sample_keys(dist: &str) -> Vec<[u8; 29]> {
    let mut smp = sampler(dist);
    let mut out = Vec::with_capacity((2 * EDGES / SAMPLE) as usize);
    for edge in 0..EDGES {
        let ks = edge_keys(&mut smp, edge);
        if edge % SAMPLE == 0 {
            out.extend(ks);
        }
    }
    out
}

fn open(dist: &str, cap: u64, sink: Arc<RecordingSink>) -> Engine {
    Engine::open(&dir_of(dist), Arc::new(FsStorage), cap, sink).expect("open")
}

/// Pages of the data in use, from page 0 of a closed database.
fn data_pages(dist: &str) -> u64 {
    let e = open(dist, 64 << 20, sink());
    e.data().page_count()
}

/// Touches every page of `tree` in key order: leaves by a full scan.
fn warm(s: &Store<'_>, tree: Tree) -> u64 {
    let mut n = 0u64;
    s.scan_prefix(tree, &[], |_, _| {
        n += 1;
        true
    })
    .expect("scan");
    n
}

#[inline(never)]
fn get_loop(s: &Store<'_>, tree: Tree, keys: &[[u8; 29]], picks: &[u32]) -> u64 {
    let mut out = Vec::new();
    let mut hits = 0;
    for &i in picks {
        hits += u64::from(s.get(tree, &keys[i as usize], &mut out).expect("get"));
    }
    assert_eq!(hits, picks.len() as u64, "a stored key was not found");
    hits
}

#[inline(never)]
fn scan_loop(s: &Store<'_>, tree: Tree, keys: &[[u8; 29]], picks: &[u32]) -> u64 {
    let mut n = 0u64;
    for &i in picks {
        s.scan_prefix(tree, &keys[i as usize][..13], |k, _| {
            black_box(k);
            n += 1;
            true
        })
        .expect("scan");
    }
    n
}

#[inline(never)]
fn put_loop(s: &Store<'_>, tree: Tree, keys: &[[u8; 29]]) {
    for k in keys {
        s.apply(&[Write {
            tree,
            key: k,
            value: Some(&[]),
        }])
        .expect("put");
    }
}

/// New keys for puts: edges beyond the built ones.
fn new_keys(dist: &str, first_edge: u64, n: u64) -> Vec<[u8; 29]> {
    let mut smp = sampler(dist);
    // A different stream from the build's: reseed by skipping nothing but
    // using fresh edge ids; endpoints follow the same distribution.
    let mut rng = StdRng::seed_from_u64(first_edge);
    for _ in 0..rng.random_range(1..1000) {
        smp.node();
    }
    (0..n.div_ceil(2))
        .flat_map(|i| edge_keys(&mut smp, first_edge + i))
        .take(n as usize)
        .collect()
}

fn picks(n: usize, len: usize, seed: u64) -> Vec<u32> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..n).map(|_| rng.random_range(0..len as u32)).collect()
}

/// In-leaf costs on resident leaf images: ns per `leaf_get` of a key in
/// the leaf, and ns per entry of a full decode (key reconstruction).
fn leaf_costs(s: &Store<'_>, tree: Tree, keys: &[[u8; 29]]) -> (Vec<f64>, Vec<f64>, f64) {
    // Copies of the leaves of 2 000 sampled keys.
    let mut leaves: Vec<(Vec<u8>, [u8; 29])> = Vec::new();
    for &i in &picks(2000, keys.len(), 5) {
        let k = keys[i as usize];
        let mut p = tree.root();
        let img = loop {
            let pin = s.pool.fetch(p).expect("fetch");
            let g = pin.read().expect("latch");
            if node::level(&g) == 0 {
                break g.to_vec();
            }
            p = node::inner_child(&g, &k).expect("inner").1;
        };
        leaves.push((img, k));
    }
    let mut buf = Vec::with_capacity(1024);
    let mut search = Vec::new();
    let mut decode = Vec::new();
    let mut entries = 0usize;
    for (img, _) in &leaves {
        entries += node::leaf_entries(img, |_, _| {}).expect("leaf").count();
    }
    for _ in 0..3 {
        let t = Instant::now();
        for _ in 0..50 {
            for (img, k) in &leaves {
                let f = node::leaf_get(black_box(img), k, &mut buf).expect("leaf");
                assert!(matches!(f, Found::Hit(_)));
            }
        }
        search.push(t.elapsed().as_nanos() as f64 / (50 * leaves.len()) as f64);
        let t = Instant::now();
        let mut n = 0usize;
        for _ in 0..5 {
            for (img, _) in &leaves {
                node::leaf_for_each_from(black_box(img), &[], &mut buf, |k, _| {
                    black_box(k);
                    n += 1;
                    true
                })
                .expect("leaf");
            }
        }
        decode.push(t.elapsed().as_nanos() as f64 / n as f64);
    }
    (search, decode, entries as f64 / leaves.len() as f64)
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn btree() {
    const GETS: u64 = 200_000;
    const SCANS: u64 = 50_000;
    const PUTS: u64 = 20_000;
    const COLD_GETS: u64 = 5_000;
    const COLD_SCANS: u64 = 2_000;
    const COLD_PUTS: u64 = 2_000;
    let mut builds = String::new();
    let mut shapes = String::new();
    let mut leaf = String::new();
    let mut rows: Vec<Measured> = Vec::new();
    let mut extra = String::new();
    for dist in ["zipf", "uniform"] {
        if let Some(line) = build(dist) {
            builds.push_str(&line);
            builds.push('\n');
        }
        let keys = sample_keys(dist);
        let pages = data_pages(dist);
        // Shape, from the integrity check at full cache.
        {
            let e = open(dist, BUILD_CAP, sink());
            let s = e.store();
            let t = Instant::now();
            let r = s.check().expect("check");
            let check_s = t.elapsed().as_secs_f64();
            assert!(r.is_ok(), "{:?}", r.problems);
            let st = &r.trees[1];
            writeln!(
                shapes,
                "| {dist} | {} | {} | {} | {} | {} | {:.2} | {:.3} | {:.2} | {check_s:.1} |",
                st.entries,
                st.height,
                st.leaves,
                st.inner,
                r.page_count,
                st.leaf_bytes as f64 / st.entries as f64,
                st.leaf_bytes as f64 / (st.leaves * PAGE_SIZE as u64) as f64,
                (r.page_count * PAGE_SIZE as u64) as f64 / st.entries as f64,
            )
            .unwrap();
            let tree = s.tree(1).expect("dir").expect("tree 1");
            warm(&s, tree);
            let (search, decode, per_leaf) = leaf_costs(&s, tree, &keys);
            writeln!(
                leaf,
                "| {dist} | {per_leaf:.0} | {} | {} |",
                median_range(&search),
                format_ns(&decode)
            )
            .unwrap();
        }
        let mut next_edge = EDGES;
        for percent in [100u64, 10, 1] {
            let pool_pages = (pages * percent).div_ceil(100);
            let sink = sink();
            let e = open(dist, cap_for(pool_pages), sink.clone());
            let s = e.store();
            let tree = s.tree(1).expect("dir").expect("tree 1");
            let data = dir_of(dist).join(data_name(1));
            let label = |mode: &str| format!("{dist}, {percent} %, {mode}");
            for cold in [false, true] {
                let mode = if cold { "cold" } else { "warm OS cache" };
                // Fill the pool: at 100 % with every page, else with the
                // pages a uniform read sequence leaves.
                warm(&s, tree);
                get_loop(&s, tree, &keys, &picks(GETS as usize, keys.len(), 7));
                let prep = |_| {
                    if cold {
                        evict_from_os_cache(&data);
                    }
                };
                let (g, sc, pu) = if cold {
                    (COLD_GETS, COLD_SCANS, COLD_PUTS)
                } else {
                    (GETS, SCANS, PUTS)
                };
                let gp: Vec<Vec<u32>> = (0..3)
                    .map(|r| picks(g as usize, keys.len(), 100 + r))
                    .collect();
                rows.push(measure(
                    &format!("get ({dist})"),
                    &label(mode),
                    g,
                    &sink,
                    prep,
                    |r| {
                        get_loop(&s, tree, &keys, &gp[r]);
                    },
                ));
                let sp: Vec<Vec<u32>> = (0..3)
                    .map(|r| picks(sc as usize, keys.len(), 200 + r))
                    .collect();
                let mut found = 0u64;
                rows.push(measure(
                    &format!("prefix scan ({dist})"),
                    &label(mode),
                    sc,
                    &sink,
                    prep,
                    |r| {
                        found += scan_loop(&s, tree, &keys, &sp[r]);
                    },
                ));
                writeln!(
                    extra,
                    "| {dist} | {percent} % | {mode} | {:.1} |",
                    found as f64 / (3 * sc) as f64
                )
                .unwrap();
                let nk: Vec<Vec<[u8; 29]>> = (0..3)
                    .map(|_| {
                        let v = new_keys(dist, next_edge, pu);
                        next_edge += pu;
                        v
                    })
                    .collect();
                rows.push(measure(
                    &format!("put ({dist})"),
                    &label(mode),
                    pu,
                    &sink,
                    prep,
                    |r| {
                        put_loop(&s, tree, &nk[r]);
                    },
                ));
            }
            e.pool().write_dirty().expect("write");
            e.data().sync().expect("sync");
            let r = s.check().expect("check");
            assert!(r.is_ok(), "{:?}", r.problems);
        }
    }
    let out = env_dir("LPG_LAB_OUT");
    let md = format!(
        "# lab::btree build, shape and in-leaf costs\n\n\
         ## Build (random order, one edge = two entries per batch, full cache)\n\n\
         | dist | entries | seconds | entries/s | ns/entry | write_dirty | VmHWM growth KiB |\n|---|---|---|---|---|---|---|\n{builds}\n\
         ## Shape (integrity check figures)\n\n\
         | dist | entries | height | leaves | inner | pages | leaf B/entry | leaf fill | file B/entry | check s |\n|---|---|---|---|---|---|---|---|---|---|\n{shapes}\n\
         ## In-leaf costs (2 000 resident leaf images, 3 rounds)\n\n\
         | dist | entries/leaf | ns per leaf_get median [min, max] | ns per entry decoded median [min, max] |\n|---|---|---|---|\n{leaf}\n\
         ## Entries per prefix scan\n\n| dist | cache | mode | entries/scan |\n|---|---|---|---|\n{extra}"
    );
    write_text(&out.join("t14-lab-btree-shape.md"), &md);
    println!("{md}");
    write_table(
        "t14-lab-btree.md",
        "lab::btree",
        &[
            ("directory", env_dir("LPG_LAB_EXT4").display().to_string()),
            (
                "data",
                format!(
                    "M-LAYOUT graph, {NODES} nodes, {EDGES} edges, 2 entries per edge, 29-byte keys, empty values"
                ),
            ),
            (
                "cache share",
                "pool page buffers as % of the data pages (`cap_for`)".to_owned(),
            ),
            (
                "cold",
                "OS cache of data-1 evicted (fadvise DONTNEED) before each round; pool kept"
                    .to_owned(),
            ),
            (
                "put",
                "one key per batch (`apply`), new edges of the same distribution".to_owned(),
            ),
        ],
        &rows,
    );
}

fn format_ns(v: &[f64]) -> String {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    format!("{:.1} [{:.1}, {:.1}]", s[1], s[0], s[2])
}

#[test]
#[ignore = "laboratory run for perf and heaptrack; see the module documentation"]
fn btree_fixed() {
    let dist = std::env::var("LPG_BTREE_DIST").unwrap_or_else(|_| "uniform".into());
    let percent: u64 = std::env::var("LPG_BTREE_SHARE").map_or(100, |v| v.parse().unwrap());
    let mode = std::env::var("LPG_BTREE_MODE").expect("set LPG_BTREE_MODE");
    let n: u64 = std::env::var("LPG_PERF_N").map_or(100_000, |v| v.parse().unwrap());
    let keys = sample_keys(&dist);
    let pages = data_pages(&dist);
    let cap = if mode == "put" {
        assert_eq!(percent, 100, "puts run only at 100 %");
        BUILD_CAP
    } else {
        cap_for((pages * percent).div_ceil(100))
    };
    let e = open(&dist, cap, sink());
    let s = e.store();
    let tree = s.tree(1).expect("dir").expect("tree 1");
    warm(&s, tree);
    get_loop(&s, tree, &keys, &picks(200_000, keys.len(), 7));
    let p = picks(n as usize, keys.len(), 11);
    let new = if mode == "put" {
        new_keys(&dist, 4 * EDGES, n)
    } else {
        Vec::new()
    };
    let t = Instant::now();
    match mode.as_str() {
        "get" => drop(get_loop(&s, tree, &keys, &p)),
        "scan" => drop(scan_loop(&s, tree, &keys, &p)),
        "put" => put_loop(&s, tree, &new),
        other => panic!("unknown LPG_BTREE_MODE {other}"),
    }
    let ns = t.elapsed().as_nanos() as f64 / n.max(1) as f64;
    println!("LAB dist={dist} share={percent} mode={mode} n={n} ns_per_op={ns:.0}");
    // Nothing is written back: at 100 % no frame was evicted.
}
