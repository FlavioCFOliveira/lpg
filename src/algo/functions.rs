//! The GDS functions of the similarity and link-prediction family (rmp
//! #65): ALG-107 `gds.similarity.*` and ALG-112 to ALG-117
//! `gds.linkprediction.*`.
//!
//! GQL has no function call in value expressions, so a function is a
//! routine with one result column called with `CALL` (FR-API-006,
//! TS-24). The column is named after what the function returns:
//! `similarity` for ALG-107, `distance` for `euclideanDistance`, `score`
//! for ALG-112 to ALG-117 (GDS's functions return a bare `FLOAT`; the
//! names are lpg's). The common keys do not apply (TS-25 Orientation).
//!
//! **Semantics: GDS 2026.09.0's** (GDS @ `417d7ecf973d`, GPL: studied, not
//! copied; `proc/common/.../functions/SimilaritiesFunc.java`,
//! `core/.../utils/Intersections.java`,
//! `alpha/alpha-proc/.../linkprediction/LinkPredictionFunc.java`,
//! `NeighborsFinder.java`):
//!
//! * `cosine`, `pearson`, `euclideanDistance`, `euclidean`: vectors of
//!   equal, non-zero length (22N11 otherwise); a null element counts as 0.
//!   `pearson` is 0 when undefined; `euclidean` is `1 / (1 + distance)`.
//! * `jaccard`: lists as multisets, nulls removed, compared by numeric
//!   value; 1 for two empty lists; 0 when an argument is null.
//! * `overlap`: the distinct values two lists share over the shorter
//!   list's length, values compared by type and value.
//! * Link prediction on the stored graph, in the caller's snapshot:
//!   `relationshipQuery` names one edge type (all types when absent);
//!   `direction` is `OUTGOING` (`OUT`, `O`, `>`), `INCOMING` (`IN`, `I`,
//!   `<`) or `BOTH` (`B`, `<>`), any case; another value means `OUTGOING`,
//!   as GDS's parser falls back to it. Neighbours are the distinct other
//!   endpoints, the node itself excluded; a degree counts edges, a
//!   self-loop once under `BOTH`. Common neighbours of a node and itself
//!   are none.
//! * `sameCommunity`: 1 when both nodes have the property and its values
//!   are equal by type and value, else 0.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::catalog::TokenKind;
use crate::error::Error;
use crate::gqlstatus::codes;
use crate::proc::{Body, Function, Parameter, ReadContext, Routine, Signature};
use crate::storage::graph::{IN, OUT};
use crate::types::Value;

/// A function of the family.
struct Gds(fn(&mut ReadContext<'_>, &[Value]) -> Result<Value, Error>);

impl Function for Gds {
    fn call(&self, cx: &mut ReadContext<'_>, args: &[Value]) -> Result<Value, Error> {
        (self.0)(cx, args)
    }
}

/// Whether `name` is one of the functions (the Rust API calls them as it
/// calls an algorithm procedure, TS-25 Rust API).
pub(crate) fn is_function(name: &str) -> bool {
    name.starts_with("gds.similarity.") || name.starts_with("gds.linkprediction.")
}

/// The routines of the functions (module documentation).
pub(crate) fn routines() -> Vec<Routine> {
    let two = || vec![Parameter::new("vector1"), Parameter::new("vector2")];
    let nodes = || {
        vec![
            Parameter::new("node1"),
            Parameter::new("node2"),
            Parameter::with_default("config", Value::Record(Vec::new().into())),
        ]
    };
    let f =
        |name: &str,
         params: Vec<Parameter>,
         column: &str,
         body: fn(&mut ReadContext<'_>, &[Value]) -> Result<Value, Error>| Routine {
            sig: Signature::new(name, params, &[column]),
            body: Body::Function(Arc::new(Gds(body))),
        };
    vec![
        f("gds.similarity.cosine", two(), "similarity", |_, a| {
            vectors(a, cosine)
        }),
        f("gds.similarity.pearson", two(), "similarity", |_, a| {
            vectors(a, pearson)
        }),
        f(
            "gds.similarity.euclideanDistance",
            two(),
            "distance",
            |_, a| vectors(a, euclidean_distance),
        ),
        f("gds.similarity.euclidean", two(), "similarity", |_, a| {
            vectors(a, |x, y| 1.0 / (1.0 + euclidean_distance(x, y)))
        }),
        f("gds.similarity.jaccard", two(), "similarity", |_, a| {
            jaccard(a)
        }),
        f("gds.similarity.overlap", two(), "similarity", |_, a| {
            overlap(a)
        }),
        f(
            "gds.linkprediction.adamicAdar",
            nodes(),
            "score",
            |cx, a| link(cx, a, Measure::AdamicAdar),
        ),
        f(
            "gds.linkprediction.commonNeighbors",
            nodes(),
            "score",
            |cx, a| link(cx, a, Measure::Common),
        ),
        f(
            "gds.linkprediction.preferentialAttachment",
            nodes(),
            "score",
            |cx, a| link(cx, a, Measure::Preferential),
        ),
        f(
            "gds.linkprediction.resourceAllocation",
            nodes(),
            "score",
            |cx, a| link(cx, a, Measure::Resource),
        ),
        f(
            "gds.linkprediction.totalNeighbors",
            nodes(),
            "score",
            |cx, a| link(cx, a, Measure::Total),
        ),
        f(
            "gds.linkprediction.sameCommunity",
            vec![
                Parameter::new("node1"),
                Parameter::new("node2"),
                Parameter::with_default("communityProperty", Value::string("community")),
            ],
            "score",
            same_community,
        ),
    ]
}

fn invalid(what: &str) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

fn wrong(what: &str) -> Error {
    Error::new(codes::C22G03, format!("invalid value type: {what}"))
}

/// The elements of a list argument.
fn list(v: &Value) -> Result<&[Value], Error> {
    match v {
        Value::List(l) => Ok(l),
        _ => Err(wrong("a similarity function takes two lists of numbers")),
    }
}

/// A list element as a float: a null is 0 (GDS `getDoubleValue`).
fn element(v: &Value) -> Result<f64, Error> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        Value::Null => Ok(0.0),
        _ => Err(wrong("a similarity function takes lists of numbers")),
    }
}

/// `f` of two vectors of equal, non-zero length.
fn vectors(args: &[Value], f: fn(&[f64], &[f64]) -> f64) -> Result<Value, Error> {
    let a = list(&args[0])?
        .iter()
        .map(element)
        .collect::<Result<Vec<_>, _>>()?;
    let b = list(&args[1])?
        .iter()
        .map(element)
        .collect::<Result<Vec<_>, _>>()?;
    if a.len() != b.len() || a.is_empty() {
        return Err(invalid("vectors must be non-empty and of the same size"));
    }
    Ok(Value::Float(f(&a, &b)))
}

/// Cosine similarity (GDS `Intersections.cosine`).
pub(crate) fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let (mut dot, mut xx, mut yy) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        xx += x * x;
        yy += y * y;
    }
    dot / (xx * yy).sqrt()
}

/// Pearson similarity, 0 when undefined (GDS `Intersections.pearson`).
pub(crate) fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut dot, mut xx, mut yy) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (x - ma, y - mb);
        dot += dx * dy;
        xx += dx * dx;
        yy += dy * dy;
    }
    let r = dot / (xx * yy).sqrt();
    if r.is_nan() { 0.0 } else { r }
}

/// Euclidean distance.
pub(crate) fn euclidean_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// A number of a list, for `jaccard`: integers and floats by value.
#[derive(Debug, Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn of(v: &Value) -> Result<Option<Num>, Error> {
        match v {
            Value::Int(i) => Ok(Some(Num::Int(*i))),
            Value::Float(f) => Ok(Some(Num::Float(*f))),
            Value::Null => Ok(None),
            _ => Err(wrong("a similarity function takes lists of numbers")),
        }
    }

    /// Numeric order, exact between integers and floats.
    fn cmp(self, o: Num) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let int_float = |i: i64, f: f64| -> Ordering {
            if f.is_nan() {
                return Ordering::Less;
            }
            // Compare exactly: `i64::MAX as f64` is 2^63, and every
            // float in [-2^63, 2^63) truncates to an `i64`.
            let t = f.trunc();
            let lim = i64::MAX as f64;
            if t < -lim {
                return Ordering::Greater;
            }
            if t >= lim {
                return Ordering::Less;
            }
            let ti = t as i64;
            i.cmp(&ti).then(if f > t {
                Ordering::Less
            } else if f < t {
                Ordering::Greater
            } else {
                Ordering::Equal
            })
        };
        match (self, o) {
            (Num::Int(a), Num::Int(b)) => a.cmp(&b),
            (Num::Int(a), Num::Float(b)) => int_float(a, b),
            (Num::Float(a), Num::Int(b)) => int_float(b, a).reverse(),
            (Num::Float(a), Num::Float(b)) => a.total_cmp(&b),
        }
    }
}

/// `jaccard` (module documentation).
fn jaccard(args: &[Value]) -> Result<Value, Error> {
    if args[0].is_null() || args[1].is_null() {
        return Ok(Value::Float(0.0));
    }
    let sorted = |v: &Value| -> Result<Vec<Num>, Error> {
        let mut out: Vec<Num> = list(v)?
            .iter()
            .map(Num::of)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        out.sort_by(|a, b| a.cmp(*b));
        Ok(out)
    };
    let (a, b) = (sorted(&args[0])?, sorted(&args[1])?);
    let (mut i, mut j, mut inter, mut union) = (0, 0, 0u64, 0u64);
    while i < a.len() && j < b.len() {
        match a[i].cmp(b[j]) {
            std::cmp::Ordering::Equal => {
                inter += 1;
                union += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                union += 1;
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                union += 1;
                j += 1;
            }
        }
    }
    union += (a.len() - i + b.len() - j) as u64;
    Ok(Value::Float(if union == 0 {
        1.0
    } else {
        inter as f64 / union as f64
    }))
}

/// A value as a set member for `overlap` and `sameCommunity`: type and
/// value (Java `equals` on `Long` and `Double`).
fn key(v: &Value) -> Option<(u8, u64)> {
    match v {
        Value::Int(i) => Some((0, i.cast_unsigned())),
        Value::Float(f) => Some((1, f.to_bits())),
        _ => None,
    }
}

/// `overlap` (module documentation).
fn overlap(args: &[Value]) -> Result<Value, Error> {
    let a: Vec<&Value> = list(&args[0])?.iter().filter(|v| !v.is_null()).collect();
    let b: Vec<&Value> = list(&args[1])?.iter().filter(|v| !v.is_null()).collect();
    for v in a.iter().chain(&b) {
        Num::of(v)?;
    }
    let sa: BTreeSet<(u8, u64)> = a.iter().filter_map(|v| key(v)).collect();
    let sb: BTreeSet<(u8, u64)> = b.iter().filter_map(|v| key(v)).collect();
    let inter = sa.intersection(&sb).count();
    let den = a.len().min(b.len());
    Ok(Value::Float(if den == 0 {
        0.0
    } else {
        inter as f64 / den as f64
    }))
}

/// A link-prediction measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    AdamicAdar,
    Common,
    Preferential,
    Resource,
    Total,
}

/// The direction of a link-prediction call (module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Out,
    In,
    Both,
}

/// The node element identifier of a node argument.
fn node_arg(v: &Value, name: &str) -> Result<u64, Error> {
    match v {
        Value::Node(id) => Ok(*id),
        Value::Null => Err(invalid("nodes must not be null")),
        _ => Err(wrong(&format!("the argument {name} must be a node"))),
    }
}

/// The edges of `node` in `dir` with type `ty`: `(neighbour, edge)`.
fn edges(
    cx: &mut ReadContext<'_>,
    node: u64,
    dir: Direction,
    ty: Option<Option<u64>>,
) -> Result<Vec<(u64, u64)>, Error> {
    let g = cx.st.graphs[cx.graph];
    let ty = match ty {
        // A type no edge has: no edge.
        Some(None) => return Ok(Vec::new()),
        Some(Some(t)) => Some(t),
        None => None,
    };
    let mut out = Vec::new();
    for d in match dir {
        Direction::Out => &[OUT][..],
        Direction::In => &[IN][..],
        Direction::Both => &[OUT, IN][..],
    } {
        for a in cx.tx.expand(&g, node, *d, ty)? {
            out.push((a.nbr, a.edge));
        }
    }
    Ok(out)
}

/// The distinct neighbours of `node`, itself excluded, and its degree.
fn neighbours(
    cx: &mut ReadContext<'_>,
    node: u64,
    dir: Direction,
    ty: Option<Option<u64>>,
) -> Result<(BTreeSet<u64>, usize), Error> {
    let es = edges(cx, node, dir, ty)?;
    let degree = es.iter().map(|(_, e)| *e).collect::<BTreeSet<u64>>().len();
    let set = es.iter().map(|(n, _)| *n).filter(|n| *n != node).collect();
    Ok((set, degree))
}

/// ALG-112 to ALG-115 and ALG-117 (module documentation).
fn link(cx: &mut ReadContext<'_>, args: &[Value], m: Measure) -> Result<Value, Error> {
    let a = node_arg(&args[0], "node1")?;
    let b = node_arg(&args[1], "node2")?;
    let config: &[(Arc<str>, Value)] = match &args[2] {
        Value::Record(r) => r,
        Value::Null => &[],
        _ => return Err(wrong("the argument config must be a record")),
    };
    let mut ty = None;
    let mut dir = Direction::Both;
    for (k, v) in config {
        match (&**k, v) {
            ("relationshipQuery", Value::String(t)) => {
                ty = Some(cx.tx.engine().token_id(TokenKind::EdgeType, t));
            }
            ("direction", Value::String(d)) => {
                dir = match d.to_lowercase().as_str() {
                    "incoming" | "in" | "i" | "<" => Direction::In,
                    "both" | "b" | "<>" => Direction::Both,
                    _ => Direction::Out,
                };
            }
            ("relationshipQuery" | "direction", Value::Null) => {}
            ("relationshipQuery" | "direction", _) => {
                return Err(wrong(&format!(
                    "the configuration key {k} must be a string"
                )));
            }
            _ => return Err(invalid(&format!("unexpected configuration key: {k}"))),
        }
    }
    let (na, da) = neighbours(cx, a, dir, ty)?;
    let (nb, db) = neighbours(cx, b, dir, ty)?;
    let score = match m {
        Measure::Preferential => (da * db) as f64,
        Measure::Total => na.union(&nb).count() as f64,
        Measure::Common | Measure::AdamicAdar | Measure::Resource => {
            if a == b {
                0.0
            } else {
                let common: Vec<u64> = na.intersection(&nb).copied().collect();
                match m {
                    Measure::Common => common.len() as f64,
                    _ => {
                        let mut sum = 0.0;
                        for c in common {
                            let d = edges(cx, c, dir, ty)?
                                .iter()
                                .map(|(_, e)| *e)
                                .collect::<BTreeSet<u64>>()
                                .len() as f64;
                            sum += if m == Measure::AdamicAdar {
                                1.0 / d.ln()
                            } else {
                                1.0 / d
                            };
                        }
                        sum
                    }
                }
            }
        }
    };
    Ok(Value::Float(score))
}

/// ALG-116 (module documentation).
fn same_community(cx: &mut ReadContext<'_>, args: &[Value]) -> Result<Value, Error> {
    let a = node_arg(&args[0], "node1")?;
    let b = node_arg(&args[1], "node2")?;
    let Value::String(name) = &args[2] else {
        return Err(wrong("the argument communityProperty must be a string"));
    };
    let Some(prop) = cx.tx.engine().token_id(TokenKind::PropertyKey, name) else {
        return Ok(Value::Float(0.0));
    };
    let g = cx.st.graphs[cx.graph];
    let mut value = |id: u64| -> Result<Option<Value>, Error> {
        Ok(cx
            .tx
            .node(&g, id)?
            .and_then(|(_, ps)| ps.into_iter().find(|(k, _)| *k == prop).map(|(_, v)| v)))
    };
    let (va, vb) = (value(a)?, value(b)?);
    let same = match (va, vb) {
        (Some(x), Some(y)) => match (key(&x), key(&y)) {
            (Some(kx), Some(ky)) => kx == ky,
            _ => x == y,
        },
        _ => false,
    };
    Ok(Value::Float(if same { 1.0 } else { 0.0 }))
}
