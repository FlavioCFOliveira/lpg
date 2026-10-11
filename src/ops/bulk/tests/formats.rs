//! Round trips of the four formats by both import modes (rmp #59), and
//! the FR-IMP-006 acceptance cases.

use super::*;

const FORMATS: [Format; 4] = [
    Format::Dump,
    Format::Csv,
    Format::JsonLines,
    Format::GraphMl,
];
const MODES: [ImportMode; 2] = [ImportMode::Transactional, ImportMode::FastLoad];

/// One generated graph through every format and both modes.
fn round_trip(seed: u64, nodes: usize, edges: usize) {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let (ns, es) = property_graph(seed, nodes, edges, true);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    let want = content(&e, DEFAULT_GRAPH_ID);
    for (i, format) in FORMATS.into_iter().enumerate() {
        let names = export_graph(&e, DEFAULT_GRAPH_ID, format, &dir, &format!("g{i}"));
        for (j, mode) in MODES.into_iter().enumerate() {
            let name = format!("t{i}{j}");
            let g = new_graph(&e, &name);
            let mut req = request(
                format,
                &format!("/public/{name}"),
                &dir,
                &names,
                &format!("r{i}{j}"),
            );
            req.mode = mode;
            let what = format!("seed {seed} {format:?} {mode:?}");
            let s = import(&e, &req).unwrap_or_else(|err| panic!("{what}: {err}"));
            assert_eq!(
                (s.nodes, s.edges, s.invalid),
                (nodes as u64, edges as u64, 0),
                "{what}"
            );
            assert_same(&want, &content(&e, g), &what);
        }
    }
    assert_intact(&e, &format!("seed {seed}"));
}

#[test]
fn a_generated_graph_round_trips_through_every_format_and_mode() {
    round_trip(1, 12, 20);
}

/// rmp #59 acceptance: 200 generated graphs (100 fixed seeds, then 100
/// from `LPG_SEED` or a random seed) with every property type and
/// mixed-type properties, of 0 to 15 nodes and 0 to 25 edges, round-trip
/// through each of the four formats by transactional import and by fast
/// load with the same nodes, edges, labels and property values (floats
/// bit-equal, NaN for NaN). The graphs run on four threads, each with its
/// own database.
#[test]
fn two_hundred_generated_graphs_round_trip_through_every_format_and_mode() {
    let base = crate::test_support::seed();
    let seeds: Vec<u64> = (0..100)
        .chain((0..100).map(|i| base.wrapping_add(i)))
        .collect();
    std::thread::scope(|sc| {
        for chunk in seeds.chunks(50) {
            sc.spawn(move || {
                for &seed in chunk {
                    let n = (seed % 16) as usize;
                    let m = if n == 0 { 0 } else { (seed / 16 % 26) as usize };
                    round_trip(seed, n, m);
                }
            });
        }
    });
}

/// A graph of one node with properties `props`, exported to `format`;
/// returns the database, the export's text (the node file for CSV) and
/// the property values the node has after a transactional import of it.
fn one_node(format: Format, props: &[(&str, Value)]) -> (String, Vec<(String, Value)>) {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let nodes = vec![GenNode {
        labels: vec!["A".into()],
        props: std::iter::once(("uid".to_owned(), Value::Int(0)))
            .chain(props.iter().map(|(k, v)| ((*k).to_owned(), v.clone())))
            .collect(),
    }];
    load_generated(&e, DEFAULT_GRAPH_ID, &nodes, &[]);
    let names = export_graph(&e, DEFAULT_GRAPH_ID, format, &dir, "one");
    let text = std::fs::read_to_string(dir.join(&names[0])).unwrap();
    let g = new_graph(&e, "back");
    import(&e, &request(format, "/public/back", &dir, &names, "r")).unwrap();
    (text, content(&e, g).nodes.remove(0).2)
}

fn prop<'a>(props: &'a [(String, Value)], k: &str) -> &'a Value {
    &props.iter().find(|(n, _)| n == k).unwrap().1
}

/// FR-IMP-006 acceptance case 1: a point exported to GraphML is a string
/// with the lpg type attribute, and reads back as the point.
#[test]
fn fr_imp_006_a_point_to_graphml() {
    let p = crate::test_support::values::point(crate::types::Srid::Wgs84_3D, &[1.5, -2.0, 3.25]);
    let (text, back) = one_node(Format::GraphMl, &[("loc", p.clone())]);
    assert!(
        text.contains(r#"attr.name="loc" attr.type="string" lpg:type="POINT""#),
        "{text}"
    );
    assert!(text.contains("{srid: 4979, x: 1.5, y: -2.0, z: 3.25}"));
    assert!(bit_eq(prop(&back, "loc"), &p));
}

/// FR-IMP-006 acceptance case 2: a date exported to JSON Lines is a
/// tagged object, and reads back as the date.
#[test]
fn fr_imp_006_a_date_to_json_lines() {
    let d = Value::Date(jiff::civil::Date::new(2015, 7, 21).unwrap());
    let (text, back) = one_node(Format::JsonLines, &[("day", d.clone())]);
    assert!(
        text.contains(r#""day":{"$lpg_type":"DATE","$value":"2015-07-21"}"#),
        "{text}"
    );
    assert_eq!(prop(&back, "day"), &d);
}

/// FR-IMP-006 acceptance case 3: a type Neo4j's header types lack (a
/// `DURATION(DAY TO SECOND)`) exported to CSV has an `lpg:` column, and
/// reads back with its type.
#[test]
fn fr_imp_006_a_non_neo4j_type_to_csv() {
    let v = Value::DayToSecond(crate::types::DayToSecond::new(93_784, 500_000_000).unwrap());
    let (text, back) = one_node(Format::Csv, &[("span", v.clone())]);
    assert!(text.contains("span:lpg:DURATION(DAY TO SECOND)"), "{text}");
    assert!(text.contains("P1DT2H3M4.5S"));
    assert_eq!(prop(&back, "span"), &v);
}

/// FR-IMP-006 acceptance case 4: NaN exported to JSON Lines, which JSON
/// numbers cannot hold, is a tagged object, and reads back as NaN.
#[test]
fn fr_imp_006_nan_to_json_lines() {
    let (text, back) = one_node(Format::JsonLines, &[("x", Value::Float(f64::NAN))]);
    assert!(
        text.contains(r#""x":{"$lpg_type":"FLOAT","$value":"NaN"}"#),
        "{text}"
    );
    assert!(matches!(prop(&back, "x"), Value::Float(x) if x.is_nan()));
}

/// FR-IMP-006 acceptance case 5: an integer and a string on one property
/// export to GraphML as a string key with the type on each value, and to
/// CSV as an `lpg:annotated` column; both read back with their types.
#[test]
fn fr_imp_006_an_integer_and_a_string_on_one_property_to_graphml_and_csv() {
    for format in [Format::GraphMl, Format::Csv] {
        let (d, e) = crate::tx::tests::db();
        let dir = files_dir(&d, "x");
        let nodes: Vec<GenNode> = [Value::Int(42), Value::string("42")]
            .into_iter()
            .enumerate()
            .map(|(i, v)| GenNode {
                labels: vec![],
                props: vec![("uid".into(), Value::Int(i as i64)), ("v".into(), v)],
            })
            .collect();
        load_generated(&e, DEFAULT_GRAPH_ID, &nodes, &[]);
        let want = content(&e, DEFAULT_GRAPH_ID);
        let names = export_graph(&e, DEFAULT_GRAPH_ID, format, &dir, "m");
        let text = std::fs::read_to_string(dir.join(&names[0])).unwrap();
        match format {
            Format::GraphMl => {
                assert!(
                    text.contains(r#"attr.name="v" attr.type="string"/>"#),
                    "{text}"
                );
                assert!(text.contains(r#"lpg:type="INTEGER">42<"#), "{text}");
                assert!(
                    text.contains(r#"lpg:type="STRING">&apos;42&apos;<"#),
                    "{text}"
                );
            }
            _ => {
                assert!(text.contains("v:lpg:annotated"), "{text}");
                assert!(
                    text.contains("INTEGER:42") && text.contains("STRING:'42'"),
                    "{text}"
                );
            }
        }
        let g = new_graph(&e, "back");
        import(&e, &request(format, "/public/back", &dir, &names, "r")).unwrap();
        assert_same(&want, &content(&e, g), &format!("{format:?}"));
    }
}

/// FR-IMP-006 acceptance case 6: a GraphML reader that ignores the lpg
/// namespace reads an annotated value as a string, its lexical form: the
/// export with every `lpg:type` attribute removed imports with strings.
#[test]
fn fr_imp_006_graphml_read_with_the_lpg_namespace_ignored() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let p = crate::test_support::values::point(crate::types::Srid::Cartesian2D, &[1.0, 2.0]);
    let nodes = vec![GenNode {
        labels: vec![],
        props: vec![
            ("uid".into(), Value::Int(0)),
            ("loc".into(), p),
            ("n".into(), Value::Float(f64::NAN)),
        ],
    }];
    load_generated(&e, DEFAULT_GRAPH_ID, &nodes, &[]);
    let names = export_graph(&e, DEFAULT_GRAPH_ID, Format::GraphMl, &dir, "g");
    let text = std::fs::read_to_string(dir.join(&names[0])).unwrap();
    let mut stripped = String::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find(" lpg:type=\"") {
        stripped.push_str(&rest[..i]);
        let after = &rest[i + " lpg:type=\"".len()..];
        rest = &after[after.find('"').unwrap() + 1..];
    }
    stripped.push_str(rest);
    std::fs::write(dir.join("plain.graphml"), stripped).unwrap();
    let g = new_graph(&e, "back");
    import(
        &e,
        &request(
            Format::GraphMl,
            "/public/back",
            &dir,
            &["plain.graphml".into()],
            "r",
        ),
    )
    .unwrap();
    let back = content(&e, g).nodes.remove(0).2;
    assert_eq!(
        prop(&back, "loc"),
        &Value::string("{srid: 7203, x: 1.0, y: 2.0}")
    );
    assert_eq!(prop(&back, "n"), &Value::string("NaN"));
}

/// Writes lpg exports of generated graphs for the external checks of the
/// laboratory (rmp #59; `docs/evidence/lab-sprint-4/`): into
/// `$LPG_EXPORT_DIR/neo4j`, a CSV export whose properties all have Neo4j
/// column types (no `lpg:` column), for `neo4j_import_check.sh`; into
/// `$LPG_EXPORT_DIR/graphml`, a GraphML export of a graph with every
/// property type and mixed properties, for `graphml_check.py`; into
/// `$LPG_EXPORT_DIR/seeds`, that graph as a dump, JSON Lines and CSV, seed
/// inputs of the reader fuzz targets. Run on
/// request: `LPG_EXPORT_DIR=… cargo nextest run --run-ignored only
/// write_samples_for_the_external_checks`.
#[test]
#[ignore = "writes files for the external laboratory checks; needs LPG_EXPORT_DIR"]
fn write_samples_for_the_external_checks() {
    use crate::types::lexical::TypeName;
    let out = PathBuf::from(std::env::var_os("LPG_EXPORT_DIR").expect("LPG_EXPORT_DIR"));
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    // Neo4j: the kinds whose uniform columns are Neo4j types, strings never
    // empty and holding no control character.
    let (mut ns, mut es) = property_graph(21, 40, 60, false);
    let keep = |props: &mut Vec<(String, Value)>| {
        props.retain(|(_, v)| {
            let t = TypeName::of(v);
            let neo = |t: &TypeName| {
                matches!(
                    t,
                    TypeName::Boolean
                        | TypeName::Integer
                        | TypeName::Float
                        | TypeName::String
                        | TypeName::Date
                        | TypeName::LocalTime
                        | TypeName::ZonedTime
                        | TypeName::LocalDateTime
                        | TypeName::ZonedDateTime
                        | TypeName::Point
                )
            };
            // Neo4j 2026.09.0 refuses a point with a non-finite coordinate
            // ("Cannot create a point with non-finite coordinate values",
            // measured 2026-10-11 by this check; rmp #59 FINDING).
            let scalar = |v: &Value| match v {
                Value::String(s) => !s.is_empty() && !s.chars().any(char::is_control),
                Value::Point(p) => p.coords().iter().all(|c| c.is_finite()),
                _ => true,
            };
            match (&t, v) {
                (TypeName::List(Some(inner)), Value::List(items)) => {
                    neo(inner) && items.iter().all(scalar)
                }
                _ => neo(&t) && scalar(v),
            }
        });
    };
    for (i, n) in ns.iter_mut().enumerate() {
        keep(&mut n.props);
        // Lists of the generator's list kind vary in element type between
        // nodes, a mixed column; a uniform `long[]` column instead.
        n.props.retain(|(k, _)| k != "p13");
        n.props.push((
            "ints".into(),
            Value::List(vec![Value::Int(i as i64), Value::Int(-1)].into()),
        ));
    }
    for ed in &mut es {
        keep(&mut ed.props);
        ed.props.retain(|(k, _)| k != "p13");
    }
    let g = new_graph(&e, "neo4j");
    load_generated(&e, g, &ns, &es);
    let names = export_graph(&e, g, Format::Csv, &dir, "neo4j");
    let neo = out.join("neo4j");
    std::fs::create_dir_all(&neo).unwrap();
    std::fs::copy(dir.join(&names[0]), neo.join("nodes.csv")).unwrap();
    std::fs::copy(dir.join(&names[1]), neo.join("edges.csv")).unwrap();
    let text = std::fs::read_to_string(neo.join("nodes.csv")).unwrap();
    assert!(
        !text.lines().next().unwrap().contains(":lpg:"),
        "an lpg-only column"
    );
    // GraphML: every property type, mixed properties.
    let (ns, es) = property_graph(22, 40, 60, true);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    let names = export_graph(&e, DEFAULT_GRAPH_ID, Format::GraphMl, &dir, "all");
    let gm = out.join("graphml");
    std::fs::create_dir_all(&gm).unwrap();
    std::fs::copy(dir.join(&names[0]), gm.join("graph.graphml")).unwrap();
    // Seed inputs of the reader fuzz targets: the same graph in the other
    // formats.
    let seeds = out.join("seeds");
    std::fs::create_dir_all(&seeds).unwrap();
    for (format, stem) in [
        (Format::Dump, "dump"),
        (Format::JsonLines, "jsonl"),
        (Format::Csv, "csv"),
    ] {
        for n in export_graph(&e, DEFAULT_GRAPH_ID, format, &dir, stem) {
            std::fs::copy(dir.join(&n), seeds.join(&n)).unwrap();
        }
    }
}

/// The fuzz entry points of the readers (rmp #57, #59) on exports and on
/// every one-byte truncation and a flip of each byte of their first 512
/// bytes: none panics; the dump reader reads an export whole and fails on
/// every damaged copy.
#[test]
fn the_readers_never_panic_on_damaged_exports() {
    let (d, e) = crate::tx::tests::db();
    let dir = files_dir(&d, "x");
    let (ns, es) = property_graph(31, 6, 8, true);
    load_generated(&e, DEFAULT_GRAPH_ID, &ns, &es);
    for (i, format) in FORMATS.into_iter().enumerate() {
        let names = export_graph(&e, DEFAULT_GRAPH_ID, format, &dir, &format!("f{i}"));
        let good = std::fs::read(dir.join(&names[0])).unwrap();
        let read = |b: &[u8]| match format {
            Format::Dump => dump::read_all(b).map_or(0, |n| n + 1),
            Format::Csv => csv::read_all(b),
            Format::JsonLines => jsonl::read_all(b),
            Format::GraphMl => graphml::read_all(b),
        };
        let full = read(&good);
        assert!(full > 0, "{format:?}");
        let mut damaged: Vec<Vec<u8>> = (0..good.len().min(512))
            .map(|n| good[..n].to_vec())
            .collect();
        for at in 0..good.len().min(512) {
            let mut b = good.clone();
            b[at] ^= 0x5A;
            damaged.push(b);
        }
        for b in &damaged {
            let n = read(b);
            if format == Format::Dump {
                assert_eq!(n, 0, "a damaged dump read");
            }
        }
    }
}

/// Unverified point 4, correctness part (rmp #58, plan §1.3): readers run
/// during a fast load of a graph with properties whose node tree has at
/// least three levels; before the install they read the empty graph,
/// after it the whole graph; a snapshot taken before the install still
/// reads it empty; the integrity check passes.
#[test]
fn readers_during_a_load_of_a_three_level_graph_see_it_empty_then_whole() {
    let (d, e) = crate::tx::tests::db_with_cap(64 << 20);
    let dir = files_dir(&d, "x");
    let mut text = String::new();
    let n = 3_000;
    for i in 0..n {
        text.push_str(&format!(
            "{{\"type\":\"node\",\"id\":\"{i}\",\"labels\":[\"A\"],\"properties\":{{\"uid\":{i},\"s\":\"{}\"}}}}\n",
            "z".repeat(1_000)
        ));
    }
    for i in 0..n {
        text.push_str(&format!(
            "{{\"type\":\"relationship\",\"label\":\"R\",\"start\":{{\"id\":\"{i}\"}},\"end\":{{\"id\":\"{}\"}}}}\n",
            (i * 11) % n
        ));
    }
    std::fs::write(dir.join("big.jsonl"), text).unwrap();
    let (reached, go) = super::fast::pause_load(&e);
    let e2 = Engine::handle(e.core_arc());
    let dir2 = dir.clone();
    let loader = std::thread::spawn(move || {
        let mut r = ImportRequest::new(
            Format::JsonLines,
            ImportTarget::Graph("/public/lpg".into()),
            &dir2,
            "big.jsonl",
            "r",
        );
        r.mode = ImportMode::FastLoad;
        import(&e2, &r)
    });
    reached
        .recv_timeout(crate::test_support::WAIT_BOUND)
        .unwrap();
    let count = |e: &Engine| {
        let mut tx = e.begin(Isolation::Snapshot).unwrap();
        let g = tx.graph(DEFAULT_GRAPH_ID).unwrap();
        let mut k = 0;
        tx.scan_prefix(g.nodes, b"", |_, _| {
            k += 1;
            true
        })
        .unwrap();
        tx.abort();
        k
    };
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let e = Engine::handle(e.core_arc());
            std::thread::spawn(move || count(&e))
        })
        .collect();
    for r in readers {
        assert_eq!(r.join().unwrap(), 0);
    }
    let mut before = e.begin(Isolation::Snapshot).unwrap();
    go.send(()).unwrap();
    loader.join().unwrap().unwrap();
    let g = before.graph(DEFAULT_GRAPH_ID).unwrap();
    let mut k = 0;
    before
        .scan_prefix(g.nodes, b"", |_, _| {
            k += 1;
            true
        })
        .unwrap();
    assert_eq!(k, 0, "a snapshot older than the install");
    before.abort();
    assert_eq!(count(&e), n);
    let levels = {
        let store = e.store();
        let t = store.tree(g.nodes.id()).unwrap().unwrap();
        let pin = store.pool.fetch(t.root()).unwrap();
        let page = pin.read().unwrap();
        u64::from(crate::storage::btree::node::level(&page)) + 1
    };
    assert!(levels >= 3, "the node tree has {levels} levels");
    *e.core().builds.hook.lock().unwrap() = None;
    assert_intact(&e, "three-level load");
}
