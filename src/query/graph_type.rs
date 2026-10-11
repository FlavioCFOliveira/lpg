//! Closed graph types (rmp #46; [TS-17] "Closed graph types", provisional
//! OQ-005): the model of a graph type written with GG03, its catalog
//! encoding, and the check that an element matches one of its element
//! types.
//!
//! **Rules** (provisional: the General Rules that define them are not in
//! the ISO inserts, OQ-005):
//!
//! * A node type is identified by its label set, which has at least one
//!   label (IL003: each element type has a key label set; without GG21 and
//!   GG23 the label set is that key). No two node types have the same
//!   label set.
//! * An edge type has exactly one label, its edge type (IL003), a source
//!   and a target node type. No two edge types have the same label
//!   (without GG25).
//! * A node matches a node type when its label set equals the type's and
//!   each of its properties is declared by the type with a type that
//!   accepts its value. An edge matches an edge type when its edge type is
//!   the type's label, its endpoints match the source and target node
//!   types, and its properties are declared as for nodes. A declared
//!   property may be absent: without GV90 every property type is
//!   nullable.
//!
//! The matching follows zu @ `a821319` (<https://github.com/tamnd/zu>,
//! Apache-2.0, see `LICENSE-APACHE`), `crates/zu-zu1/src/catalog.rs`
//! (`GraphType`, `closed`, `types_for`, `closed_type_of`) and
//! `crates/zu/src/declare.rs` (`promised`: an element that no element
//! type describes fails with G2000). Adapted: one element type per label
//! set; property values are checked by type. The type descriptors of
//! frogql @ `b5dd0f0` (MIT) `src/typing/` were studied only.
//!
//! **Encoding** (the catalog's graph-type definition bytes, never
//! changed after creation): a version byte 1; the node types (count,
//! then per type its labels and properties); the edge types (count, then
//! per type its label, source and target node type indexes and
//! properties). Counts and indexes are LEB128 varints, names are a varint
//! length and UTF-8 bytes, property types one byte.
//!
//! [TS-17]: ../../../docs/technical-spec/catalog-and-indexes.md

use std::collections::BTreeSet;

use crate::types::Value;

/// The type of a declared property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PropType {
    Bool = 1,
    Int = 2,
    Float = 3,
    String = 4,
    Date = 5,
    LocalTime = 6,
    ZonedTime = 7,
    LocalDateTime = 8,
    ZonedDateTime = 9,
    YearToMonth = 10,
    DayToSecond = 11,
}

impl PropType {
    fn from_byte(b: u8) -> Option<PropType> {
        use PropType as P;
        Some(match b {
            1 => P::Bool,
            2 => P::Int,
            3 => P::Float,
            4 => P::String,
            5 => P::Date,
            6 => P::LocalTime,
            7 => P::ZonedTime,
            8 => P::LocalDateTime,
            9 => P::ZonedDateTime,
            10 => P::YearToMonth,
            11 => P::DayToSecond,
            _ => return None,
        })
    }

    /// Whether a value of the property may be `v`.
    pub(crate) fn accepts(self, v: &Value) -> bool {
        use PropType as P;
        matches!(
            (self, v),
            (_, Value::Null)
                | (P::Bool, Value::Bool(_))
                | (P::Int, Value::Int(_))
                | (P::Float, Value::Float(_))
                | (P::String, Value::String(_))
                | (P::Date, Value::Date(_))
                | (P::LocalTime, Value::LocalTime(_))
                | (P::ZonedTime, Value::ZonedTime(_))
                | (P::LocalDateTime, Value::LocalDateTime(_))
                | (P::ZonedDateTime, Value::ZonedDateTime(_))
                | (P::YearToMonth, Value::YearToMonth(_))
                | (P::DayToSecond, Value::DayToSecond(_))
        )
    }
}

/// A node type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeType {
    /// Its labels, sorted.
    pub(crate) labels: BTreeSet<String>,
    /// Its properties, sorted by name.
    pub(crate) props: Vec<(String, PropType)>,
}

/// An edge type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EdgeType {
    /// Its label: the edge type.
    pub(crate) label: String,
    /// Its source node type (an index into [`GraphType::nodes`]).
    pub(crate) src: usize,
    /// Its target node type.
    pub(crate) dst: usize,
    /// Its properties, sorted by name.
    pub(crate) props: Vec<(String, PropType)>,
}

/// A graph type.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct GraphType {
    /// The node types.
    pub(crate) nodes: Vec<NodeType>,
    /// The edge types.
    pub(crate) edges: Vec<EdgeType>,
}

/// Whether the properties `props` (name, value) are all declared by
/// `declared` with an accepting type.
fn props_match(declared: &[(String, PropType)], props: &[(String, Value)]) -> bool {
    props.iter().all(|(n, v)| {
        declared
            .iter()
            .find(|(d, _)| d == n)
            .is_some_and(|(_, t)| t.accepts(v))
    })
}

impl GraphType {
    /// The node type a node with `labels` and `props` matches.
    pub(crate) fn node_type(
        &self,
        labels: &BTreeSet<String>,
        props: &[(String, Value)],
    ) -> Option<usize> {
        self.nodes
            .iter()
            .position(|t| &t.labels == labels && props_match(&t.props, props))
    }

    /// Whether an edge of type `label` with `props` between nodes with
    /// labels `src` and `dst` matches an edge type.
    pub(crate) fn edge_matches(
        &self,
        label: &str,
        props: &[(String, Value)],
        src: &BTreeSet<String>,
        dst: &BTreeSet<String>,
    ) -> bool {
        self.edges.iter().any(|t| {
            t.label == label
                && &self.nodes[t.src].labels == src
                && &self.nodes[t.dst].labels == dst
                && props_match(&t.props, props)
        })
    }

    /// The encoding of the type (module documentation).
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = vec![1u8];
        let props = |out: &mut Vec<u8>, ps: &[(String, PropType)]| {
            varint(out, ps.len() as u64);
            for (n, t) in ps {
                string(out, n);
                out.push(*t as u8);
            }
        };
        varint(&mut out, self.nodes.len() as u64);
        for n in &self.nodes {
            varint(&mut out, n.labels.len() as u64);
            for l in &n.labels {
                string(&mut out, l);
            }
            props(&mut out, &n.props);
        }
        varint(&mut out, self.edges.len() as u64);
        for e in &self.edges {
            string(&mut out, &e.label);
            varint(&mut out, e.src as u64);
            varint(&mut out, e.dst as u64);
            props(&mut out, &e.props);
        }
        out
    }

    /// Decodes [`GraphType::encode`]'s bytes; `None` if malformed.
    pub(crate) fn decode(b: &[u8]) -> Option<GraphType> {
        let mut r = Reader { b, at: 0 };
        if r.byte()? != 1 {
            return None;
        }
        let props = |r: &mut Reader<'_>| -> Option<Vec<(String, PropType)>> {
            let n = r.varint()?;
            let mut v = Vec::new();
            for _ in 0..n {
                let name = r.string()?;
                v.push((name, PropType::from_byte(r.byte()?)?));
            }
            Some(v)
        };
        let mut g = GraphType::default();
        for _ in 0..r.varint()? {
            let mut labels = BTreeSet::new();
            for _ in 0..r.varint()? {
                labels.insert(r.string()?);
            }
            g.nodes.push(NodeType {
                labels,
                props: props(&mut r)?,
            });
        }
        for _ in 0..r.varint()? {
            let label = r.string()?;
            let src = usize::try_from(r.varint()?).ok()?;
            let dst = usize::try_from(r.varint()?).ok()?;
            if src >= g.nodes.len() || dst >= g.nodes.len() {
                return None;
            }
            g.edges.push(EdgeType {
                label,
                src,
                dst,
                props: props(&mut r)?,
            });
        }
        (r.at == b.len()).then_some(g)
    }
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn string(out: &mut Vec<u8>, s: &str) {
    varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.b.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= u64::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn string(&mut self) -> Option<String> {
        let n = usize::try_from(self.varint()?).ok()?;
        let s = std::str::from_utf8(self.b.get(self.at..self.at.checked_add(n)?)?).ok()?;
        self.at += n;
        Some(s.to_string())
    }
}
