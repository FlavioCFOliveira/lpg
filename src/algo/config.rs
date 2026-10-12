//! The parameter record of an algorithm run ([TS-25] Names and
//! signatures; rmp #63).
//!
//! One record serves `CALL` and the Rust API: both pass the routine's
//! `configuration` argument, a GQL record, which [`Config::parse`] checks
//! against the entry's keys ([`Key`]) and the common keys every procedure
//! takes. The parsed record keeps every key with its value or default, in
//! key order, and is returned as the `configuration` column of a write
//! mode.
//!
//! * An unknown key fails with 22N11, as GDS rejects one ("Unexpected
//!   configuration key", measured on GDS 2026.09.0, rmp #62); a value of
//!   the wrong type with 22G03; a value outside its domain with 22N11
//!   (`src/proc/builtin.rs` uses the same codes).
//! * A null value counts as an absent key.
//! * Keys that tune only GDS's own machinery ([`IGNORED`]) are accepted
//!   and change nothing; they appear in `configuration` only when given.
//!
//! [TS-25]: ../../docs/technical-spec/algorithms.md

use std::sync::Arc;

use crate::error::Error;
use crate::gqlstatus::codes;
use crate::types::Value;

/// The type and domain of a configuration key.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Ty {
    /// An integer of at least `min`.
    Int { min: i64 },
    /// A number in `[min, max]`, or `[min, max)` when `max_open`; an
    /// integer is accepted (IA005).
    Float { min: f64, max: f64, max_open: bool },
    /// A string.
    Str,
    /// One of the given words, compared without case and kept in upper
    /// case (GDS's `orientation` values).
    Choice(&'static [&'static str]),
    /// A string or a list of strings (labels, edge types).
    Names,
    /// A list of nodes or node element identifiers (`sourceNodes`); a
    /// single one is a list of one.
    Nodes,
    /// One node or node element identifier (`sourceNode`).
    Node,
    /// A boolean.
    Bool,
    /// A list of non-negative integers (`degreeSequence`).
    Ints,
    /// A GDS scaler: a name, or a record with `type` and, for `log`,
    /// `offset` ([`super::scale`]).
    Scaler,
}

/// The default of a configuration key.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Def {
    /// The key must be given.
    Required,
    /// No default: null when absent.
    Absent,
    Int(i64),
    Float(f64),
    Str(&'static str),
    Bool(bool),
    /// `["*"]`: every label or edge type.
    Star,
    /// `[]`.
    EmptyList,
}

/// A configuration key: its name, type and default.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Key {
    pub(crate) name: &'static str,
    pub(crate) ty: Ty,
    pub(crate) def: Def,
}

impl Key {
    pub(crate) const fn new(name: &'static str, ty: Ty, def: Def) -> Key {
        Key { name, ty, def }
    }
}

/// `nodeLabels`: the labels a node of the projection has one of.
pub(crate) const NODE_LABELS: &str = "nodeLabels";
/// `relationshipTypes`: the edge types of the projection.
pub(crate) const RELATIONSHIP_TYPES: &str = "relationshipTypes";
/// `concurrency`: the most worker threads of a parallel entry.
pub(crate) const CONCURRENCY: &str = "concurrency";
/// `orientation`: which arrays of the projection a run reads.
pub(crate) const ORIENTATION: &str = "orientation";
/// `randomSeed`: the seed of a randomised entry.
pub(crate) const RANDOM_SEED: &str = "randomSeed";
/// `writeProperty`: the node property a node-property write mode sets.
pub(crate) const WRITE_PROPERTY: &str = "writeProperty";

/// The orientation values of GDS (TS-25 Orientation).
pub(crate) const ORIENTATIONS: &[&str] = &["NATURAL", "REVERSE", "UNDIRECTED"];

/// The keys every procedure takes (TS-25 Common keys), before the entry's
/// own. `concurrency` defaults to 4, as GDS's does.
pub(crate) const COMMON: &[Key] = &[
    Key::new(NODE_LABELS, Ty::Names, Def::Star),
    Key::new(RELATIONSHIP_TYPES, Ty::Names, Def::Star),
    Key::new(CONCURRENCY, Ty::Int { min: 1 }, Def::Int(4)),
    Key::new(ORIENTATION, Ty::Choice(ORIENTATIONS), Def::Str("NATURAL")),
];

/// The seed key of the randomised entries (`samplingSeed` for ALG-035 is
/// an entry key of its own).
pub(crate) const SEED: Key = Key::new(RANDOM_SEED, Ty::Int { min: i64::MIN }, Def::Int(0));

/// The key of a node-property write mode.
pub(crate) const WRITE: Key = Key::new(WRITE_PROPERTY, Ty::Str, Def::Required);

/// `writeRelationshipType`: the type of the relationships a write mode
/// creates.
pub(crate) const WRITE_RELATIONSHIP_TYPE: &str = "writeRelationshipType";

/// The keys of a node-property write mode.
pub(crate) const NODE_WRITE: &[Key] = &[WRITE];

/// The keys of a relationship write mode (`algorithm-entries.md` RelWrite).
pub(crate) const REL_WRITE: &[Key] = &[Key::new(WRITE_RELATIONSHIP_TYPE, Ty::Str, Def::Required)];

/// The keys of a path write mode (`algorithm-entries.md` PathWrite).
pub(crate) const PATH_WRITE: &[Key] = &[
    Key::new(WRITE_RELATIONSHIP_TYPE, Ty::Str, Def::Required),
    Key::new("writeNodeIds", Ty::Bool, Def::Bool(false)),
    Key::new("writeCosts", Ty::Bool, Def::Bool(false)),
];

/// Keys accepted with no effect (TS-25 Common keys).
pub(crate) const IGNORED: &[&str] = &[
    "jobId",
    "logProgress",
    "sudo",
    "writeConcurrency",
    "writeToResultStore",
];

/// A parsed configuration record (module documentation).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Config {
    values: Vec<(&'static str, Value)>,
    /// Ignored keys as given.
    ignored: Vec<(Arc<str>, Value)>,
}

fn invalid(what: String) -> Error {
    Error::new(codes::C22N11, format!("invalid argument: {what}"))
}

fn wrong_type(key: &str, what: &str) -> Error {
    Error::new(
        codes::C22G03,
        format!("invalid value type: the configuration key {key} must be {what}"),
    )
}

impl Config {
    /// Checks `record` (the `configuration` argument; null for none)
    /// against `keys`, in that order.
    ///
    /// # Errors
    ///
    /// 22N11 for an unknown key, a missing required key or a value out of
    /// its domain; 22G03 for a value of the wrong type or a configuration
    /// that is not a record.
    pub(crate) fn parse(record: &Value, keys: &[Key]) -> Result<Config, Error> {
        let given: &[(Arc<str>, Value)] = match record {
            Value::Null => &[],
            Value::Record(r) => r,
            _ => {
                return Err(Error::new(
                    codes::C22G03,
                    "invalid value type: the configuration must be a record",
                ));
            }
        };
        let mut ignored = Vec::new();
        for (k, v) in given {
            if keys.iter().any(|key| key.name == &**k) {
                continue;
            }
            if IGNORED.contains(&&**k) {
                ignored.push((k.clone(), v.clone()));
                continue;
            }
            return Err(invalid(format!("unexpected configuration key: {k}")));
        }
        let mut values = Vec::with_capacity(keys.len());
        for key in keys {
            let v = given
                .iter()
                .find(|(k, _)| **k == *key.name)
                .map(|(_, v)| v)
                .filter(|v| !v.is_null());
            let v = match v {
                Some(v) => check(key, v)?,
                None => match key.def {
                    Def::Required => {
                        return Err(invalid(format!(
                            "the configuration needs the key {}",
                            key.name
                        )));
                    }
                    Def::Absent => Value::Null,
                    Def::Int(i) => Value::Int(i),
                    Def::Float(f) => Value::Float(f),
                    Def::Str(s) => Value::string(s),
                    Def::Bool(b) => Value::Bool(b),
                    Def::Star => Value::List(vec![Value::string("*")].into()),
                    Def::EmptyList => Value::List(Vec::new().into()),
                },
            };
            values.push((key.name, v));
        }
        Ok(Config { values, ignored })
    }

    /// The value of key `name`.
    ///
    /// # Panics
    ///
    /// If the entry has no such key: a programming error of the entry.
    pub(crate) fn get(&self, name: &str) -> &Value {
        self.values
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("the entry declares the key {name}"))
    }

    /// An integer key.
    pub(crate) fn int(&self, name: &str) -> i64 {
        match self.get(name) {
            Value::Int(i) => *i,
            other => panic!("the key {name} holds {other:?}, not an integer"),
        }
    }

    /// A number key, as a float.
    pub(crate) fn float(&self, name: &str) -> f64 {
        match self.get(name) {
            Value::Float(f) => *f,
            other => panic!("the key {name} holds {other:?}, not a float"),
        }
    }

    /// A string key, `None` when null.
    pub(crate) fn str(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The strings of a [`Ty::Names`] key.
    pub(crate) fn names(&self, name: &str) -> Vec<&str> {
        match self.get(name) {
            Value::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(&**s),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The node element identifiers of a [`Ty::Nodes`] key.
    pub(crate) fn nodes(&self, name: &str) -> Vec<u64> {
        match self.get(name) {
            Value::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    Value::Node(id) => Some(*id),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A [`Ty::Bool`] key; false when null.
    pub(crate) fn bool(&self, name: &str) -> bool {
        matches!(self.get(name), Value::Bool(true))
    }

    /// The integers of a [`Ty::Ints`] key.
    pub(crate) fn ints(&self, name: &str) -> Vec<i64> {
        match self.get(name) {
            Value::List(l) => l
                .iter()
                .filter_map(|v| match v {
                    Value::Int(i) => Some(*i),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `concurrency`.
    pub(crate) fn concurrency(&self) -> usize {
        usize::try_from(self.int(CONCURRENCY)).unwrap_or(usize::MAX)
    }

    /// `randomSeed` as the generator's seed.
    pub(crate) fn seed(&self) -> u64 {
        self.int(RANDOM_SEED).cast_unsigned()
    }

    /// Forces key `name` to `v` (TS-25: the entries marked U always run
    /// `UNDIRECTED`).
    pub(crate) fn set(&mut self, name: &'static str, v: Value) {
        if let Some(slot) = self.values.iter_mut().find(|(k, _)| *k == name) {
            slot.1 = v;
        }
    }

    /// The record as the `configuration` result column shows it: every
    /// key with its value, then the ignored keys as given.
    pub(crate) fn record(&self) -> Value {
        let mut out: Vec<(Arc<str>, Value)> = self
            .values
            .iter()
            .map(|(k, v)| (Arc::from(*k), v.clone()))
            .collect();
        out.extend(self.ignored.iter().cloned());
        Value::Record(out.into())
    }
}

/// `v` checked against `key`, normalised: floats for [`Ty::Float`], lists
/// for [`Ty::Names`], node references for [`Ty::Nodes`], upper case for
/// [`Ty::Choice`].
fn check(key: &Key, v: &Value) -> Result<Value, Error> {
    let name = key.name;
    match key.ty {
        Ty::Int { min } => match v {
            Value::Int(i) if *i >= min => Ok(Value::Int(*i)),
            Value::Int(_) => Err(invalid(format!(
                "the configuration key {name} must be at least {min}"
            ))),
            _ => Err(wrong_type(name, "an integer")),
        },
        Ty::Float { min, max, max_open } => {
            let f = match v {
                Value::Int(i) => *i as f64,
                Value::Float(f) => *f,
                _ => return Err(wrong_type(name, "a number")),
            };
            let below_max = if max_open { f < max } else { f <= max };
            if f >= min && below_max {
                Ok(Value::Float(f))
            } else {
                let close = if max_open { ")" } else { "]" };
                Err(invalid(format!(
                    "the configuration key {name} must be in [{min}, {max}{close}"
                )))
            }
        }
        Ty::Str => match v {
            Value::String(_) => Ok(v.clone()),
            _ => Err(wrong_type(name, "a string")),
        },
        Ty::Choice(words) => match v {
            Value::String(s) => {
                let up = s.to_uppercase();
                if words.contains(&up.as_str()) {
                    Ok(Value::string(&up))
                } else {
                    Err(invalid(format!(
                        "the configuration key {name} must be one of {}",
                        words.join(", ")
                    )))
                }
            }
            _ => Err(wrong_type(name, "a string")),
        },
        Ty::Names => match v {
            Value::String(_) => Ok(Value::List(vec![v.clone()].into())),
            Value::List(l) if l.iter().all(|x| matches!(x, Value::String(_))) => Ok(v.clone()),
            _ => Err(wrong_type(name, "a string or a list of strings")),
        },
        Ty::Nodes => {
            let one = |x: &Value| match x {
                Value::Node(id) => Ok(Value::Node(*id)),
                Value::Int(i) if *i >= 0 => Ok(Value::Node(i.cast_unsigned())),
                Value::Int(_) => Err(invalid(format!(
                    "the configuration key {name} holds a negative node identifier"
                ))),
                _ => Err(wrong_type(name, "a list of nodes or node identifiers")),
            };
            match v {
                Value::List(l) => Ok(Value::List(
                    l.iter().map(one).collect::<Result<Vec<_>, _>>()?.into(),
                )),
                x => Ok(Value::List(vec![one(x)?].into())),
            }
        }
        Ty::Node => match v {
            Value::Node(id) => Ok(Value::Node(*id)),
            Value::Int(i) if *i >= 0 => Ok(Value::Node(i.cast_unsigned())),
            Value::Int(_) => Err(invalid(format!(
                "the configuration key {name} holds a negative node identifier"
            ))),
            _ => Err(wrong_type(name, "a node or a node identifier")),
        },
        Ty::Bool => match v {
            Value::Bool(_) => Ok(v.clone()),
            _ => Err(wrong_type(name, "a boolean")),
        },
        Ty::Ints => match v {
            Value::List(l) if l.iter().all(|x| matches!(x, Value::Int(_))) => {
                if l.iter().any(|x| matches!(x, Value::Int(i) if *i < 0)) {
                    Err(invalid(format!(
                        "the configuration key {name} holds a negative integer"
                    )))
                } else {
                    Ok(v.clone())
                }
            }
            _ => Err(wrong_type(name, "a list of integers")),
        },
        Ty::Scaler => super::scale::Scaler::parse(name, v).map(|s| s.value()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(pairs: &[(&str, Value)]) -> Value {
        Value::Record(
            pairs
                .iter()
                .map(|(k, v)| (Arc::from(*k), v.clone()))
                .collect::<Vec<_>>()
                .into(),
        )
    }

    const KEYS: &[Key] = &[
        Key::new("n", Ty::Int { min: 1 }, Def::Int(20)),
        Key::new(
            "d",
            Ty::Float {
                min: 0.0,
                max: 1.0,
                max_open: true,
            },
            Def::Float(0.85),
        ),
        Key::new("w", Ty::Str, Def::Absent),
        Key::new("o", Ty::Choice(ORIENTATIONS), Def::Str("NATURAL")),
        Key::new("l", Ty::Names, Def::Star),
        Key::new("s", Ty::Nodes, Def::EmptyList),
    ];

    /// Defaults fill absent and null keys; integers are accepted for
    /// floats; names, nodes and choices are normalised; ignored keys are
    /// kept as given.
    #[test]
    fn defaults_and_normalisation() {
        let c = Config::parse(
            &rec(&[
                ("d", Value::Int(0)),
                ("w", Value::Null),
                ("o", Value::string("undirected")),
                ("l", Value::string("A")),
                ("s", Value::List(vec![Value::Int(3), Value::Node(4)].into())),
                ("jobId", Value::string("x")),
            ]),
            KEYS,
        )
        .unwrap();
        assert_eq!(c.int("n"), 20);
        assert_eq!(c.float("d"), 0.0);
        assert_eq!(c.str("w"), None);
        assert_eq!(c.str("o"), Some("UNDIRECTED"));
        assert_eq!(c.names("l"), vec!["A"]);
        assert_eq!(c.nodes("s"), vec![3, 4]);
        let Value::Record(r) = c.record() else {
            panic!("a record")
        };
        assert_eq!(r.len(), KEYS.len() + 1);
        assert_eq!(&*r[KEYS.len()].0, "jobId");
        assert_eq!(
            Config::parse(&Value::Null, KEYS).unwrap().names("l"),
            vec!["*"]
        );
    }

    /// Each failure has its code: unknown key and domain 22N11, type 22G03.
    #[test]
    fn failures() {
        let code = |pairs: &[(&str, Value)]| Config::parse(&rec(pairs), KEYS).unwrap_err().code();
        assert_eq!(code(&[("graphName", Value::Int(1))]), "22N11");
        assert_eq!(code(&[("n", Value::Int(0))]), "22N11");
        assert_eq!(code(&[("n", Value::Float(1.0))]), "22G03");
        assert_eq!(code(&[("d", Value::Float(1.0))]), "22N11");
        assert_eq!(code(&[("d", Value::Float(f64::NAN))]), "22N11");
        assert_eq!(code(&[("o", Value::string("UP"))]), "22N11");
        assert_eq!(code(&[("l", Value::Int(1))]), "22G03");
        assert_eq!(code(&[("s", Value::Int(-1))]), "22N11");
        assert_eq!(
            Config::parse(&Value::Int(1), KEYS).unwrap_err().code(),
            "22G03"
        );
        let req = [Key::new("p", Ty::Str, Def::Required)];
        assert_eq!(
            Config::parse(&Value::Null, &req).unwrap_err().code(),
            "22N11"
        );
    }
}
