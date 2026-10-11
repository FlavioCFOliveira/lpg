//! The project's micro-workloads (NFR-PERF-002; rmp #53): each a GQL text
//! with its expected result, run through the public API and checked. Their
//! timing is measured in Sprint 6 (user decision 2026-10-08, "Definir
//! agora, medir no S6", rmp #39 point 16); here PROFILE's counters are
//! recorded (rows, batches, entries read, buffer hits and misses, peak
//! memory), never its times.
//!
//! * **Layout** (`L1`–`L5`): the workloads of the M-LAYOUT harness
//!   (`docs/evidence/layout-bench/results.md`, "Workloads": 1-hop OUT over
//!   all types and over type 2, 2-hop and 3-hop distinct, a single-edge
//!   insert) on a graph from its generator ([`super::generator`]: Zipf
//!   endpoints, α = 0.8, four edge types). Expected results come from a
//!   reference evaluation over the generator's edge list, as M-LAYOUT's
//!   correctness gate compares 1-hop counts with the generator's degrees.
//!   Under `DIFFERENT EDGES` (ID086) a path never repeats an edge, so the
//!   reference counts the end nodes of trails.
//! * **SNB** (`IS1`–`IS7`): the LDBC SNB Interactive short reads, from the
//!   Cypher reference queries of ldbc_snb_interactive_v1_impls @ `11db98c`
//!   (<https://github.com/ldbc/ldbc_snb_interactive_v1_impls>,
//!   `cypher/queries/interactive-short-{1..7}.cypher`, Copyright 2020-2025
//!   Linked Data Benchmark Council, Apache-2.0, see `LICENSE-APACHE`),
//!   rewritten in the approved GQL subset over a small SNB-shaped graph.
//!   The changes, each forced by an unsupported feature: every edge pattern
//!   points right, `<-[ ]-` and `-[ ]-` being G043, so `KNOWS` is stored in
//!   both directions; `LIMIT` (GQ13) is dropped; `-[:REPLY_OF*0..]->`
//!   (G035) is dropped, so IS2 and IS6 take Posts, whose root post is
//!   themselves; `CASE r WHEN null …` becomes `r IS NOT NULL`. Expected
//!   results come from a reference evaluation over the graph's own data.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::generator::{Sampler, ZIPF_ALPHA};
use crate::auth::{Action, Authoriser, BoxFuture, Object};
use crate::sync::Arc;
use crate::test_support::{OWNER, graph_dir, no_flush};
use crate::types::Value;
use crate::{Database, Options, Parameters, PlanDescription, Session, Summary};

struct AllowAll;

impl Authoriser for AllowAll {
    fn authorise<'a>(
        &'a self,
        _principal: &'a str,
        _action: Action,
        _object: &'a Object,
        _owner: &'a str,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async { true })
    }
}

/// PROFILE's counters of a statement, summed over its operators (peak
/// memory: the largest operator's).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counters {
    rows: u64,
    batches: u64,
    entries_read: u64,
    hits: u64,
    misses: u64,
    peak_memory: u64,
}

fn sum(d: &PlanDescription, c: &mut Counters) {
    if let Some(p) = d.profile {
        c.batches += p.batches;
        c.entries_read += p.entries_read;
        c.hits += p.hits;
        c.misses += p.misses;
        c.peak_memory = c.peak_memory.max(p.peak_memory);
    }
    for ch in &d.children {
        sum(ch, c);
    }
}

/// Runs `gql` profiled; its rows and counters.
async fn profiled(
    s: &Session,
    gql: &str,
    params: Parameters,
) -> (Vec<Vec<Value>>, Counters, Summary) {
    let mut st = s
        .profile(gql, params)
        .await
        .unwrap_or_else(|e| panic!("{gql}: {e}"));
    let mut rows = Vec::new();
    while let Some(r) = st.next().await.unwrap_or_else(|e| panic!("{gql}: {e}")) {
        rows.push(r);
    }
    let summary = st.summary().cloned().expect("a summary");
    let mut c = Counters {
        rows: rows.len() as u64,
        ..Counters::default()
    };
    if let Some(d) = &summary.profile {
        sum(d, &mut c);
    }
    (rows, c, summary)
}

fn param(pairs: &[(&str, Value)]) -> Parameters {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// One workload's record for the report.
struct Record {
    id: &'static str,
    gql: String,
    expected: String,
    counters: Counters,
}

/// Writes the records to `LPG_MICRO_OUT` when set, and prints them.
fn publish(records: &[Record]) {
    let mut out = String::from(
        "id\tgql\texpected\trows\tbatches\tentries_read\tbuffer_hits\tbuffer_misses\tpeak_memory\n",
    );
    for r in records {
        let c = r.counters;
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.id,
            r.gql.replace('\n', " "),
            r.expected,
            c.rows,
            c.batches,
            c.entries_read,
            c.hits,
            c.misses,
            c.peak_memory
        );
    }
    print!("{out}");
    if let Some(path) = std::env::var_os("LPG_MICRO_OUT") {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open LPG_MICRO_OUT");
        f.write_all(out.as_bytes()).expect("write LPG_MICRO_OUT");
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a current_thread runtime")
}

async fn open(dir: &std::path::Path) -> (Database, Session) {
    let db = Database::create(
        dir,
        no_flush(),
        64 << 20,
        OWNER,
        Options::new(Arc::new(AllowAll)),
    )
    .await
    .expect("create");
    let s = db.session(OWNER, None, None).await.expect("session");
    (db, s)
}

// ---------------------------------------------------------------------
// Layout workloads.

/// Nodes and edges of the layout graph.
const NODES: u64 = 200;
const EDGES: u64 = 1_000;
const SEED: u64 = 20_261_011;

/// The generator's edges `(src, dst, type)`, OUT direction resolved.
fn layout_edges() -> Vec<(u64, u64, u32)> {
    let mut s = Sampler::zipf(NODES, ZIPF_ALPHA, SEED);
    (0..EDGES)
        .map(|e| {
            let (node, dir, ty, nbr, _) = s.adj_key(e);
            if dir == 0 {
                (node, nbr, ty)
            } else {
                (nbr, node, ty)
            }
        })
        .collect()
}

/// End nodes of the trails of `len` edges from `start` (no edge twice).
fn trail_ends(edges: &[(u64, u64, u32)], start: u64, len: usize) -> BTreeSet<u64> {
    fn walk(
        edges: &[(u64, u64, u32)],
        at: u64,
        left: usize,
        used: &mut Vec<usize>,
        out: &mut BTreeSet<u64>,
    ) {
        if left == 0 {
            out.insert(at);
            return;
        }
        for (i, e) in edges.iter().enumerate() {
            if e.0 == at && !used.contains(&i) {
                used.push(i);
                walk(edges, e.1, left - 1, used, out);
                used.pop();
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(edges, start, len, &mut Vec::new(), &mut out);
    out
}

#[test]
fn layout_micro_workloads_return_their_expected_results() {
    let edges = layout_edges();
    let mut degree = vec![0u64; NODES as usize];
    for e in &edges {
        degree[e.0 as usize] += 1;
    }
    // The hub (highest out-degree, lowest id on ties) and a node of
    // out-degree 1 as starts, as M-LAYOUT's degree-weighted and uniform
    // starts.
    let hub = (0..NODES)
        .max_by_key(|n| (degree[*n as usize], std::cmp::Reverse(*n)))
        .expect("nodes");
    let low = (0..NODES)
        .find(|n| degree[*n as usize] == 1)
        .expect("a node of out-degree 1");
    let dir = graph_dir();
    runtime().block_on(async {
        let (db, s) = open(dir.path()).await;
        let mut load = String::from("INSERT ");
        let mut items: Vec<String> = (0..NODES)
            .map(|n| format!("(n{n}:N {{id: {n}}})"))
            .collect();
        items.extend(
            edges
                .iter()
                .map(|(a, b, t)| format!("(n{a})-[:T{t}]->(n{b})")),
        );
        load.push_str(&items.join(", "));
        let (_, c, _) = profiled(&s, &load, Parameters::new()).await;
        assert_eq!(c.rows, 0);
        let mut records = Vec::new();
        let count = |rows: &[Vec<Value>]| match rows {
            [r] => match r.as_slice() {
                [Value::Int(i)] => *i,
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };
        for (start, tag) in [(hub, "hub"), (low, "low")] {
            let p = || param(&[("s", Value::Int(i64::try_from(start).expect("small")))]);
            let cases: [(&'static str, &str, i64); 4] = [
                (
                    "L1",
                    "MATCH (a:N {id: $s})-[]->(b) RETURN COUNT(*) AS c",
                    i64::try_from(degree[start as usize]).expect("small"),
                ),
                (
                    "L2",
                    "MATCH (a:N {id: $s})-[:T2]->(b) RETURN COUNT(*) AS c",
                    edges.iter().filter(|e| e.0 == start && e.2 == 2).count() as i64,
                ),
                (
                    "L3",
                    "MATCH (a:N {id: $s})-[]->()-[]->(c) RETURN COUNT(DISTINCT c) AS c",
                    trail_ends(&edges, start, 2).len() as i64,
                ),
                (
                    "L4",
                    "MATCH (a:N {id: $s})-[]->()-[]->()-[]->(c) RETURN COUNT(DISTINCT c) AS c",
                    trail_ends(&edges, start, 3).len() as i64,
                ),
            ];
            for (id, gql, want) in cases {
                let (rows, c, _) = profiled(&s, gql, p()).await;
                assert_eq!(count(&rows), want, "{id} {tag}: {gql}");
                records.push(Record {
                    id: if tag == "hub" { id } else { lowered(id) },
                    gql: format!("{gql}  -- $s = {start}"),
                    expected: format!("c = {want}"),
                    counters: c,
                });
            }
        }
        // L5: one edge inserted between the two starts, then counted.
        let p = param(&[
            ("s", Value::Int(i64::try_from(low).expect("small"))),
            ("t", Value::Int(i64::try_from(hub).expect("small"))),
        ]);
        let gql = "MATCH (a:N {id: $s}), (b:N {id: $t}) INSERT (a)-[:T0]->(b)";
        let (_, c, summary) = profiled(&s, gql, p.clone()).await;
        assert_eq!(summary.counters.edges_created, 1);
        let (rows, _, _) =
            profiled(&s, "MATCH (a:N {id: $s})-[]->(b) RETURN COUNT(*) AS c", p).await;
        assert_eq!(count(&rows), 2, "the inserted edge is visible");
        records.push(Record {
            id: "L5",
            gql: format!("{gql}  -- $s = {low}, $t = {hub}"),
            expected: "1 edge created; L1 from $s then 2".into(),
            counters: c,
        });
        publish(&records);
        drop(s);
        db.close().await.expect("close");
    });
}

/// The record id of a workload run from the low-degree start.
fn lowered(id: &'static str) -> &'static str {
    match id {
        "L1" => "L1-low",
        "L2" => "L2-low",
        "L3" => "L3-low",
        _ => "L4-low",
    }
}

// ---------------------------------------------------------------------
// LDBC SNB Interactive short reads.

struct Person {
    id: i64,
    first: &'static str,
    last: &'static str,
    gender: &'static str,
    birthday: &'static str,
    ip: &'static str,
    browser: &'static str,
    created: i64,
    city: i64,
}

struct Message {
    id: i64,
    post: bool,
    content: Option<&'static str>,
    image: Option<&'static str>,
    created: i64,
    creator: i64,
    /// The message it replies to (comments).
    reply_of: Option<i64>,
}

const PERSONS: [Person; 4] = [
    Person {
        id: 1,
        first: "Ana",
        last: "Silva",
        gender: "female",
        birthday: "1989-12-03",
        ip: "10.0.0.1",
        browser: "Firefox",
        created: 100,
        city: 10,
    },
    Person {
        id: 2,
        first: "Bo",
        last: "Lind",
        gender: "male",
        birthday: "1991-04-20",
        ip: "10.0.0.2",
        browser: "Chrome",
        created: 110,
        city: 11,
    },
    Person {
        id: 3,
        first: "Cy",
        last: "Ng",
        gender: "male",
        birthday: "1985-07-07",
        ip: "10.0.0.3",
        browser: "Safari",
        created: 120,
        city: 10,
    },
    Person {
        id: 4,
        first: "Di",
        last: "Roy",
        gender: "female",
        birthday: "1993-01-15",
        ip: "10.0.0.4",
        browser: "Opera",
        created: 130,
        city: 12,
    },
];
const CITIES: [i64; 3] = [10, 11, 12];
/// `(a, b, creationDate)`: an undirected friendship.
const KNOWS: [(i64, i64, i64); 4] = [(1, 2, 200), (1, 3, 210), (2, 4, 220), (3, 4, 205)];
const MESSAGES: [Message; 6] = [
    Message {
        id: 100,
        post: true,
        content: Some("first post"),
        image: None,
        created: 300,
        creator: 1,
        reply_of: None,
    },
    Message {
        id: 101,
        post: true,
        content: None,
        image: Some("photo.jpg"),
        created: 310,
        creator: 1,
        reply_of: None,
    },
    Message {
        id: 102,
        post: true,
        content: Some("bo writes"),
        image: None,
        created: 305,
        creator: 2,
        reply_of: None,
    },
    Message {
        id: 200,
        post: false,
        content: Some("reply by bo"),
        image: None,
        created: 320,
        creator: 2,
        reply_of: Some(100),
    },
    Message {
        id: 201,
        post: false,
        content: Some("reply by di"),
        image: None,
        created: 330,
        creator: 4,
        reply_of: Some(100),
    },
    Message {
        id: 202,
        post: false,
        content: Some("reply to a reply"),
        image: None,
        created: 340,
        creator: 3,
        reply_of: Some(200),
    },
];
/// `(forum id, title, moderator, posts)`.
const FORUMS: [(i64, &str, i64, &[i64]); 2] = [
    (50, "Wall of Ana", 1, &[100, 101]),
    (51, "Wall of Bo", 2, &[102]),
];

fn s(x: &str) -> Value {
    Value::string(x)
}

fn opt(x: Option<&str>) -> Value {
    x.map_or(Value::Null, s)
}

/// The INSERT statement of the SNB graph.
fn snb_insert() -> String {
    let lit = |x: Option<&str>| x.map_or("NULL".to_string(), |v| format!("'{v}'"));
    let mut items = Vec::new();
    for c in CITIES {
        items.push(format!("(c{c}:City {{id: {c}}})"));
    }
    for p in &PERSONS {
        items.push(format!(
            "(p{}:Person {{id: {}, firstName: '{}', lastName: '{}', gender: '{}', birthday: DATE '{}', \
             locationIP: '{}', browserUsed: '{}', creationDate: {}}})",
            p.id, p.id, p.first, p.last, p.gender, p.birthday, p.ip, p.browser, p.created
        ));
        items.push(format!("(p{})-[:IS_LOCATED_IN]->(c{})", p.id, p.city));
    }
    for (a, b, d) in KNOWS {
        items.push(format!("(p{a})-[:KNOWS {{creationDate: {d}}}]->(p{b})"));
        items.push(format!("(p{b})-[:KNOWS {{creationDate: {d}}}]->(p{a})"));
    }
    for m in &MESSAGES {
        let kind = if m.post { "Post" } else { "Comment" };
        items.push(format!(
            "(m{}:Message&{kind} {{id: {}, content: {}, imageFile: {}, creationDate: {}}})",
            m.id,
            m.id,
            lit(m.content),
            lit(m.image),
            m.created
        ));
        items.push(format!("(m{})-[:HAS_CREATOR]->(p{})", m.id, m.creator));
        if let Some(r) = m.reply_of {
            items.push(format!("(m{})-[:REPLY_OF]->(m{r})", m.id));
        }
    }
    for (f, title, moderator, posts) in FORUMS {
        items.push(format!("(f{f}:Forum {{id: {f}, title: '{title}'}})"));
        items.push(format!("(f{f})-[:HAS_MODERATOR]->(p{moderator})"));
        for m in posts {
            items.push(format!("(f{f})-[:CONTAINER_OF]->(m{m})"));
        }
    }
    format!("INSERT {}", items.join(",\n       "))
}

fn person(id: i64) -> &'static Person {
    PERSONS.iter().find(|p| p.id == id).expect("a person")
}

fn message(id: i64) -> &'static Message {
    MESSAGES.iter().find(|m| m.id == id).expect("a message")
}

fn knows(a: i64, b: i64) -> Option<i64> {
    KNOWS
        .iter()
        .find(|k| (k.0, k.1) == (a, b) || (k.0, k.1) == (b, a))
        .map(|k| k.2)
}

fn date(text: &str) -> Value {
    Value::Date(text.parse().expect("a date"))
}

/// Each short read: id, GQL text, parameters, reference rows.
fn short_reads() -> Vec<(&'static str, &'static str, Parameters, Vec<Vec<Value>>)> {
    let mut out = Vec::new();
    // IS1: profile of a person.
    let p = person(3);
    out.push((
        "IS1",
        "MATCH (n:Person {id: $personId})-[:IS_LOCATED_IN]->(p:City)\n\
         RETURN n.firstName AS firstName, n.lastName AS lastName, n.birthday AS birthday,\n\
         n.locationIP AS locationIP, n.browserUsed AS browserUsed, p.id AS cityId,\n\
         n.gender AS gender, n.creationDate AS creationDate",
        param(&[("personId", Value::Int(3))]),
        vec![vec![
            s(p.first),
            s(p.last),
            date(p.birthday),
            s(p.ip),
            s(p.browser),
            Value::Int(p.city),
            s(p.gender),
            Value::Int(p.created),
        ]],
    ));
    // IS2: recent messages (posts) of a person.
    let mut posts: Vec<&Message> = MESSAGES
        .iter()
        .filter(|m| m.post && m.creator == 1)
        .collect();
    posts.sort_by_key(|m| (std::cmp::Reverse(m.created), m.id));
    let author = person(1);
    out.push((
        "IS2",
        "MATCH (message:Post)-[:HAS_CREATOR]->(person:Person {id: $personId})\n\
         RETURN message.id AS messageId,\n\
         COALESCE(message.imageFile, message.content) AS messageContent,\n\
         message.creationDate AS messageCreationDate, message.id AS postId,\n\
         person.id AS personId, person.firstName AS personFirstName,\n\
         person.lastName AS personLastName\n\
         ORDER BY messageCreationDate DESC, messageId ASC",
        param(&[("personId", Value::Int(1))]),
        posts
            .iter()
            .map(|m| {
                vec![
                    Value::Int(m.id),
                    opt(m.image.or(m.content)),
                    Value::Int(m.created),
                    Value::Int(m.id),
                    Value::Int(author.id),
                    s(author.first),
                    s(author.last),
                ]
            })
            .collect(),
    ));
    // IS3: friends of a person.
    let mut friends: Vec<(i64, i64)> = PERSONS
        .iter()
        .filter_map(|f| knows(4, f.id).filter(|_| f.id != 4).map(|d| (f.id, d)))
        .collect();
    friends.sort_by_key(|(id, d)| (std::cmp::Reverse(*d), *id));
    out.push((
        "IS3",
        "MATCH (n:Person {id: $personId})-[r:KNOWS]->(friend:Person)\n\
         RETURN friend.id AS personId, friend.firstName AS firstName,\n\
         friend.lastName AS lastName, r.creationDate AS friendshipCreationDate\n\
         ORDER BY friendshipCreationDate DESC, personId ASC",
        param(&[("personId", Value::Int(4))]),
        friends
            .iter()
            .map(|(id, d)| {
                let f = person(*id);
                vec![Value::Int(f.id), s(f.first), s(f.last), Value::Int(*d)]
            })
            .collect(),
    ));
    // IS4: content of a message.
    let m = message(101);
    out.push((
        "IS4",
        "MATCH (m:Message {id: $messageId})\n\
         RETURN m.creationDate AS messageCreationDate,\n\
         COALESCE(m.content, m.imageFile) AS messageContent",
        param(&[("messageId", Value::Int(101))]),
        vec![vec![Value::Int(m.created), opt(m.content.or(m.image))]],
    ));
    // IS5: creator of a message.
    let c = person(message(202).creator);
    out.push((
        "IS5",
        "MATCH (m:Message {id: $messageId})-[:HAS_CREATOR]->(p:Person)\n\
         RETURN p.id AS personId, p.firstName AS firstName, p.lastName AS lastName",
        param(&[("messageId", Value::Int(202))]),
        vec![vec![Value::Int(c.id), s(c.first), s(c.last)]],
    ));
    // IS6: forum of a message (a post).
    let (fid, title, moderator, _) = FORUMS.iter().find(|f| f.3.contains(&102)).expect("a forum");
    let mo = person(*moderator);
    out.push((
        "IS6",
        "MATCH (f:Forum)-[:CONTAINER_OF]->(p:Post {id: $messageId}),\n\
         (f)-[:HAS_MODERATOR]->(moderator:Person)\n\
         RETURN f.id AS forumId, f.title AS forumTitle, moderator.id AS moderatorId,\n\
         moderator.firstName AS moderatorFirstName, moderator.lastName AS moderatorLastName",
        param(&[("messageId", Value::Int(102))]),
        vec![vec![
            Value::Int(*fid),
            s(title),
            Value::Int(mo.id),
            s(mo.first),
            s(mo.last),
        ]],
    ));
    // IS7: replies of a message.
    let original = message(100);
    let mut replies: Vec<&Message> = MESSAGES
        .iter()
        .filter(|r| r.reply_of == Some(100))
        .collect();
    replies.sort_by_key(|r| (std::cmp::Reverse(r.created), r.creator));
    out.push((
        "IS7",
        "MATCH (c:Comment)-[:REPLY_OF]->(m:Message {id: $messageId}),\n\
         (c)-[:HAS_CREATOR]->(p:Person)\n\
         OPTIONAL MATCH (m)-[:HAS_CREATOR]->(a:Person)-[r:KNOWS]->(p)\n\
         RETURN c.id AS commentId, c.content AS commentContent,\n\
         c.creationDate AS commentCreationDate, p.id AS replyAuthorId,\n\
         p.firstName AS replyAuthorFirstName, p.lastName AS replyAuthorLastName,\n\
         r IS NOT NULL AS replyAuthorKnowsOriginalMessageAuthor\n\
         ORDER BY commentCreationDate DESC, replyAuthorId",
        param(&[("messageId", Value::Int(100))]),
        replies
            .iter()
            .map(|r| {
                let a = person(r.creator);
                vec![
                    Value::Int(r.id),
                    opt(r.content),
                    Value::Int(r.created),
                    Value::Int(a.id),
                    s(a.first),
                    s(a.last),
                    Value::Bool(knows(original.creator, a.id).is_some()),
                ]
            })
            .collect(),
    ));
    out
}

#[test]
fn snb_short_reads_return_their_expected_results() {
    let dir = graph_dir();
    runtime().block_on(async {
        let (db, s) = open(dir.path()).await;
        profiled(&s, &snb_insert(), Parameters::new()).await;
        let mut records = Vec::new();
        for (id, gql, params, want) in short_reads() {
            assert!(!want.is_empty(), "{id}: a non-empty reference result");
            let (rows, c, _) = profiled(&s, gql, params.clone()).await;
            assert_eq!(rows, want, "{id}: {gql}");
            let shown: BTreeMap<_, _> = params.iter().collect();
            records.push(Record {
                id,
                gql: format!("{gql}  -- {shown:?}"),
                expected: format!("{want:?}"),
                counters: c,
            });
        }
        publish(&records);
        drop(s);
        db.close().await.expect("close");
    });
}
