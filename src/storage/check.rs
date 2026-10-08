//! The on-demand integrity check (TS-12), the oracle of the tree, recovery
//! and restore tests.
//!
//! It runs under the write latch, so no batch changes pages meanwhile, and
//! reads every page below the page count once:
//!
//! - **Checksum.** A page the pool holds dirty has no on-disk image yet
//!   (its checksum is set when it is written), so it is checked from its
//!   frame; every other page is read from the data file, bypassing the
//!   pool, and its checksum verified.
//! - **Trees.** From the directory (tree 0) to every tree it lists: page
//!   type, tree identifier, level, leaf encoding, key order within a page,
//!   keys within the bounds the parent's separators give, each high key
//!   equal to the parent's bound, the right links of each level in key
//!   order with the right-most page linking nowhere, and overflow chains.
//!   These are PostgreSQL amcheck's invariants (R-PG
//!   `contrib/amcheck/verify_nbtree.c`: item order, high key, parent key
//!   against child high key, sibling links).
//! - **Membership.** Every page below the page count is in exactly one of:
//!   page 0, a tree, the free list (trunk or listed page). A page reached
//!   twice, or never, is reported, as SQLite's "2nd reference to page" and
//!   "never used" (R-SQLITE `src/btree.c`, `checkRef`,
//!   `sqlite3BtreeIntegrityCheck`). Pages at or above the page count are
//!   the unused reserve and must not be referenced.
//!
//! The cross-tree checks run after this one, as a read-only transaction
//! ([`crate::storage::engine::Engine::check_graphs`]: adjacency against
//! edges; indexes against data arrive with rmp #33). Leaves of either key
//! form ([`super::btree::Keys`]) are decoded by their own flag.

use super::btree::node::{self, NONE, Value};
use super::btree::{DIRECTORY, Store, Tree};
use super::datafile::Header;
use super::error::{Error, Result};
use super::freelist::decode_trunk;
use super::page::{self, PAGE_SIZE, PageType};

/// Problems reported before the check stops listing them.
pub(crate) const MAX_PROBLEMS: usize = 1000;

/// Shape figures of one tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TreeStats {
    /// Tree identifier.
    pub(crate) id: u64,
    /// Levels, leaves included.
    pub(crate) height: u32,
    /// Leaf pages.
    pub(crate) leaves: u64,
    /// Inner pages.
    pub(crate) inner: u64,
    /// Overflow pages.
    pub(crate) overflow: u64,
    /// Entries.
    pub(crate) entries: u64,
    /// Bytes used in leaves: fixed fields, high key, entries and restart
    /// array (M-LAYOUT's "leaf bytes used").
    pub(crate) leaf_bytes: u64,
}

/// The result of an integrity check.
#[derive(Debug, Default)]
pub(crate) struct Report {
    /// Every problem found, at most [`MAX_PROBLEMS`]; empty if the
    /// database is intact.
    pub(crate) problems: Vec<String>,
    /// Figures of each tree, the directory first.
    pub(crate) trees: Vec<TreeStats>,
    /// Pages below the page count.
    pub(crate) page_count: u64,
    /// Free-list trunk pages.
    pub(crate) trunk_pages: u64,
    /// Pages listed as free.
    pub(crate) free_pages: u64,
}

impl Report {
    /// Whether no problem was found.
    pub(crate) fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    None,
    Header,
    Tree(u64),
    Trunk,
    Free,
}

struct Checker<'s, 'e> {
    store: &'s Store<'e>,
    owners: Vec<Owner>,
    report: Report,
    bad_checksum: bool,
    buf: Vec<u8>,
}

/// Per-level state of a tree walk: the last page seen and its right link.
type LevelLast = Vec<Option<(u64, u64)>>;

impl Checker<'_, '_> {
    fn problem(&mut self, msg: String) {
        if self.report.problems.len() < MAX_PROBLEMS {
            self.report.problems.push(msg);
        }
    }

    /// Claims page `p` for `who`. Returns whether the claim is the first.
    fn claim(&mut self, p: u64, who: Owner, from: &str) -> bool {
        let Some(slot) = self.owners.get_mut(p as usize) else {
            self.problem(format!(
                "page {p}, referenced by {from}, is at or above the page count {}",
                self.report.page_count
            ));
            return false;
        };
        if *slot != Owner::None {
            let was = *slot;
            self.problem(format!(
                "page {p} is in two places: {was:?} and {who:?} (referenced by {from})"
            ));
            return false;
        }
        *slot = who;
        true
    }

    /// Loads page `p` into `self.buf`: from its frame if the pool holds it
    /// dirty, else from the data file with its checksum verified. Returns
    /// `false` after reporting a checksum failure.
    fn load(&mut self, p: u64) -> Result<bool> {
        if let Some((pin, true)) = self.store.pool.resident(p) {
            let g = pin.read()?;
            self.buf.copy_from_slice(&g);
            return Ok(true);
        }
        if self.store.data.read_page_unverified(p, &mut self.buf)? {
            Ok(true)
        } else {
            self.bad_checksum = true;
            self.problem(format!("page {p} fails its checksum"));
            Ok(false)
        }
    }

    fn check_header(&mut self, free_head: u64, next_tree_id: u64) -> Result<Option<Header>> {
        self.claim(0, Owner::Header, "the file");
        if !self.load(0)? {
            return Ok(None);
        }
        match Header::decode(&self.buf) {
            Err(e) => {
                self.problem(format!("page 0: {e}"));
                Ok(None)
            }
            Ok(h) => {
                let mem = (free_head, next_tree_id, self.report.page_count);
                let disk = (h.free_list_head, h.next_tree_id, h.page_count);
                if mem != disk {
                    self.problem(format!(
                        "page 0 holds (free-list head, next tree id, page count) = {disk:?}, \
                         the open database {mem:?}"
                    ));
                }
                Ok(Some(h))
            }
        }
    }

    fn check_free_list(
        &mut self,
        head: u64,
        expected_pages: &[u64],
        expected_trunks: &[u64],
    ) -> Result<()> {
        let mut trunk = head;
        let mut pages = Vec::new();
        let mut trunks = Vec::new();
        let mut first_trunk = true;
        while trunk != NONE {
            if !self.claim(trunk, Owner::Trunk, "the free-list chain") {
                break;
            }
            trunks.push(trunk);
            if !self.load(trunk)? {
                break;
            }
            match decode_trunk(&self.buf) {
                Err(e) => {
                    self.problem(format!("free-list trunk {trunk}: {e}"));
                    break;
                }
                Ok(t) => {
                    if !first_trunk && t.first != 0 {
                        self.problem(format!(
                            "free-list trunk {trunk} is not first but has taken entries"
                        ));
                    }
                    for &p in &t.pages {
                        self.claim(p, Owner::Free, &format!("free-list trunk {trunk}"));
                    }
                    pages.extend(t.pages);
                    trunk = t.next;
                }
            }
            first_trunk = false;
        }
        self.report.trunk_pages = trunks.len() as u64;
        self.report.free_pages = pages.len() as u64;
        if pages != expected_pages || trunks != expected_trunks {
            self.problem("the persisted free list differs from the in-memory free-page set".into());
        }
        Ok(())
    }

    /// Checks `tree` and returns the entries of its leaves when `collect`
    /// (the directory).
    fn check_tree(&mut self, tree: Tree, collect: bool) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut stats = TreeStats {
            id: tree.id(),
            ..TreeStats::default()
        };
        let mut out = Vec::new();
        if tree.root() == NONE || tree.root() >= self.report.page_count {
            self.problem(format!(
                "tree {}: root {} outside the page count",
                tree.id(),
                tree.root()
            ));
            self.report.trees.push(stats);
            return Ok(out);
        }
        // The root's level gives the height; `visit` claims and checks it.
        if !self.load(tree.root())? {
            self.report.trees.push(stats);
            return Ok(out);
        }
        let height = u32::from(node::level(&self.buf)) + 1;
        stats.height = height;
        let mut last: LevelLast = vec![None; height as usize];
        let from = format!("the directory entry of tree {}", tree.id());
        self.visit(
            tree,
            tree.root(),
            height - 1,
            None,
            None,
            &mut last,
            &mut stats,
            collect.then_some(&mut out),
            &from,
        )?;
        for (level, l) in last.iter().enumerate() {
            if let Some((p, right)) = *l
                && right != NONE
            {
                self.problem(format!(
                    "tree {}: page {p}, last of level {level}, links right to {right}",
                    tree.id()
                ));
            }
        }
        self.report.trees.push(stats);
        Ok(out)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the walk state of one page: bounds, level states, figures, output"
    )]
    fn visit(
        &mut self,
        tree: Tree,
        p: u64,
        level: u32,
        low: Option<&[u8]>,
        high: Option<&[u8]>,
        last: &mut LevelLast,
        stats: &mut TreeStats,
        mut collect: Option<&mut Vec<(Vec<u8>, Vec<u8>)>>,
        from: &str,
    ) -> Result<()> {
        let id = tree.id();
        if !self.claim(p, Owner::Tree(id), from) || !self.load(p)? {
            return Ok(());
        }
        let page = std::mem::replace(&mut self.buf, vec![0u8; PAGE_SIZE]);
        let here = format!("tree {id} page {p}");
        // Sibling link of the previous page at this level.
        if let Some((prev, right)) = last[level as usize]
            && right != p
        {
            self.problem(format!(
                "tree {id}: page {prev} links right to {right}, but the next page of level {level} is {p}"
            ));
        }
        last[level as usize] = Some((p, node::right(&page)));
        let kind = page::page_type(&page);
        let expected = if level == 0 {
            PageType::Leaf
        } else {
            PageType::Inner
        };
        if kind != Some(expected) {
            self.problem(format!("{here}: type {kind:?}, expected {expected:?}"));
            return Ok(());
        }
        if page::tree(&page) != id
            || u32::from(node::level(&page)) != level
            || node::is_deleted(&page)
        {
            self.problem(format!(
                "{here}: tree id {}, level {}, deleted {}; expected tree {id}, level {level}, live",
                page::tree(&page),
                node::level(&page),
                node::is_deleted(&page)
            ));
            return Ok(());
        }
        if node::body_start(&page).is_none() {
            self.problem(format!("{here}: malformed high-key length"));
            return Ok(());
        }
        if node::high_key(&page) != high {
            self.problem(format!("{here}: high key differs from the parent's bound"));
        }
        // Keys of the page, in stored order.
        let mut keys: Vec<Vec<u8>> = Vec::new();
        if level == 0 {
            let mut chains = Vec::new();
            let mut collected = Vec::new();
            match node::leaf_entries(&page, |_, e| {
                keys.push(e.key.to_vec());
                match e.value {
                    Value::Overflow { len, first } => chains.push((len, first)),
                    Value::Inline(v) => {
                        if collect.is_some() {
                            collected.push((e.key.to_vec(), v.to_vec()));
                        }
                    }
                }
            }) {
                Err(e) => {
                    self.problem(format!("{here}: {e}"));
                    return Ok(());
                }
                Ok(v) => {
                    stats.leaves += 1;
                    stats.entries += v.count() as u64;
                    stats.leaf_bytes += (node::BODY
                        + node::high_key(&page).map_or(0, <[u8]>::len)
                        + v.used_bytes()) as u64;
                }
            }
            if let Some(c) = collect.as_deref_mut() {
                c.extend(collected);
            }
            for (len, first) in chains {
                self.check_overflow(id, len, first, &here)?;
                stats.overflow += len.div_ceil(node::OVERFLOW_CAP as u64);
            }
            self.check_keys(&here, &keys, low, high);
            return Ok(());
        }
        let mut children = vec![node::leftmost(&page)];
        if let Err(e) = node::inner_entries(&page, |k, c| {
            keys.push(k.to_vec());
            children.push(c);
        }) {
            self.problem(format!("{here}: {e}"));
            return Ok(());
        }
        stats.inner += 1;
        self.check_keys(&here, &keys, low, high);
        for (i, &c) in children.iter().enumerate() {
            let lo = if i == 0 {
                low
            } else {
                Some(keys[i - 1].as_slice())
            };
            let hi = keys.get(i).map(Vec::as_slice).or(high);
            self.visit(
                tree,
                c,
                level - 1,
                lo,
                hi,
                last,
                stats,
                collect.as_deref_mut(),
                &here,
            )?;
        }
        Ok(())
    }

    /// Keys strictly ascending, within `[low, high)`.
    fn check_keys(
        &mut self,
        here: &str,
        keys: &[Vec<u8>],
        low: Option<&[u8]>,
        high: Option<&[u8]>,
    ) {
        for (i, w) in keys.windows(2).enumerate() {
            if w[0] >= w[1] {
                self.problem(format!("{here}: keys {i} and {} out of order", i + 1));
            }
        }
        if let (Some(lo), Some(first)) = (low, keys.first())
            && first.as_slice() < lo
        {
            self.problem(format!("{here}: first key below the parent's separator"));
        }
        if let (Some(hi), Some(last)) = (high, keys.last())
            && last.as_slice() >= hi
        {
            self.problem(format!("{here}: last key at or above the high key"));
        }
    }

    fn check_overflow(&mut self, id: u64, len: u64, first: u64, from: &str) -> Result<()> {
        let mut p = first;
        let mut got = 0u64;
        while p != NONE && got < len {
            if !self.claim(p, Owner::Tree(id), from) || !self.load(p)? {
                return Ok(());
            }
            if page::page_type(&self.buf) != Some(PageType::Overflow) || page::tree(&self.buf) != id
            {
                self.problem(format!(
                    "{from}: page {p} is not an overflow page of tree {id}"
                ));
                return Ok(());
            }
            let Some((next, bytes)) = node::overflow(&self.buf) else {
                self.problem(format!("{from}: overflow page {p} is malformed"));
                return Ok(());
            };
            got += bytes.len() as u64;
            p = next;
        }
        if got != len || p != NONE {
            self.problem(format!(
                "{from}: overflow chain at {first} holds {got} bytes, value has {len}"
            ));
        }
        Ok(())
    }
}

impl Store<'_> {
    /// Checks the whole database (TS-12) and reports every problem found.
    /// A checksum failure also sets the unusable latch, as on any read.
    ///
    /// # Errors
    ///
    /// I/O errors; [`Error::Unusable`] if the latch was already set.
    pub(crate) fn check(&self) -> Result<Report> {
        self.health.check()?;
        let st = self.lock();
        let page_count = self.data.page_count();
        let mut c = Checker {
            store: self,
            owners: vec![Owner::None; page_count as usize],
            report: Report {
                page_count,
                ..Report::default()
            },
            bad_checksum: false,
            buf: vec![0u8; PAGE_SIZE],
        };
        let free = st.free_list();
        let expected_pages: Vec<u64> = free.pages().collect();
        let expected_trunks: Vec<u64> = free.trunk_pages().collect();
        if let Some(h) = c.check_header(free.head(), st.next_tree_id())? {
            c.check_free_list(h.free_list_head, &expected_pages, &expected_trunks)?;
        }
        let entries = c.check_tree(DIRECTORY, true)?;
        for (k, v) in entries {
            let (Ok(id), Some((root, _))) = (
                <[u8; 8]>::try_from(k.as_slice()),
                super::btree::decode_directory_entry(&v),
            ) else {
                c.problem("malformed directory entry".into());
                continue;
            };
            let id = u64::from_be_bytes(id);
            if id == 0 || id >= st.next_tree_id() {
                c.problem(format!(
                    "directory lists tree {id}, not below the next tree id"
                ));
            }
            c.check_tree(Tree::new(id, root), false)?;
        }
        for p in 0..page_count {
            if c.owners[p as usize] == Owner::None {
                c.problem(format!("page {p} is in no tree and not free"));
            }
        }
        drop(st);
        if c.bad_checksum {
            self.health.fail(Error::Corruption(
                "integrity check found a checksum failure".into(),
            ));
        }
        Ok(c.report)
    }
}

#[cfg(test)]
mod tests;
