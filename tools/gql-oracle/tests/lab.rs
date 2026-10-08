//! Tests of the oracle, the grammar reader and the generator.

use gql_oracle::g4::{Grammar, Kind};
use gql_oracle::generate::Generator;
use gql_oracle::{accepts, default_prediction_mode};
use std::path::Path;

const GRAMMAR: &str = include_str!("../grammar/GQL.g4");
const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/evidence/antlr-exp/corpus"
);

/// Runs `f` on a thread with a large stack: the ANTLR parser recurses per
/// nesting level.
fn big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn corpus(prefix_invalid: bool) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(Path::new(CORPUS))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "gql"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .filter(|(n, _)| n.starts_with("zz_") == prefix_invalid)
        .collect();
    v.sort();
    v
}

#[test]
fn oracle_accepts_the_samples_and_the_q_inputs_and_rejects_the_invalid_one() {
    big_stack(|| {
        let valid = corpus(false);
        assert_eq!(
            valid.len(),
            20,
            "14 opengql/grammar samples and six q0* inputs"
        );
        assert_eq!(valid.iter().filter(|(n, _)| n.starts_with('q')).count(), 6);
        for (name, text) in &valid {
            assert!(
                accepts(text),
                "{name} rejected: {:?}",
                gql_oracle::first_error(text)
            );
        }
        let invalid = corpus(true);
        assert_eq!(invalid.len(), 1);
        assert!(!accepts(&invalid[0].1));
    });
}

#[test]
fn oracle_uses_ll_prediction() {
    assert_eq!(default_prediction_mode(), antlr4rust::PredictionMode::LL);
}

#[test]
fn reader_reads_every_rule_of_gql_g4() {
    let g = Grammar::parse(GRAMMAR).unwrap();
    let count = |k| g.rules.iter().filter(|r| r.kind == k).count();
    // Counted independently with a regular expression over GQL.g4 without
    // comments (`^\s*(fragment\s+)?NAME\s*(options{..})?:`): 1 018 rules,
    // 574 of them lower-case; docs/evidence/planning/round-s/README.md also
    // records 574 parser rules, 3 unreachable from gqlProgram.
    assert_eq!(g.rules.len(), 1018);
    assert_eq!(count(Kind::Parser), 574);
    let start = g.index("gqlProgram").unwrap();
    assert_eq!(g.reachable(start).len(), 571);
    let sp = &g.rules[g.index("SP").unwrap()];
    assert!(sp.hidden);
    let esc = &g.rules[g.index("ESCAPED_TAB").unwrap()];
    assert!(!esc.case_insensitive && esc.kind == Kind::Fragment);
    assert!(g.rules[g.index("ABS").unwrap()].case_insensitive);
}

#[test]
fn reader_rejects_unknown_constructs() {
    assert!(Grammar::parse("grammar G; a : {action} ;").is_err());
    assert!(
        Grammar::parse("grammar G; a : b ;").is_err(),
        "undefined rule"
    );
}

#[test]
fn generator_is_deterministic_per_seed() {
    let g = Grammar::parse(GRAMMAR).unwrap();
    let run = |seed| {
        let mut w = Generator::new(&g, "gqlProgram", seed)
            .unwrap()
            .with_budget_max(64);
        (0..300).map(|_| w.next_sentence().text).collect::<Vec<_>>()
    };
    assert_eq!(run(7), run(7));
    assert_ne!(run(7), run(8));
}

#[test]
fn generator_expands_every_reachable_rule_and_the_oracle_accepts_its_sentences() {
    big_stack(|| {
        let g = Grammar::parse(GRAMMAR).unwrap();
        let start = g.index("gqlProgram").unwrap();
        let mut w = Generator::new(&g, "gqlProgram", 36)
            .unwrap()
            .with_budget_max(64);
        let mut hit = vec![false; g.rules.len()];
        let mut n = 0;
        // The directed phase ends once every reachable rule is expanded.
        while g.reachable(start).iter().any(|&r| !hit[r]) {
            let s = w.next_sentence();
            assert!(
                accepts(&s.text),
                "rejected: {} ({:?})",
                s.text,
                gql_oracle::first_error(&s.text)
            );
            s.rules.iter().for_each(|&r| hit[r] = true);
            n += 1;
            assert!(n <= 600, "coverage not reached in 600 sentences");
        }
        assert_eq!(
            n,
            w.directed(),
            "every sentence until full coverage is directed"
        );
    });
}

#[test]
fn shrinker_keeps_the_property_and_the_grammar() {
    big_stack(|| {
        let g = Grammar::parse(GRAMMAR).unwrap();
        let mut w = Generator::new(&g, "gqlProgram", 1)
            .unwrap()
            .with_budget_max(64);
        let s = (0..5000)
            .map(|_| w.next_sentence())
            .find(|s| s.text.to_uppercase().contains("INSERT") && s.tokens().len() >= 20)
            .expect("a long INSERT sentence");
        let small = w.reduce(&s, |t| t.to_uppercase().contains("INSERT"));
        assert!(
            small.tokens().len() < s.tokens().len(),
            "{} -> {}",
            s.text,
            small.text
        );
        assert!(small.text.to_uppercase().contains("INSERT"));
        assert!(
            accepts(&small.text),
            "shrunk sentence left the grammar: {}",
            small.text
        );
    });
}

#[test]
fn mutants_are_near_misses_and_deterministic() {
    let g = Grammar::parse(GRAMMAR).unwrap();
    let mk = || {
        let mut w = Generator::new(&g, "gqlProgram", 5).unwrap();
        let s = w.next_sentence();
        (s.text.clone(), w.mutants(&s, 20))
    };
    let (text, a) = mk();
    assert_eq!(a, mk().1);
    // A swap of two equal neighbours is the only mutant equal to its source.
    assert!(
        a.iter().filter(|m| **m != text).count() >= 15,
        "{text} -> {a:?}"
    );
}
