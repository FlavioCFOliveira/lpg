//! `lab::bulk`: the external merge sort and the bottom-up bulk build (task
//! #17).
//!
//! [`bulk`] builds, in a new database under `$LPG_LAB_EXT4`, one tree of
//! `LPG_BULK_KEYS` (default 10^8 and 2×10^7) adjacency-shaped keys `(u64,
//! u8, u32, u64, u64)`, 29 bytes, empty values, drawn in random order from
//! M-LAYOUT's uniform generator (10^7 nodes per 10^8 keys): every key goes
//! through the sort, whose reservation is `LPG_BULK_SORT_MIB` (default 512)
//! of the memory cap `LPG_BULK_CAP_MIB` (default 1 024), and the sorted
//! stream through the build. It reports, to `$LPG_LAB_OUT/t17-lab-bulk.md`:
//! keys per second of the sort and of the build, the temporary bytes the
//! process wrote and read (`/proc/self/io`), the sort's runs, the
//! accountant's peak, and the tree's shape with its leaf bytes per entry
//! (integrity-check figures), to compare with M-LAYOUT's bulk load (1.88 M
//! edges/s) and with the tree task #14 built by puts.

use crate::sync::Arc;
use std::fmt::Write as _;
use std::time::Instant;

use super::generator::Sampler;
use super::{Probe, env_dir, env_list, env_or, header, write_text};
use crate::obs::recording::RecordingSink;
use crate::storage::FsStorage;
use crate::storage::engine::Engine;
use crate::storage::page::PAGE_SIZE;
use crate::storage::sort::Sorter;

/// The adjacency key of edge `e` drawn from `s`.
fn adj(s: &mut Sampler, e: u64) -> [u8; 29] {
    let (src, dir, ty, dst, id) = s.adj_key(e);
    let mut k = [0u8; 29];
    k[..8].copy_from_slice(&src.to_be_bytes());
    k[8] = dir;
    k[9..13].copy_from_slice(&ty.to_be_bytes());
    k[13..21].copy_from_slice(&dst.to_be_bytes());
    k[21..].copy_from_slice(&id.to_be_bytes());
    k
}

#[test]
#[ignore = "laboratory workload; see src/lab/mod.rs"]
fn bulk() {
    let sizes = env_list("LPG_BULK_KEYS", &[100_000_000, 20_000_000]);
    let cap: u64 = env_or("LPG_BULK_CAP_MIB", 1024u64) << 20;
    let sort: u64 = env_or("LPG_BULK_SORT_MIB", 512u64) << 20;
    let base = env_dir("LPG_LAB_EXT4");
    let mut md = header(
        "lab::bulk",
        &[
            ("keys", "29-byte adjacency keys (u64, u8, u32, u64, u64), empty values, random order, uniform generator".into()),
            ("memory", format!("cap {} MiB, sort reservation {} MiB", cap >> 20, sort >> 20)),
            ("directory", base.display().to_string()),
        ],
    );
    md.push_str(
        "\n| keys | sort s | sort keys/s | runs | build s | build keys/s | total keys/s | temp MiB written | MiB read | accountant peak MiB | height | leaves | inner | leaf B/entry | leaf fill | VmHWM MiB |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for &n in &sizes {
        let dir = tempfile::tempdir_in(&base).unwrap();
        let tmp = tempfile::tempdir_in(&base).unwrap();
        let sink = Arc::new(RecordingSink::default());
        let e = Engine::create(dir.path(), Arc::new(FsStorage), cap, sink).unwrap();
        let mut s = Sampler::uniform((n / 10).max(1), 17);
        let p0 = Probe::now();
        let t = Instant::now();
        let mut sorter = Sorter::new(&FsStorage, tmp.path(), "bulk", e.pool(), sort).unwrap();
        for i in 0..n {
            sorter.push(&adj(&mut s, i), &[]).unwrap();
        }
        let runs = sorter.runs();
        let mut sorted = sorter.finish().unwrap();
        let sort_s = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let tree = e.bulk_build(&mut sorted).unwrap();
        let build_s = t.elapsed().as_secs_f64();
        drop(sorted);
        let p1 = Probe::now();
        let r = e.store().check().unwrap();
        assert!(r.is_ok(), "{:?}", r.problems);
        let st = r.trees.iter().find(|x| x.id == tree.id()).unwrap();
        let line = format!(
            "| {n} | {sort_s:.1} | {:.0} | {runs} | {build_s:.1} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {} | {} | {} | {:.2} | {:.3} | {:.0} |",
            n as f64 / sort_s,
            n as f64 / build_s,
            n as f64 / (sort_s + build_s),
            (p1.write_bytes - p0.write_bytes) as f64 / 1048576.0,
            (p1.read_bytes - p0.read_bytes) as f64 / 1048576.0,
            e.accountant().peak() as f64 / 1048576.0,
            st.height,
            st.leaves,
            st.inner,
            st.leaf_bytes as f64 / st.entries as f64,
            st.leaf_bytes as f64 / (st.leaves * PAGE_SIZE as u64) as f64,
            p1.hwm_kib as f64 / 1024.0,
        );
        println!("{line}");
        writeln!(md, "{line}").unwrap();
        assert_eq!(st.entries, n);
        drop(e);
    }
    let path = env_dir("LPG_LAB_OUT").join("t17-lab-bulk.md");
    write_text(&path, &md);
    println!("{md}\nwritten to {}", path.display());
}
