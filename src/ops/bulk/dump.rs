//! The native dump (TS-26 Export layouts, native dump; rmp #57).
//!
//! **Layout.** All integers little-endian.
//!
//! | Part | Bytes |
//! |---|---|
//! | header | magic `LPGDUMP\0` (8), dump format version (4), scope (1: 0 database, 1 graph), crate format version (4, TS-28), CRC-32C of the 17 bytes before it (4) |
//! | block | kind (1), payload length (4), payload, CRC-32C of kind, length and payload (4) |
//! | trailer | a block of kind 0xFF: blocks of each kind (4 × 8), nodes (8), edges (8) |
//!
//! Blocks come in this order: one catalog block (tokens, and for the
//! database scope the catalog objects), node blocks, edge blocks, one
//! definitions block (index and constraint definitions), then the
//! trailer; nothing follows it. A node or edge block starts with the
//! dump's identifier of its graph (varint; 0 in a graph-scope dump) and
//! holds records of one graph, each the element's identifier and, for an
//! edge, its source, target and type token, then the TS-21 record
//! encoding of its labels and properties (`src/types/record.rs`), with the
//! dump's own token identifiers, which the catalog block names.
//!
//! A dump that fails a length, order or CRC-32C check fails the import
//! whatever the invalid-record option ([`verify`] runs before any write;
//! rmp #55 DECISION 412). The checks follow the backup `MANIFEST` (own
//! code, `src/ops/backup.rs:132-160`); the order follows Turso's `.dump`,
//! which writes each table's definition and rows, then the indexes and the
//! other schema objects (R-TURSO @ `be5bab8` `cli/app.rs:1847-1874`, MIT,
//! studied only).

use std::collections::HashMap;
use std::io::{self, Write};

use super::invalid_input;
use super::model::{EdgeIn, Invalid, NodeIn, Pos, Sink, Source};
use crate::catalog::{Kind, TokenKind};
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::index::{SchemaDef, schema_key};
use crate::storage::StorageFile;
use crate::storage::btree::node::put_varint;
use crate::storage::engine::FORMAT_VERSION;
use crate::sync::Arc;

/// The magic number of a dump.
pub(crate) const MAGIC: [u8; 8] = *b"LPGDUMP\0";
/// The dump format version.
pub(crate) const VERSION: u32 = 1;
/// Bytes of the header.
pub(crate) const HEADER: usize = 21;

const CATALOG: u8 = 1;
const NODES: u8 = 2;
const EDGES: u8 = 3;
const DEFINITIONS: u8 = 4;
const TRAILER: u8 = 0xFF;

/// The payload size at which a node or edge block is closed.
const BLOCK_TARGET: usize = 64 << 10;

/// What a dump covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Database,
    Graph,
}

/// A catalog object of a database dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DumpObject {
    pub(crate) id: u64,
    pub(crate) parent: u64,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    /// A graph's graph type (0 for none).
    pub(crate) graph_type: u64,
    /// A graph type's definition bytes.
    pub(crate) definition: Vec<u8>,
}

/// The catalog block: tokens and objects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DumpCatalog {
    pub(crate) tokens: Vec<(u64, TokenKind, String)>,
    pub(crate) objects: Vec<DumpObject>,
}

fn kind_byte(k: Kind) -> u8 {
    k as u8
}

fn kind_of(b: u8) -> Option<Kind> {
    Some(match b {
        1 => Kind::Directory,
        2 => Kind::Schema,
        3 => Kind::GraphType,
        4 => Kind::Graph,
        _ => return None,
    })
}

fn token_kind_of(b: u8) -> Option<TokenKind> {
    Some(match b {
        0 => TokenKind::Label,
        1 => TokenKind::EdgeType,
        2 => TokenKind::PropertyKey,
        _ => return None,
    })
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_varint(out, b.len() as u64);
    out.extend_from_slice(b);
}

impl DumpCatalog {
    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        put_varint(&mut b, self.tokens.len() as u64);
        for (id, kind, name) in &self.tokens {
            put_varint(&mut b, *id);
            b.push(*kind as u8);
            put_bytes(&mut b, name.as_bytes());
        }
        put_varint(&mut b, self.objects.len() as u64);
        for o in &self.objects {
            put_varint(&mut b, o.id);
            put_varint(&mut b, o.parent);
            b.push(kind_byte(o.kind));
            put_bytes(&mut b, o.name.as_bytes());
            put_varint(&mut b, o.graph_type);
            put_bytes(&mut b, &o.definition);
        }
        b
    }

    fn decode(b: &[u8]) -> Option<DumpCatalog> {
        let mut r = Cursor { b, at: 0 };
        let mut c = DumpCatalog::default();
        for _ in 0..r.varint()? {
            let id = r.varint()?;
            let kind = token_kind_of(r.byte()?)?;
            c.tokens.push((id, kind, r.string()?));
        }
        for _ in 0..r.varint()? {
            c.objects.push(DumpObject {
                id: r.varint()?,
                parent: r.varint()?,
                kind: kind_of(r.byte()?)?,
                name: r.string()?,
                graph_type: r.varint()?,
                definition: r.bytes()?.to_vec(),
            });
        }
        r.done().then_some(c)
    }
}

/// A reader of the varint fields of a payload.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= u64::from(b & 0x7F) << shift;
            if b < 0x80 {
                return Some(v);
            }
        }
        None
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.varint()?).ok()?;
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn string(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?.to_vec()).ok()
    }

    fn done(&self) -> bool {
        self.at == self.b.len()
    }
}

/// Writes a dump in the layout of the module documentation.
pub(crate) struct DumpWriter<'w> {
    out: &'w mut dyn Write,
    /// Blocks of each kind: catalog, nodes, edges, definitions.
    blocks: [u64; 4],
    nodes: u64,
    edges: u64,
    /// The open node or edge block: its kind, graph and payload.
    open: Option<(u8, u64)>,
    payload: Vec<u8>,
}

impl<'w> DumpWriter<'w> {
    /// Writes the header and the catalog block.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub(crate) fn new(
        out: &'w mut dyn Write,
        scope: Scope,
        catalog: &DumpCatalog,
    ) -> io::Result<DumpWriter<'w>> {
        let mut h = Vec::with_capacity(HEADER);
        h.extend_from_slice(&MAGIC);
        h.extend_from_slice(&VERSION.to_le_bytes());
        h.push(match scope {
            Scope::Database => 0,
            Scope::Graph => 1,
        });
        h.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        let crc = crc32c::crc32c(&h);
        h.extend_from_slice(&crc.to_le_bytes());
        out.write_all(&h)?;
        let mut w = DumpWriter {
            out,
            blocks: [0; 4],
            nodes: 0,
            edges: 0,
            open: None,
            payload: Vec::new(),
        };
        w.block(CATALOG, &catalog.encode())?;
        Ok(w)
    }

    fn block(&mut self, kind: u8, payload: &[u8]) -> io::Result<()> {
        let len = u32::try_from(payload.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a dump block over 4 GiB"))?;
        let mut head = [0u8; 5];
        head[0] = kind;
        head[1..].copy_from_slice(&len.to_le_bytes());
        let crc = crc32c::crc32c_append(crc32c::crc32c(&head), payload);
        self.out.write_all(&head)?;
        self.out.write_all(payload)?;
        self.out.write_all(&crc.to_le_bytes())?;
        if (1..=4).contains(&kind) {
            self.blocks[usize::from(kind - 1)] += 1;
        }
        Ok(())
    }

    /// Closes the open node or edge block.
    fn close(&mut self) -> io::Result<()> {
        if let Some((kind, _)) = self.open.take() {
            let p = std::mem::take(&mut self.payload);
            self.block(kind, &p)?;
        }
        Ok(())
    }

    /// Opens a block of `kind` for `graph` unless one is open with room.
    fn room(&mut self, kind: u8, graph: u64) -> io::Result<()> {
        if self.open != Some((kind, graph)) || self.payload.len() >= BLOCK_TARGET {
            self.close()?;
            self.open = Some((kind, graph));
            put_varint(&mut self.payload, graph);
        }
        Ok(())
    }

    /// Adds node `id` of `graph` with its TS-21 record bytes. Every node
    /// comes before every edge.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub(crate) fn node(&mut self, graph: u64, id: u64, record: &[u8]) -> io::Result<()> {
        self.room(NODES, graph)?;
        put_varint(&mut self.payload, id);
        put_bytes(&mut self.payload, record);
        self.nodes += 1;
        Ok(())
    }

    /// Adds edge `id` of `graph` from `src` to `dst` of type token `ty`,
    /// with the TS-21 record bytes of its properties.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub(crate) fn edge(
        &mut self,
        graph: u64,
        id: u64,
        (src, dst, ty): (u64, u64, u64),
        record: &[u8],
    ) -> io::Result<()> {
        self.room(EDGES, graph)?;
        for v in [id, src, dst, ty] {
            put_varint(&mut self.payload, v);
        }
        put_bytes(&mut self.payload, record);
        self.edges += 1;
        Ok(())
    }

    /// Writes the definitions block and the trailer.
    ///
    /// # Errors
    ///
    /// I/O errors.
    pub(crate) fn finish(mut self, defs: &[SchemaDef]) -> io::Result<()> {
        self.close()?;
        let mut p = Vec::new();
        put_varint(&mut p, defs.len() as u64);
        for d in defs {
            put_varint(&mut p, d.graph);
            put_varint(&mut p, d.id);
            put_bytes(&mut p, &d.encode());
        }
        self.block(DEFINITIONS, &p)?;
        let mut t = Vec::with_capacity(48);
        for n in self.blocks {
            t.extend_from_slice(&n.to_le_bytes());
        }
        t.extend_from_slice(&self.nodes.to_le_bytes());
        t.extend_from_slice(&self.edges.to_le_bytes());
        self.block(TRAILER, &t)
    }
}

/// A damaged dump: 22N05, whatever the invalid-record option (TS-26).
fn damaged(what: &str) -> Error {
    invalid_input(&format!("a damaged native dump: {what}"))
}

/// One block of a dump, read and checked.
pub(crate) struct Block {
    pub(crate) kind: u8,
    pub(crate) payload: Vec<u8>,
    /// Its ordinal in the dump, from 1.
    pub(crate) ordinal: u64,
    /// The payload's charge to the memory cap, released with the block.
    _charge: Option<crate::storage::memory::Reservation>,
}

/// A dump being read: its scope, catalog and definitions, read and
/// checked by [`verify`].
pub(crate) struct Dump<'f> {
    file: &'f dyn StorageFile,
    len: u64,
    pub(crate) scope: Scope,
    pub(crate) catalog: DumpCatalog,
    pub(crate) definitions: Vec<SchemaDef>,
    /// The token names, by the dump's identifiers.
    names: HashMap<u64, (TokenKind, String)>,
    /// The file name, for the report.
    name: Arc<str>,
    /// The pool the payloads are charged to; `None` for the fuzz target.
    pool: Option<&'f crate::storage::pool::Pool>,
    /// The charge of the catalog and the token names kept.
    _charge: Option<crate::storage::memory::Reservation>,
}

fn read_exact(file: &dyn StorageFile, buf: &mut [u8], at: u64) -> Result<(), Error> {
    let n = file.read_at(buf, at).map_err(super::files::io_error)?;
    if n < buf.len() {
        return Err(damaged("truncated"));
    }
    Ok(())
}

/// Reads the block at `*at` of a dump of `len` bytes and advances `*at`.
fn read_block(
    file: &dyn StorageFile,
    len: u64,
    at: &mut u64,
    ordinal: u64,
    pool: Option<&crate::storage::pool::Pool>,
) -> Result<Block, Error> {
    let mut head = [0u8; 5];
    if len.saturating_sub(*at) < 9 {
        return Err(damaged("truncated before a block"));
    }
    read_exact(file, &mut head, *at)?;
    let n = u64::from(u32::from_le_bytes(head[1..].try_into().expect("4 bytes")));
    // The length is checked against the file before any buffer is made,
    // so a damaged length cannot ask for more memory than the file holds.
    if n > len - *at - 9 {
        return Err(damaged("a block longer than the file"));
    }
    // The payload is charged to the memory cap before it is made (rmp #57
    // FINDING 458): a block holds a whole record, which can be large, so
    // no small fixed maximum serves; the cap bounds it instead (51N72).
    let charge = match pool {
        Some(p) => Some(p.reserve(n)?),
        None => None,
    };
    let mut payload = vec![0u8; n as usize];
    read_exact(file, &mut payload, *at + 5)?;
    let mut crc = [0u8; 4];
    read_exact(file, &mut crc, *at + 5 + n)?;
    let want = crc32c::crc32c_append(crc32c::crc32c(&head), &payload);
    if u32::from_le_bytes(crc) != want {
        return Err(damaged("a block fails its CRC-32C"));
    }
    *at += 9 + n;
    Ok(Block {
        kind: head[0],
        payload,
        ordinal,
        _charge: charge,
    })
}

/// Reads and checks the whole dump in `file` (`name` for the report):
/// header, every block's length and CRC-32C, the block order, the trailer
/// and its counts, and that nothing follows it. Returns the dump with its
/// catalog and definitions. Nothing is written before this succeeds, so a
/// damaged dump adds nothing (rmp #57 acceptance).
///
/// # Errors
///
/// 22N05 for a damaged dump or one of another dump or crate format
/// version; 50N00 for an I/O failure.
pub(crate) fn verify<'f>(
    file: &'f dyn StorageFile,
    name: &str,
    pool: Option<&'f crate::storage::pool::Pool>,
) -> Result<Dump<'f>, Error> {
    let len = file.len().map_err(super::files::io_error)?;
    if len < HEADER as u64 {
        return Err(damaged("truncated header"));
    }
    let mut h = [0u8; HEADER];
    read_exact(file, &mut h, 0)?;
    let crc = u32::from_le_bytes(h[17..21].try_into().expect("4 bytes"));
    if h[..8] != MAGIC || crc != crc32c::crc32c(&h[..17]) {
        return Err(damaged("not a native dump, or a damaged header"));
    }
    let version = u32::from_le_bytes(h[8..12].try_into().expect("4 bytes"));
    let format = u32::from_le_bytes(h[13..17].try_into().expect("4 bytes"));
    if version != VERSION || format != FORMAT_VERSION {
        return Err(damaged("another dump or crate format version"));
    }
    let scope = match h[12] {
        0 => Scope::Database,
        1 => Scope::Graph,
        _ => return Err(damaged("an unknown scope")),
    };
    let mut at = HEADER as u64;
    let mut ordinal = 0u64;
    let mut counts = [0u64; 4];
    let (mut nodes, mut edges) = (0u64, 0u64);
    let mut stage = 0u8; // the highest block kind read so far
    let mut catalog = None;
    let mut catalog_charge = None;
    let mut graphs: Vec<u64> = Vec::new();
    let mut definitions = None;
    loop {
        ordinal += 1;
        let b = read_block(file, len, &mut at, ordinal, pool)?;
        match b.kind {
            CATALOG if stage == 0 => {
                // The catalog and the name map built from it stay while
                // the dump is read: charged at twice the payload.
                catalog_charge = match pool {
                    Some(p) => Some(p.reserve(2 * b.payload.len() as u64)?),
                    None => None,
                };
                let c: DumpCatalog =
                    DumpCatalog::decode(&b.payload).ok_or_else(|| damaged("catalog"))?;
                graphs = match scope {
                    Scope::Graph => vec![0],
                    Scope::Database => c
                        .objects
                        .iter()
                        .filter(|o| o.kind == Kind::Graph)
                        .map(|o| o.id)
                        .collect(),
                };
                catalog = Some(c);
            }
            NODES | EDGES if stage >= CATALOG && stage <= b.kind => {
                let mut c = Cursor {
                    b: &b.payload,
                    at: 0,
                };
                // A block of a graph outside the scope would never be read
                // (rmp #57 FINDING 459).
                let g = c.varint().ok_or_else(|| damaged("block graph"))?;
                if !graphs.contains(&g) {
                    return Err(damaged("a block of a graph outside the dump's scope"));
                }
                let n = count_records(&mut c, b.kind).ok_or_else(|| damaged("block records"))?;
                if b.kind == NODES {
                    nodes += n;
                } else {
                    edges += n;
                }
            }
            DEFINITIONS if (CATALOG..=EDGES).contains(&stage) => {
                definitions =
                    Some(decode_definitions(&b.payload).ok_or_else(|| damaged("definitions"))?);
            }
            TRAILER if stage == DEFINITIONS => {
                let t = &b.payload;
                if t.len() != 48 {
                    return Err(damaged("trailer"));
                }
                let u = |i: usize| u64::from_le_bytes(t[i * 8..i * 8 + 8].try_into().expect("8"));
                if (0..4).any(|i| u(i) != counts[i]) || u(4) != nodes || u(5) != edges {
                    return Err(damaged("the trailer's counts differ from the blocks"));
                }
                if at != len {
                    return Err(damaged("bytes after the trailer"));
                }
                break;
            }
            _ => return Err(damaged("a block out of order")),
        }
        counts[usize::from(b.kind - 1)] += 1;
        stage = b.kind;
    }
    let catalog = catalog.expect("checked by the order");
    let names = catalog
        .tokens
        .iter()
        .map(|(id, k, n)| (*id, (*k, n.clone())))
        .collect();
    Ok(Dump {
        file,
        len,
        scope,
        catalog,
        definitions: definitions.expect("checked by the order"),
        names,
        name: Arc::from(name),
        pool,
        _charge: catalog_charge,
    })
}

/// Counts the records of a node or edge payload after its graph field;
/// `None` if one is malformed.
fn count_records(c: &mut Cursor<'_>, kind: u8) -> Option<u64> {
    let mut n = 0;
    while !c.done() {
        let fields = if kind == NODES { 1 } else { 4 };
        for _ in 0..fields {
            c.varint()?;
        }
        c.bytes()?;
        n += 1;
    }
    Some(n)
}

fn decode_definitions(b: &[u8]) -> Option<Vec<SchemaDef>> {
    let mut c = Cursor { b, at: 0 };
    let mut out = Vec::new();
    for _ in 0..c.varint()? {
        let graph = c.varint()?;
        let id = c.varint()?;
        out.push(SchemaDef::decode(&schema_key(graph, id), c.bytes()?)?);
    }
    c.done().then_some(out)
}

impl Dump<'_> {
    /// Calls `f` with every node or edge block of `graph`, in order.
    fn blocks(
        &self,
        kind: u8,
        graph: u64,
        f: &mut dyn FnMut(&Block) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut at = HEADER as u64;
        let mut ordinal = 0;
        while at < self.len {
            ordinal += 1;
            let b = read_block(self.file, self.len, &mut at, ordinal, self.pool)?;
            if b.kind == TRAILER {
                break;
            }
            if b.kind == kind {
                let mut c = Cursor {
                    b: &b.payload,
                    at: 0,
                };
                if c.varint() == Some(graph) {
                    f(&b)?;
                }
            }
        }
        Ok(())
    }

    /// The name of token `id` of `kind`; `None` if the catalog block has
    /// no such token.
    fn token(&self, id: u64, kind: TokenKind) -> Option<&str> {
        match self.names.get(&id) {
            Some((k, n)) if *k == kind => Some(n),
            _ => None,
        }
    }

    /// Labels (or nothing for an edge) and properties of record `bytes`,
    /// by name; `None` for a malformed record or an unknown token.
    fn decode_record(
        &self,
        bytes: &[u8],
        labels_kind: TokenKind,
    ) -> Option<super::model::Labelled> {
        let (labels, props) = crate::types::record::decode(bytes)?;
        let labels = labels
            .iter()
            .map(|&t| self.token(t, labels_kind).map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        let props = props
            .into_iter()
            .map(|(k, v)| Some((self.token(k, TokenKind::PropertyKey)?.to_owned(), v)))
            .collect::<Option<Vec<_>>>()?;
        Some((labels, props))
    }

    /// The records of graph `graph` of the dump, as an import [`Source`].
    pub(crate) fn graph(&self, graph: u64) -> GraphSource<'_, '_> {
        GraphSource { dump: self, graph }
    }
}

/// The node and edge records of one graph of a dump (TS-26: a record's
/// input identifier is its identifier in the dumped database).
pub(crate) struct GraphSource<'d, 'f> {
    dump: &'d Dump<'f>,
    graph: u64,
}

fn malformed(file: &Arc<str>, pos: Pos, id: Option<&str>) -> Invalid {
    Invalid::new(
        file,
        pos,
        id,
        codes::C22N05,
        "input failed validation: a malformed record or an unknown token in a native dump",
    )
}

impl Source for GraphSource<'_, '_> {
    fn nodes(&mut self, f: &mut Sink<'_, NodeIn>) -> Result<(), Error> {
        let d = self.dump;
        d.blocks(NODES, self.graph, &mut |b| {
            let mut c = Cursor {
                b: &b.payload,
                at: 0,
            };
            c.varint();
            let mut i = 0;
            while !c.done() {
                i += 1;
                let pos = Pos::Dump(b.ordinal, i);
                // Checked by `verify`, but the file is read again: a block
                // changed since then is damaged, never a panic (rmp #57
                // FINDING 459).
                let (Some(id), Some(rec)) = (c.varint(), c.bytes()) else {
                    return Err(damaged("a block changed since it was checked"));
                };
                let id = id.to_string();
                let item = match d.decode_record(rec, TokenKind::Label) {
                    Some((labels, props)) => Ok(NodeIn {
                        file: d.name.clone(),
                        pos,
                        id: Some(id),
                        space: String::new(),
                        labels,
                        props,
                    }),
                    None => Err(malformed(&d.name, pos, Some(&id))),
                };
                f(item)?;
            }
            Ok(())
        })
    }

    fn edges(&mut self, f: &mut Sink<'_, EdgeIn>) -> Result<(), Error> {
        let d = self.dump;
        d.blocks(EDGES, self.graph, &mut |b| {
            let mut c = Cursor {
                b: &b.payload,
                at: 0,
            };
            c.varint();
            let mut i = 0;
            while !c.done() {
                i += 1;
                let pos = Pos::Dump(b.ordinal, i);
                let v: Option<Vec<u64>> = (0..4).map(|_| c.varint()).collect();
                let (Some(v), Some(rec)) = (v, c.bytes()) else {
                    return Err(damaged("a block changed since it was checked"));
                };
                let id = v[0].to_string();
                let ty = d.token(v[3], TokenKind::EdgeType).map(str::to_owned);
                let item = match (ty, d.decode_record(rec, TokenKind::EdgeType)) {
                    (Some(ty), Some((labels, props))) if labels.is_empty() => Ok(EdgeIn {
                        file: d.name.clone(),
                        pos,
                        id: Some(id),
                        start: (String::new(), v[1].to_string()),
                        end: (String::new(), v[2].to_string()),
                        ty,
                        props,
                    }),
                    _ => Err(malformed(&d.name, pos, Some(&id))),
                };
                f(item)?;
            }
            Ok(())
        })
    }
}

/// Reads and checks `data` as a dump, then reads every record of every
/// graph it names: never panics, and a damaged dump fails with an error
/// (the `dump_read` fuzz target, rmp #57).
#[cfg(any(test, fuzzing))]
pub(crate) fn read_all(data: &[u8]) -> Result<u64, Error> {
    let file = Memory(data.to_vec());
    let d = verify(&file, "fuzz", None)?;
    let mut graphs: Vec<u64> = d
        .catalog
        .objects
        .iter()
        .filter(|o| o.kind == Kind::Graph)
        .map(|o| o.id)
        .collect();
    graphs.push(0);
    let mut n = 0u64;
    for g in graphs {
        let mut s = d.graph(g);
        s.nodes(&mut |_| {
            n += 1;
            Ok(())
        })?;
        s.edges(&mut |_| {
            n += 1;
            Ok(())
        })?;
    }
    Ok(n)
}

/// A read-only file in memory, for [`read_all`].
#[cfg(any(test, fuzzing))]
pub(crate) struct Memory(pub(crate) Vec<u8>);

#[cfg(any(test, fuzzing))]
impl StorageFile for Memory {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        let at = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        let n = buf.len().min(self.0.len() - at);
        buf[..n].copy_from_slice(&self.0[at..at + n]);
        Ok(n)
    }

    fn write_at(&self, _: &[u8], _: u64) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn sync_data(&self) -> io::Result<()> {
        Ok(())
    }

    fn set_len(&self, _: u64) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn len(&self) -> io::Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn reserve(&self, _: u64, _: u64) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}
