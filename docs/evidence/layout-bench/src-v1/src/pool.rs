//! Buffer pool over fixed 8 KiB pages backed by one file opened with O_DIRECT.
//!
//! Every page access goes through `read` / `write`, which (1) looks the page up
//! in the page table, (2) on a miss evicts a frame by clock sweep (usage count
//! capped at 5) and reads the page with `pread` (`FileExt::read_exact_at`),
//! and (3) pins the frame with an atomic increment/decrement around the access,
//! which stands in for the latch acquire/release a concurrent pool would pay.
//! The pool is single-threaded; that is a stated limitation of the benchmark.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

pub const PAGE: usize = 8192;
pub const NONE: u32 = u32::MAX;
const CHUNK: usize = 256; // frames per allocation chunk (2 MiB)
const ALIGN: usize = 4096; // O_DIRECT buffer alignment

struct Chunk {
    buf: Vec<u8>,
    base: usize,
}
impl Chunk {
    fn new() -> Chunk {
        let buf = vec![0u8; CHUNK * PAGE + ALIGN];
        // Reading an address as an integer is safe; the slice starting at
        // `base` is 4096-aligned, which O_DIRECT requires.
        let a = buf.as_ptr() as usize;
        let base = (ALIGN - a % ALIGN) % ALIGN;
        Chunk { buf, base }
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    /// Logical page accesses.
    pub fetches: u64,
    /// Distinct pages accessed within each operation, summed over operations.
    pub distinct: u64,
    /// Physical page reads (cache misses).
    pub misses: u64,
    /// Distinct pages modified within each operation, summed over operations.
    pub dirtied: u64,
    /// Dirty pages written back on eviction.
    pub writebacks: u64,
    /// Nanoseconds spent inside physical reads.
    pub read_ns: u64,
}
impl Stats {
    pub fn since(&self, b: &Stats) -> Stats {
        Stats {
            fetches: self.fetches - b.fetches,
            distinct: self.distinct - b.distinct,
            misses: self.misses - b.misses,
            dirtied: self.dirtied - b.dirtied,
            writebacks: self.writebacks - b.writebacks,
            read_ns: self.read_ns - b.read_ns,
        }
    }
}

pub struct Pool {
    file: File,
    /// Same file without O_DIRECT: misses served by the OS page cache when `direct` is false.
    bfile: File,
    direct: bool,
    chunks: Vec<Chunk>,
    cap: usize,
    nframes: usize,
    frame_pid: Vec<u32>,
    usage: Vec<u8>,
    dirty: Vec<bool>,
    pins: Vec<AtomicU32>,
    map: Vec<u32>,
    npages: u32,
    hand: usize,
    touch: Vec<u32>,
    dtouch: Vec<u32>,
    epoch: u32,
    pub st: Stats,
}

impl Pool {
    pub fn create(path: &Path) -> Pool {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_DIRECT)
            .open(path)
            .expect("open page file with O_DIRECT");
        let bfile = OpenOptions::new().read(true).open(path).expect("open buffered handle");
        Pool {
            file,
            bfile,
            direct: true,
            chunks: Vec::new(),
            cap: usize::MAX,
            nframes: 0,
            frame_pid: Vec::new(),
            usage: Vec::new(),
            dirty: Vec::new(),
            pins: Vec::new(),
            map: Vec::new(),
            npages: 0,
            hand: 0,
            touch: Vec::new(),
            dtouch: Vec::new(),
            epoch: 1,
            st: Stats::default(),
        }
    }

    pub fn npages(&self) -> u32 {
        self.npages
    }

    /// Starts a new operation for the distinct-page counters.
    #[inline]
    pub fn begin_op(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    #[inline(always)]
    fn off(&self, f: usize) -> (usize, usize) {
        let c = f / CHUNK;
        (c, self.chunks[c].base + (f % CHUNK) * PAGE)
    }

    #[inline(always)]
    fn fetch(&mut self, pid: u32) -> usize {
        let p = pid as usize;
        self.st.fetches += 1;
        if self.touch[p] != self.epoch {
            self.touch[p] = self.epoch;
            self.st.distinct += 1;
        }
        let f = self.map[p];
        if f != NONE {
            let f = f as usize;
            let u = &mut self.usage[f];
            if *u < 5 {
                *u += 1;
            }
            return f;
        }
        self.load(pid)
    }

    #[cold]
    #[inline(never)]
    fn load(&mut self, pid: u32) -> usize {
        self.st.misses += 1;
        let f = self.victim();
        let (c, o) = self.off(f);
        let t = Instant::now();
        let src = if self.direct { &self.file } else { &self.bfile };
        src
            .read_exact_at(&mut self.chunks[c].buf[o..o + PAGE], pid as u64 * PAGE as u64)
            .expect("pread page");
        self.st.read_ns += t.elapsed().as_nanos() as u64;
        self.frame_pid[f] = pid;
        self.map[pid as usize] = f as u32;
        self.usage[f] = 1;
        f
    }

    fn writeback(&mut self, f: usize) {
        let pid = self.frame_pid[f];
        let (c, o) = self.off(f);
        self.file
            .write_all_at(&self.chunks[c].buf[o..o + PAGE], pid as u64 * PAGE as u64)
            .expect("pwrite page");
        self.dirty[f] = false;
        self.st.writebacks += 1;
    }

    fn victim(&mut self) -> usize {
        if self.nframes < self.cap {
            let f = self.nframes;
            self.nframes += 1;
            if f / CHUNK >= self.chunks.len() {
                self.chunks.push(Chunk::new());
            }
            if f >= self.frame_pid.len() {
                self.frame_pid.push(NONE);
                self.usage.push(0);
                self.dirty.push(false);
                self.pins.push(AtomicU32::new(0));
            }
            return f;
        }
        loop {
            let f = self.hand;
            self.hand += 1;
            if self.hand == self.nframes {
                self.hand = 0;
            }
            if self.pins[f].load(Ordering::Relaxed) != 0 {
                continue;
            }
            if self.usage[f] > 0 {
                self.usage[f] -= 1;
                continue;
            }
            let old = self.frame_pid[f];
            if old != NONE {
                if self.dirty[f] {
                    self.writeback(f);
                }
                self.map[old as usize] = NONE;
                self.frame_pid[f] = NONE;
            }
            return f;
        }
    }

    #[inline(always)]
    pub fn read<R>(&mut self, pid: u32, f: impl FnOnce(&[u8]) -> R) -> R {
        let fr = self.fetch(pid);
        self.pins[fr].fetch_add(1, Ordering::Acquire);
        let (c, o) = self.off(fr);
        let r = f(&self.chunks[c].buf[o..o + PAGE]);
        self.pins[fr].fetch_sub(1, Ordering::Release);
        r
    }

    #[inline(always)]
    pub fn write<R>(&mut self, pid: u32, f: impl FnOnce(&mut [u8]) -> R) -> R {
        let fr = self.fetch(pid);
        let p = pid as usize;
        if self.dtouch[p] != self.epoch {
            self.dtouch[p] = self.epoch;
            self.st.dirtied += 1;
        }
        self.dirty[fr] = true;
        self.pins[fr].fetch_add(1, Ordering::Acquire);
        let (c, o) = self.off(fr);
        let r = f(&mut self.chunks[c].buf[o..o + PAGE]);
        self.pins[fr].fetch_sub(1, Ordering::Release);
        r
    }

    /// Allocates a new zeroed page, resident and dirty.
    pub fn alloc(&mut self) -> u32 {
        let pid = self.npages;
        self.npages += 1;
        self.map.push(NONE);
        self.touch.push(self.epoch);
        self.dtouch.push(self.epoch);
        self.st.distinct += 1;
        self.st.dirtied += 1;
        let f = self.victim();
        let (c, o) = self.off(f);
        self.chunks[c].buf[o..o + PAGE].fill(0);
        self.frame_pid[f] = pid;
        self.map[pid as usize] = f as u32;
        self.usage[f] = 1;
        self.dirty[f] = true;
        pid
    }

    pub fn flush_all(&mut self) {
        for f in 0..self.nframes {
            if self.dirty[f] && self.frame_pid[f] != NONE {
                self.writeback(f);
            }
        }
    }

    /// Writes back all dirty pages, empties the pool and sets a new frame budget.
    pub fn set_capacity(&mut self, cap: usize) {
        self.flush_all();
        for f in 0..self.nframes {
            let p = self.frame_pid[f];
            if p != NONE {
                self.map[p as usize] = NONE;
            }
            self.frame_pid[f] = NONE;
            self.usage[f] = 0;
        }
        self.nframes = 0;
        self.hand = 0;
        self.cap = cap.max(8);
        let keep = self.cap.div_ceil(CHUNK).min(self.chunks.len());
        self.chunks.truncate(keep);
        self.chunks.shrink_to_fit();
    }

    /// Chooses O_DIRECT (device) or buffered (OS page cache) reads for misses.
    /// Buffered mode first reads the whole file once so the OS cache holds it.
    pub fn set_direct(&mut self, d: bool) {
        self.direct = d;
        if !d {
            let mut buf = vec![0u8; 1 << 20];
            let len = self.npages as u64 * PAGE as u64;
            let mut off = 0u64;
            while off < len {
                let k = ((len - off) as usize).min(buf.len());
                self.bfile.read_exact_at(&mut buf[..k], off).expect("warm OS cache");
                off += k as u64;
            }
        }
    }

    /// Reads every page into the pool (used to return to the all-in-memory state).
    pub fn preload(&mut self) {
        for p in 0..self.npages {
            self.read(p, |_| ());
        }
    }
}
