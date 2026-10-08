//! `gql-oracle`: the oracle and the generator as a command.
//!
//! ```text
//! gql-oracle label [DIR]     label each file of DIR, or each line of stdin
//! gql-oracle generate N      write N sentences, one per line (LPG_SEED)
//! gql-oracle mutants N       write near-miss mutants of N sentences (LPG_SEED)
//! gql-oracle check N         generate N sentences, label them, report
//!                            rule coverage and the rejected sentences shrunk
//! ```
//!
//! `label` prints `ACCEPT<TAB>name` or `REJECT<TAB>name<TAB>first error`.

#![forbid(unsafe_code)]

use gql_oracle::g4::Grammar;
use gql_oracle::generate::{Generator, Sentence, seed_from_env};
use gql_oracle::{accepts, first_error};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::process::ExitCode;

const GRAMMAR: &str = include_str!("../grammar/GQL.g4");
const START: &str = "gqlProgram";
/// The ANTLR parser and the walk recurse per nesting level.
const STACK: usize = 512 << 20;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || command(&args));
    match run.map(|h| h.join()) {
        Ok(Ok(Ok(()))) => ExitCode::SUCCESS,
        Ok(Ok(Err(e))) => {
            eprintln!("gql-oracle: {e}");
            ExitCode::FAILURE
        }
        _ => ExitCode::FAILURE,
    }
}

fn command(args: &[String]) -> Result<(), String> {
    let count = || -> Result<usize, String> {
        args.get(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| "expected a count".to_owned())
    };
    match args.first().map(String::as_str) {
        Some("label") => label(args.get(1)),
        Some("generate") => generate(count()?, false),
        Some("mutants") => generate(count()?, true),
        Some("check") => check(count()?),
        _ => Err("usage: gql-oracle label [DIR] | generate N | mutants N | check N".into()),
    }
}

fn label(dir: Option<&String>) -> Result<(), String> {
    let mut inputs: Vec<(String, String)> = Vec::new();
    match dir {
        Some(d) => {
            for e in std::fs::read_dir(d).map_err(|e| format!("{d}: {e}"))? {
                let p = e.map_err(|e| e.to_string())?.path();
                if p.is_file() {
                    let text =
                        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    inputs.push((name, text));
                }
            }
            inputs.sort();
        }
        None => {
            for (i, line) in std::io::stdin().lock().lines().enumerate() {
                inputs.push(((i + 1).to_string(), line.map_err(|e| e.to_string())?));
            }
        }
    }
    let mut out = std::io::stdout().lock();
    for (name, text) in &inputs {
        let line = match first_error(text) {
            None => format!("ACCEPT\t{name}"),
            Some(e) => format!("REJECT\t{name}\t{}", e.replace(['\n', '\t'], " ")),
        };
        writeln!(out, "{line}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn budget() -> usize {
    std::env::var("BUDGET")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64)
}

fn grammar() -> Result<Grammar, String> {
    Grammar::parse(GRAMMAR).map_err(|e| e.to_string())
}

fn generate(n: usize, mutants: bool) -> Result<(), String> {
    let g = grammar()?;
    let mut walker = Generator::new(&g, START, seed_from_env())?.with_budget_max(budget());
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    for _ in 0..n {
        let s = walker.next_sentence();
        let lines = if mutants {
            walker.mutants(&s, 1)
        } else {
            vec![s.text]
        };
        for l in lines {
            writeln!(out, "{l}").map_err(|e| e.to_string())?;
        }
    }
    out.flush().map_err(|e| e.to_string())
}

/// Labels `texts` on all cores; true = accepted.
fn label_all(texts: &[&str]) -> Vec<bool> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let chunk = texts.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = texts
            .chunks(chunk)
            .map(|c| {
                std::thread::Builder::new()
                    .stack_size(STACK)
                    .spawn_scoped(s, move || c.iter().map(|t| accepts(t)).collect::<Vec<_>>())
                    .expect("spawn an oracle thread")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("oracle thread"))
            .collect()
    })
}

fn check(n: usize) -> Result<(), String> {
    let g = grammar()?;
    let seed = seed_from_env();
    let mut walker = Generator::new(&g, START, seed)?.with_budget_max(budget());
    let start = g.index(START).ok_or("no start rule")?;
    let reachable = g.reachable(start);
    let sentences: Vec<Sentence> = (0..n).map(|_| walker.next_sentence()).collect();
    let mutants: Vec<String> = sentences
        .iter()
        .flat_map(|s| walker.mutants(s, 1))
        .collect();

    let mut hits = vec![0u32; g.rules.len()];
    for s in &sentences {
        for &r in &s.rules {
            hits[r] += 1;
        }
    }
    let missing: Vec<&str> = reachable
        .iter()
        .filter(|&&r| hits[r] == 0)
        .map(|&r| g.rules[r].name.as_str())
        .collect();
    let tokens: usize = sentences.iter().map(|s| s.tokens().len()).sum();
    let max_tokens = sentences
        .iter()
        .map(|s| s.tokens().len())
        .max()
        .unwrap_or(0);

    let texts: Vec<&str> = sentences.iter().map(|s| s.text.as_str()).collect();
    let labels = label_all(&texts);
    let mtexts: Vec<&str> = mutants.iter().map(String::as_str).collect();
    let mlabels = label_all(&mtexts);

    println!("seed {seed}");
    println!(
        "sentences {n}, tokens {tokens} (mean {:.1}, max {max_tokens})",
        tokens as f64 / n as f64
    );
    println!(
        "budget max {} tokens (largest minimal context {})",
        walker.budget_max(),
        walker.context_max()
    );
    println!("directed sentences {}", walker.directed());
    println!(
        "parser rules {}, reachable from {START} {}, expanded {}, missing {}",
        g.rules
            .iter()
            .filter(|r| r.kind == gql_oracle::g4::Kind::Parser)
            .count(),
        reachable.len(),
        reachable.len() - missing.len(),
        missing.len()
    );
    for m in &missing {
        println!("  missing {m}");
    }
    for (r, c) in walker.collisions() {
        println!(
            "token {} kept an earlier literal rule's text {c} times",
            g.rules[*r].name
        );
    }
    let rejected: Vec<usize> = (0..n).filter(|&i| !labels[i]).collect();
    println!(
        "oracle: accepted {}, rejected {}",
        n - rejected.len(),
        rejected.len()
    );
    println!(
        "mutants {}: accepted {}, rejected {}",
        mutants.len(),
        mlabels.iter().filter(|&&a| a).count(),
        mlabels.iter().filter(|&&a| !a).count()
    );

    // Each rejected sentence shrunk while the oracle still rejects it, then
    // grouped by the shrunk text.
    let mut classes: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for &i in &rejected {
        let small = walker.reduce(&sentences[i], |t| !accepts(t));
        let e = first_error(&small.text).unwrap_or_default();
        classes.entry(small.text).or_insert((0, e)).0 += 1;
    }
    for (text, (count, err)) in &classes {
        println!("  rejected class x{count}: {text}  [{err}]");
    }
    Ok(())
}
