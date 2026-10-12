//! The operation trace of the algorithm entries (rmp #71), compiled under
//! `cfg(test)` only: normal and release builds hold none of it, and the
//! [`trace!`](super::trace) macro expands to nothing there.
//!
//! * **Recording.** A test starts a [`Recording`] on its thread; every
//!   `trace!(kind, format, args…)` the entry then runs on that thread, or
//!   on a worker thread the run's scheduler starts ([`super::sched`]),
//!   appends one event `"kind: text"`. Without a recording, `trace!` only
//!   reads a thread-local flag.
//! * **Blocks.** An event emitted inside a block of the scheduler carries
//!   the block's index. Workers run blocks in an order the operating
//!   system decides, so [`Recording::take`] sorts each run of consecutive
//!   block events by block, keeping the order within a block: the result
//!   is the block-ordered sequence, the same at every `concurrency`. An
//!   entry emits its per-round state from the coordinating thread, after
//!   the round's merge, so rounds are delimited by events outside blocks.
//! * **Kinds.** One word per operation, shared by the passes of rmp #71:
//!   `level`, `frontier`, `round`, `relax`, `pop`, `push`, `visit`,
//!   `merge`, `move`, `path`, `label`, `colour`, `state`, `stop`.
//! * **Graphs.** [`TraceGraph`] builds a small graph in a fresh database
//!   under `/tmp/lpgram` (CLAUDE.md): node `i` holds `uid` `i` and edge
//!   `i` of the list holds `eid` `i`. It checks that dense index `i` is
//!   node `i` and that edge `i` has element identifier `n + i`, so traces
//!   read nodes by dense index and edges by element identifier, and a
//!   reference script can name both.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, PoisonError};

use crate::test_support::algo::{AllowAll, rows};
use crate::test_support::{OWNER, graph_dir, no_flush};
use crate::types::Value;
use crate::{Database, Options, Parameters, Session};

/// One event: its text and the block it was emitted in.
type Event = (String, Option<usize>);

/// The events of one recording, shared with its worker threads.
#[derive(Clone, Default)]
pub(crate) struct Sink(Arc<Mutex<Vec<Event>>>);

thread_local! {
    /// The sink of this thread and the block it runs, if recording.
    static CURRENT: RefCell<Option<(Sink, Option<usize>)>> = const { RefCell::new(None) };
}

/// Whether this thread records.
pub(crate) fn on() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}

/// Appends an event to this thread's recording, if any.
pub(crate) fn push(kind: &str, text: &str) {
    CURRENT.with(|c| {
        if let Some((sink, block)) = &*c.borrow() {
            sink.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((format!("{kind}: {text}"), *block));
        }
    });
}

/// This thread's sink, for the worker threads it starts.
pub(crate) fn current() -> Option<Sink> {
    CURRENT.with(|c| c.borrow().as_ref().map(|(s, _)| s.clone()))
}

/// Makes a worker thread record into `sink` (none: no recording).
pub(crate) fn adopt(sink: Option<Sink>) {
    CURRENT.with(|c| *c.borrow_mut() = sink.map(|s| (s, None)));
}

/// Marks this thread's next events as emitted in block `block`.
pub(crate) fn block(block: Option<usize>) {
    CURRENT.with(|c| {
        if let Some((_, b)) = &mut *c.borrow_mut() {
            *b = block;
        }
    });
}

/// A recording on the current thread, ended when dropped.
pub(crate) struct Recording(Sink);

impl Recording {
    /// Starts recording on this thread.
    pub(crate) fn start() -> Recording {
        let sink = Sink::default();
        adopt(Some(sink.clone()));
        Recording(sink)
    }

    /// The events so far, each run of block events in block order
    /// (module documentation), and clears them.
    pub(crate) fn take(&self) -> Vec<String> {
        let events = std::mem::take(&mut *self.0.0.lock().unwrap_or_else(PoisonError::into_inner));
        let mut out: Vec<String> = Vec::with_capacity(events.len());
        let mut run: Vec<(usize, String)> = Vec::new();
        for (text, b) in events {
            match b {
                Some(b) => run.push((b, text)),
                None => {
                    run.sort_by_key(|x| x.0);
                    out.extend(run.drain(..).map(|x| x.1));
                    out.push(text);
                }
            }
        }
        run.sort_by_key(|x| x.0);
        out.extend(run.into_iter().map(|x| x.1));
        out
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        adopt(None);
    }
}

/// The events of `got` that start with one of `kinds`.
pub(crate) fn only(got: &[String], kinds: &[&str]) -> Vec<String> {
    got.iter()
        .filter(|e| kinds.iter().any(|k| e.starts_with(&format!("{k}: "))))
        .cloned()
        .collect()
}

/// Asserts that `got` equals `want`, naming the first different step.
#[track_caller]
pub(crate) fn assert_steps(what: &str, got: &[String], want: &[&str]) {
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g, w, "{what}: step {i} differs\n got: {got:#?}");
    }
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: {} steps instead of {}\n got: {got:#?}",
        got.len(),
        want.len()
    );
}

/// A small graph for traces (module documentation).
pub(crate) struct TraceGraph {
    _dir: tempfile::TempDir,
    pub(crate) db: Database,
    pub(crate) s: Session,
    /// The element identifier of node `i`.
    pub(crate) ids: Vec<u64>,
}

/// An edge of a trace graph: source, target, type, `weight` (none: no
/// property).
pub(crate) type TraceEdge<'a> = (usize, usize, &'a str, Option<f64>);

impl TraceGraph {
    /// Nodes `(:N {uid: i})` and `edges`.
    pub(crate) async fn new(n: usize, edges: &[TraceEdge<'_>]) -> TraceGraph {
        let nodes: Vec<String> = (0..n).map(|_| ":N".to_string()).collect();
        let nodes: Vec<&str> = nodes.iter().map(String::as_str).collect();
        TraceGraph::with_nodes(&nodes, edges).await
    }

    /// Nodes whose labels and extra properties are `nodes[i]`, written as
    /// a GQL node filler without the variable and the closing brace of
    /// its properties (`":A"`, `":A {lat: 1e0,"`; floats in E notation), and `edges`.
    pub(crate) async fn with_nodes(nodes: &[&str], edges: &[TraceEdge<'_>]) -> TraceGraph {
        let dir = graph_dir();
        let db = Database::create(
            dir.path(),
            no_flush(),
            32 << 20,
            OWNER,
            Options::new(Arc::new(AllowAll)),
        )
        .await
        .expect("create");
        let s = db.session(OWNER, None, None).await.expect("session");
        let n = nodes.len();
        if n > 0 {
            let mut parts: Vec<String> = nodes
                .iter()
                .enumerate()
                .map(|(i, body)| match body.find('{') {
                    Some(_) => format!("(n{i}{body} uid: {i}}})"),
                    None => format!("(n{i}{body} {{uid: {i}}})"),
                })
                .collect();
            for (i, (a, b, t, w)) in edges.iter().enumerate() {
                parts.push(match w {
                    Some(w) => format!("(n{a})-[:{t} {{eid: {i}, weight: {w:e}}}]->(n{b})"),
                    None => format!("(n{a})-[:{t} {{eid: {i}}}]->(n{b})"),
                });
            }
            rows(
                &s,
                &format!("INSERT {}", parts.join(", ")),
                Parameters::new(),
            )
            .await
            .expect("insert the trace graph");
        }
        let mut ids = vec![0u64; n];
        for r in rows(&s, "MATCH (n) RETURN n, n.uid", Parameters::new())
            .await
            .expect("read the nodes")
        {
            let (Value::Node(id), Value::Int(uid)) = (&r[0], &r[1]) else {
                panic!("a node and its uid: {r:?}")
            };
            ids[usize::try_from(*uid).expect("a uid")] = *id;
        }
        assert!(
            ids.windows(2).all(|w| w[0] < w[1]),
            "dense index i must be node i: element identifiers {ids:?}"
        );
        // Traces name edges by element identifier: edge i must be n + i
        // when node i is i.
        for r in rows(&s, "MATCH ()-[e]->() RETURN e, e.eid", Parameters::new())
            .await
            .expect("read the edges")
        {
            let (Value::Edge(id), Value::Int(i)) = (&r[0], &r[1]) else {
                panic!("an edge and its eid: {r:?}")
            };
            assert_eq!(
                (*id, ids.iter().copied().eq(0..n as u64)),
                (n as u64 + i.cast_unsigned(), true),
                "edge {i} must have element identifier n + {i}"
            );
        }
        TraceGraph {
            _dir: dir,
            db,
            s,
            ids,
        }
    }

    /// The fixture `name` of `src/algo/testdata` as a trace graph, for
    /// list-valued properties, which `INSERT` cannot write (GV50; rmp #71
    /// pass B); node `i` must have element identifier `i`.
    pub(crate) async fn fixture(name: &str) -> TraceGraph {
        let f = crate::test_support::algo::Fixture::load(name, 32 << 20).await;
        assert!(
            f.ids.iter().copied().eq(0..f.ids.len() as u64),
            "node i must have element identifier i: {:?}",
            f.ids
        );
        TraceGraph {
            _dir: f._dir,
            db: f.db,
            s: f.s,
            ids: f.ids,
        }
    }

    /// The largest number of worker threads a run on this database
    /// started since the last call (the worker-count probe, [`super::sched`]).
    pub(crate) fn workers(&self) -> Option<usize> {
        super::sched::probe::take(
            &self
                .db
                .engine_handle()
                .core()
                .location
                .display()
                .to_string(),
        )
    }

    /// Node `i`.
    pub(crate) fn node(&self, i: usize) -> Value {
        Value::Node(self.ids[i])
    }

    /// The `uid` of a node value or element identifier.
    pub(crate) fn uid(&self, v: &Value) -> usize {
        let id = match v {
            Value::Node(id) => *id,
            Value::Int(id) => id.cast_unsigned(),
            other => panic!("not a node: {other:?}"),
        };
        self.ids.binary_search(&id).expect("a node of the graph")
    }

    /// The `uid`s of a list of nodes.
    pub(crate) fn uids(&self, v: &Value) -> Vec<usize> {
        match v {
            Value::List(l) => l.iter().map(|x| self.uid(x)).collect(),
            other => panic!("not a list: {other:?}"),
        }
    }

    /// The `uid`s of a path's nodes.
    pub(crate) fn path_uids(&self, v: &Value) -> Vec<usize> {
        match v {
            Value::Path(p) => p
                .iter()
                .step_by(2)
                .map(|id| self.uid(&Value::Node(*id)))
                .collect(),
            other => panic!("not a path: {other:?}"),
        }
    }

    /// The rows and the trace of `CALL proc(args…, config)`.
    ///
    /// # Panics
    ///
    /// If the call fails.
    pub(crate) async fn trace(
        &self,
        proc: &str,
        args: &[Value],
        config: &Value,
    ) -> (Vec<Vec<Value>>, Vec<String>) {
        let (rows, events) = self.try_trace(proc, args, config).await;
        (rows.unwrap_or_else(|e| panic!("{proc}: {e}")), events)
    }

    /// As [`TraceGraph::trace`], with the call's error.
    pub(crate) async fn try_trace(
        &self,
        proc: &str,
        args: &[Value],
        config: &Value,
    ) -> (Result<Vec<Vec<Value>>, crate::Error>, Vec<String>) {
        let (text, ps) = super::tests_paths::call_text(proc, args, config);
        let rec = Recording::start();
        let out = rows(&self.s, &text, ps).await;
        (out, rec.take())
    }
}
