//! Seeded sentence generator over a [`Grammar`], with a shrinker and
//! near-miss mutants.
//!
//! Port of gql-compat `grammar/generate.go` and `grammar/reduce.go`
//! (<https://github.com/tamnd/gql-compat>, commit `9de1991`, Apache-2.0;
//! the project states no copyright notice and has no NOTICE file; its
//! licence is `LICENSE-gql-compat` in this crate). Changes from the
//! original:
//!
//! * the input is `GQL.g4` ([`crate::g4`]) instead of the ISO BNF XML, and
//!   token texts are generated from the lexer rules instead of a hand-written
//!   leaf table;
//! * the walk is bounded by a token budget checked against the minimum
//!   derivation cost of every choice: an alternative, option or repetition
//!   is taken only if its minimum cost fits the budget left after reserving
//!   the minimum cost of the siblings that follow (the scheme of
//!   grammarinator @ `3a6a745`, BSD-3-Clause, studied, not copied:
//!   `grammarinator/tool/grammar.py:347-375` minimum sizes,
//!   `grammarinator/runtime/generator.py:162-186` reservation and limit),
//!   instead of a depth budget;
//! * alternatives are weighted by `1 / (1 + times taken so far)`, instead of
//!   by their cost, to spread the walk over the grammar;
//! * while a reachable rule has not been expanded, the next sentence is
//!   directed at it along a minimum-cost path (in the spirit of Purdom,
//!   "A sentence generator for testing parsers", BIT 12, 1972), so every
//!   reachable rule is expanded;
//! * the shrinker measures savings in tokens;
//! * near-miss mutants (token deletion, insertion, swap) are new.
//!
//! Output depends only on the grammar and the seed: the random stream is a
//! local SplitMix64 and nothing iterates a hash map.

use crate::g4::{Grammar, Kind, Node, SetItem};
use std::collections::{BTreeMap, HashMap};

/// The cost of something that cannot be written.
const INF: usize = usize::MAX / 4;

/// The seed used when `LPG_SEED` is not set.
pub const DEFAULT_SEED: u64 = 20_261_008;

/// The seed from `LPG_SEED` (decimal, or hexadecimal with `0x`), else
/// [`DEFAULT_SEED`].
///
/// # Panics
///
/// Panics when `LPG_SEED` is set but is not a number.
#[must_use]
pub fn seed_from_env() -> u64 {
    match std::env::var("LPG_SEED") {
        Ok(s) => {
            let s = s.trim();
            let parsed = match s.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16),
                None => s.parse(),
            };
            parsed.unwrap_or_else(|_| panic!("LPG_SEED={s:?} is not a u64"))
        }
        Err(_) => DEFAULT_SEED,
    }
}

/// SplitMix64 (Steele, Lea, Flood 2014; public-domain reference by Vigna).
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        ((u128::from(self.next()) * n as u128) >> 64) as usize
    }
}

/// One derivation step, mirroring the grammar node it expands.
#[derive(Debug, Clone)]
enum Deriv {
    /// A token (a lexer rule reference in a parser rule), with its text.
    Tok(String),
    /// `EOF`: writes nothing.
    Eof,
    /// A parser rule entered.
    Rule(usize, Box<Deriv>),
    Seq(Vec<Deriv>),
    /// The index of the alternative taken.
    Alt(usize, Box<Deriv>),
    Opt(Option<Box<Deriv>>),
    Rep(Vec<Deriv>),
}

impl Deriv {
    fn tokens<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Deriv::Tok(t) => out.push(t),
            Deriv::Eof | Deriv::Opt(None) => {}
            Deriv::Rule(_, d) | Deriv::Alt(_, d) | Deriv::Opt(Some(d)) => d.tokens(out),
            Deriv::Seq(v) | Deriv::Rep(v) => v.iter().for_each(|d| d.tokens(out)),
        }
    }

    fn len(&self) -> usize {
        let mut v = Vec::new();
        self.tokens(&mut v);
        v.len()
    }

    fn rules(&self, out: &mut Vec<usize>) {
        match self {
            Deriv::Rule(r, d) => {
                out.push(*r);
                d.rules(out);
            }
            Deriv::Alt(_, d) | Deriv::Opt(Some(d)) => d.rules(out),
            Deriv::Seq(v) | Deriv::Rep(v) => v.iter().for_each(|d| d.rules(out)),
            Deriv::Tok(_) | Deriv::Eof | Deriv::Opt(None) => {}
        }
    }

    fn child_mut(&mut self, i: usize) -> &mut Deriv {
        match self {
            Deriv::Rule(_, d) | Deriv::Alt(_, d) | Deriv::Opt(Some(d)) => d,
            Deriv::Seq(v) | Deriv::Rep(v) => &mut v[i],
            Deriv::Tok(_) | Deriv::Eof | Deriv::Opt(None) => unreachable!("no child"),
        }
    }
}

/// One generated sentence.
#[derive(Debug, Clone)]
pub struct Sentence {
    /// Tokens joined by one space.
    pub text: String,
    /// The parser rules the derivation expanded, sorted, without duplicates.
    pub rules: Vec<usize>,
    deriv: Deriv,
}

impl Sentence {
    fn from_deriv(deriv: Deriv) -> Self {
        let mut toks = Vec::new();
        deriv.tokens(&mut toks);
        let text = toks.join(" ");
        let mut rules = Vec::new();
        deriv.rules(&mut rules);
        rules.sort_unstable();
        rules.dedup();
        Self { text, rules, deriv }
    }

    /// The tokens of the sentence.
    #[must_use]
    pub fn tokens(&self) -> Vec<&str> {
        let mut v = Vec::new();
        self.deriv.tokens(&mut v);
        v
    }
}

/// Token texts the lexer gives to a literal-only token rule: lower-cased
/// text to the first rule (ANTLR breaks equal-length matches by rule order).
type Literals = BTreeMap<String, usize>;

/// A walk of a grammar from a start rule.
pub struct Generator<'g> {
    g: &'g Grammar,
    start: usize,
    /// Minimum token count of each parser rule.
    cost: Vec<usize>,
    /// Minimum token count of a whole sentence that expands each rule.
    context: Vec<usize>,
    literals: Literals,
    rng: Rng,
    /// The mutants' own stream, so that making mutants never changes the
    /// sentences.
    mutant_rng: Rng,
    /// How often each alternative of each `Alt` node was taken.
    uses: HashMap<*const Node, Vec<u32>>,
    budget_max: usize,
    /// Token texts that still collide with an earlier literal rule after
    /// the retries, per token rule.
    collisions: BTreeMap<usize, u32>,
    /// The reachable writable parser rules, and whether a sentence already
    /// expanded each.
    targets: Vec<usize>,
    expanded: Vec<bool>,
    directed: usize,
}

impl<'g> Generator<'g> {
    /// Prepares a walk of `g` from `start`, seeded with `seed`.
    ///
    /// # Errors
    ///
    /// Fails when `start` is not a parser rule, cannot be written, or a lexer
    /// rule uses a Unicode property the generator has no characters for.
    pub fn new(g: &'g Grammar, start: &str, seed: u64) -> Result<Self, String> {
        let start = g.index(start).ok_or_else(|| format!("no rule {start}"))?;
        if g.rules[start].kind != Kind::Parser {
            return Err("the start rule is not a parser rule".into());
        }
        for r in &g.rules {
            check_properties(&r.body)?;
        }
        let cost = rule_costs(g);
        if cost[start] >= INF {
            return Err("the start rule cannot be written".into());
        }
        let context = context_costs(g, &cost, start);
        let budget_max = g
            .reachable(start)
            .iter()
            .map(|&r| context[r])
            .filter(|&c| c < INF)
            .max()
            .unwrap_or(cost[start]);
        let targets = g
            .reachable(start)
            .into_iter()
            .filter(|&r| context[r] < INF)
            .collect();
        Ok(Self {
            g,
            start,
            literals: literal_tokens(g),
            cost,
            context,
            rng: Rng(seed),
            mutant_rng: Rng(seed ^ 0x6D75_7461_6E74),
            uses: HashMap::new(),
            budget_max,
            collisions: BTreeMap::new(),
            targets,
            expanded: vec![false; g.rules.len()],
            directed: 0,
        })
    }

    /// The minimum token count of a sentence that expands each rule
    /// (`usize::MAX / 4` when none can).
    #[must_use]
    pub fn context_costs(&self) -> &[usize] {
        &self.context
    }

    /// The largest token budget a sentence is given.
    #[must_use]
    pub fn budget_max(&self) -> usize {
        self.budget_max
    }

    /// The largest minimum sentence cost over the reachable rules: the
    /// smallest budget in which every reachable rule fits.
    #[must_use]
    pub fn context_max(&self) -> usize {
        self.g
            .reachable(self.start)
            .iter()
            .map(|&r| self.context[r])
            .filter(|&c| c < INF)
            .max()
            .unwrap_or(0)
    }

    /// Sets the largest token budget; it is raised to [`Self::context_max`]
    /// when smaller.
    #[must_use]
    pub fn with_budget_max(mut self, max: usize) -> Self {
        self.budget_max = max.max(self.context_max());
        self
    }

    /// Token rules whose generated text kept colliding with an earlier
    /// literal-only token rule, with the number of such texts emitted.
    #[must_use]
    pub fn collisions(&self) -> &BTreeMap<usize, u32> {
        &self.collisions
    }

    /// How many sentences so far were directed at an unexpanded rule.
    #[must_use]
    pub fn directed(&self) -> usize {
        self.directed
    }

    /// Writes the next sentence. While a reachable rule has not been
    /// expanded by any sentence, the walk is directed at the first such rule
    /// (rule order) along a minimum-cost path, as in Purdom's sentence
    /// generator; every other choice is random. Otherwise the walk is
    /// random. The token budget is drawn in `minimum..=budget_max`.
    pub fn next_sentence(&mut self) -> Sentence {
        let target = self.targets.iter().copied().find(|&r| !self.expanded[r]);
        let d = match target {
            Some(t) => {
                self.directed += 1;
                let reach = reach_costs(self.g, &self.cost, t);
                let lo = reach[self.start];
                let budget = lo + self.rng.below(self.budget_max.max(lo) - lo + 1);
                if t == self.start {
                    self.walk_rule(t, budget)
                } else {
                    let g = self.g;
                    let body = self.expand_to(&g.rules[self.start].body, t, &reach, budget);
                    Deriv::Rule(self.start, Box::new(body))
                }
            }
            None => {
                let lo = self.cost[self.start];
                let budget = lo + self.rng.below(self.budget_max - lo + 1);
                self.walk_rule(self.start, budget)
            }
        };
        let s = Sentence::from_deriv(d);
        for &r in &s.rules {
            self.expanded[r] = true;
        }
        s
    }

    /// Expands `n` so that the derivation expands rule `t`; `reach` holds
    /// the minimum cost of each rule's derivation through `t`, and
    /// `reach_cost(n) <= budget`.
    fn expand_to(&mut self, n: &'g Node, t: usize, reach: &[usize], budget: usize) -> Deriv {
        let g = self.g;
        match n {
            Node::Ref(r) if *r == t => self.walk_rule(t, budget),
            Node::Ref(r) => Deriv::Rule(
                *r,
                Box::new(self.expand_to(&g.rules[*r].body, t, reach, budget)),
            ),
            Node::Seq(children) => {
                let costs: Vec<usize> = children.iter().map(|c| self.node_cost(c)).collect();
                let all: usize = costs.iter().sum();
                let j = (0..children.len())
                    .min_by_key(|&j| {
                        let r = reach_cost(g, &self.cost, reach, &children[j]);
                        (if r >= INF { INF } else { all - costs[j] + r }, j)
                    })
                    .unwrap_or(0);
                let mut rest: usize =
                    all - costs[j] + reach_cost(g, &self.cost, reach, &children[j]);
                let mut used = 0;
                let mut out = Vec::with_capacity(children.len());
                for (i, c) in children.iter().enumerate() {
                    let d = if i == j {
                        rest -= reach_cost(g, &self.cost, reach, c);
                        self.expand_to(c, t, reach, budget - used - rest)
                    } else {
                        rest -= costs[i];
                        self.expand(c, budget - used - rest)
                    };
                    used += d.len();
                    out.push(d);
                }
                Deriv::Seq(out)
            }
            Node::Alt(children) => {
                let i = (0..children.len())
                    .min_by_key(|&i| (reach_cost(g, &self.cost, reach, &children[i]), i))
                    .unwrap_or(0);
                Deriv::Alt(i, Box::new(self.expand_to(&children[i], t, reach, budget)))
            }
            Node::Opt(c) => Deriv::Opt(Some(Box::new(self.expand_to(c, t, reach, budget)))),
            Node::Star(c) | Node::Plus(c) => Deriv::Rep(vec![self.expand_to(c, t, reach, budget)]),
            Node::Eof | Node::Lit(_) | Node::Set { .. } | Node::Any => {
                unreachable!("a directed walk only enters nodes that reach the target")
            }
        }
    }

    fn node_cost(&self, n: &Node) -> usize {
        node_cost(self.g, &self.cost, n)
    }

    fn walk_rule(&mut self, r: usize, budget: usize) -> Deriv {
        let g = self.g;
        Deriv::Rule(r, Box::new(self.expand(&g.rules[r].body, budget)))
    }

    /// Expands `n` within `budget` tokens; requires `node_cost(n) <= budget`.
    fn expand(&mut self, n: &'g Node, budget: usize) -> Deriv {
        match n {
            Node::Eof => Deriv::Eof,
            Node::Ref(r) => match self.g.rules[*r].kind {
                Kind::Parser => self.walk_rule(*r, budget),
                Kind::Token | Kind::Fragment => Deriv::Tok(self.token_text(*r)),
            },
            Node::Seq(children) => {
                let mut rest: usize = children.iter().map(|c| self.node_cost(c)).sum();
                let mut used = 0;
                let mut out = Vec::with_capacity(children.len());
                for c in children {
                    rest -= self.node_cost(c);
                    let d = self.expand(c, budget - used - rest);
                    used += d.len();
                    out.push(d);
                }
                Deriv::Seq(out)
            }
            Node::Alt(children) => {
                let i = self.choose(n, children, budget);
                Deriv::Alt(i, Box::new(self.expand(&children[i], budget)))
            }
            Node::Opt(c) => {
                if self.node_cost(c) <= budget && self.rng.below(2) == 0 {
                    Deriv::Opt(Some(Box::new(self.expand(c, budget))))
                } else {
                    Deriv::Opt(None)
                }
            }
            Node::Star(c) | Node::Plus(c) => {
                let min = usize::from(matches!(n, Node::Plus(_)));
                let one = self.node_cost(c);
                let mut out = Vec::new();
                let mut used = 0;
                while out.len() < 3 {
                    let left = budget - used;
                    let required = out.len() < min;
                    if !required && (one > left || self.rng.below(2) == 0) {
                        break;
                    }
                    let d = self.expand(c, left);
                    used += d.len();
                    out.push(d);
                }
                Deriv::Rep(out)
            }
            Node::Lit(_) | Node::Set { .. } | Node::Any => {
                unreachable!("lexer construct in a parser rule")
            }
        }
    }

    /// Picks an alternative that fits `budget`, weighted towards the ones
    /// taken least so far.
    fn choose(&mut self, n: &Node, children: &[Node], budget: usize) -> usize {
        let fits: Vec<usize> = (0..children.len())
            .filter(|&i| self.node_cost(&children[i]) <= budget)
            .collect();
        let uses = self
            .uses
            .entry(std::ptr::from_ref(n))
            .or_insert_with(|| vec![0; children.len()]);
        let weights: Vec<u64> = fits
            .iter()
            .map(|&i| (1u64 << 20) / (1 + u64::from(uses[i])))
            .collect();
        let total: u64 = weights.iter().sum();
        let mut pick = self.rng.below(total as usize) as u64;
        let mut chosen = fits[fits.len() - 1];
        for (k, w) in weights.iter().enumerate() {
            if pick < *w {
                chosen = fits[k];
                break;
            }
            pick -= w;
        }
        uses[chosen] += 1;
        chosen
    }

    /// A random text of token rule `r`, retried while an earlier
    /// literal-only rule would take it.
    fn token_text(&mut self, r: usize) -> String {
        let mut text = String::new();
        for _ in 0..16 {
            text.clear();
            let ci = self.g.rules[r].case_insensitive;
            self.lex(&self.g.rules[r].body, ci, &mut text);
            match self.literals.get(&text.to_lowercase()) {
                Some(&first) if first < r => {}
                _ => return text,
            }
        }
        *self.collisions.entry(r).or_default() += 1;
        text
    }

    fn lex(&mut self, n: &'g Node, ci: bool, out: &mut String) {
        match n {
            Node::Lit(s) => {
                // Case-insensitive literals: as written (half), lower case
                // (a quarter) or mixed case (a quarter).
                let mode = if ci { self.rng.below(4) } else { 0 };
                for ch in s.chars() {
                    let lower = match mode {
                        0 | 1 => None,
                        2 => Some(true),
                        _ => Some(self.rng.below(2) == 0),
                    };
                    push_case(out, ch, lower);
                }
            }
            Node::Set { negated, items } => {
                let pool = set_pool(items, *negated);
                let ch = pool[self.rng.below(pool.len())];
                let lower = if ci {
                    Some(self.rng.below(2) == 0)
                } else {
                    None
                };
                push_case(out, ch, lower);
            }
            Node::Any => out.push('a'),
            Node::Ref(r) => {
                let rule = &self.g.rules[*r];
                let ci = if rule.kind == Kind::Fragment {
                    rule.case_insensitive
                } else {
                    ci
                };
                self.lex(&rule.body, ci, out);
            }
            Node::Seq(v) => v.iter().for_each(|c| self.lex(c, ci, out)),
            Node::Alt(v) => {
                let i = self.rng.below(v.len());
                self.lex(&v[i], ci, out);
            }
            Node::Opt(c) => {
                if self.rng.below(2) == 0 {
                    self.lex(c, ci, out);
                }
            }
            Node::Star(c) | Node::Plus(c) => {
                let n_reps = usize::from(matches!(n, Node::Plus(_))) + self.rng.below(3);
                for _ in 0..n_reps {
                    self.lex(c, ci, out);
                }
            }
            Node::Eof => {}
        }
    }

    /// Near-miss mutants of `s`: `count` texts, each one token deleted,
    /// inserted (a literal token of the grammar) or swapped with its
    /// neighbour. A mutant can still be a valid sentence; the oracle labels it.
    pub fn mutants(&mut self, s: &Sentence, count: usize) -> Vec<String> {
        let toks = s.tokens();
        let pool: Vec<&str> = self.literals.keys().map(String::as_str).collect();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let mut m: Vec<&str> = toks.clone();
            match self.mutant_rng.below(3) {
                0 if !m.is_empty() => {
                    m.remove(self.mutant_rng.below(m.len()));
                }
                1 => {
                    let at = self.mutant_rng.below(m.len() + 1);
                    m.insert(at, pool[self.mutant_rng.below(pool.len())]);
                }
                _ if m.len() >= 2 => {
                    let i = self.mutant_rng.below(m.len() - 1);
                    m.swap(i, i + 1);
                }
                _ => m.push(pool[self.mutant_rng.below(pool.len())]),
            }
            out.push(m.join(" "));
        }
        out
    }

    /// Shrinks `s` while `still` holds, by undoing choices (dropping an
    /// option, trimming a repetition to its minimum, replacing a rule or an
    /// alternative by its smallest derivation), so every candidate is a
    /// sentence of the grammar. Greedy and deterministic: the first candidate
    /// that holds, largest token saving first, ties by position.
    pub fn reduce(&self, s: &Sentence, mut still: impl FnMut(&str) -> bool) -> Sentence {
        const MAX_STEPS: usize = 40;
        const MAX_TRIES: usize = 200;
        let mut cur = s.deriv.clone();
        let mut tries = 0;
        for _ in 0..MAX_STEPS {
            let mut improved = false;
            for cand in self.candidates(&cur) {
                if tries >= MAX_TRIES {
                    break;
                }
                tries += 1;
                if still(&Sentence::from_deriv(cand.clone()).text) {
                    cur = cand;
                    improved = true;
                    break;
                }
            }
            if !improved {
                break;
            }
        }
        Sentence::from_deriv(cur)
    }

    fn candidates(&self, d: &Deriv) -> Vec<Deriv> {
        let mut sites: Vec<(usize, Vec<usize>, Deriv)> = Vec::new();
        let start = Node::Ref(self.start);
        self.sites(d, &start, &mut Vec::new(), &mut sites);
        sites.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        sites
            .into_iter()
            .map(|(_, path, repl)| {
                let mut c = d.clone();
                let mut at = &mut c;
                for &i in &path {
                    at = at.child_mut(i);
                }
                *at = repl;
                c
            })
            .collect()
    }

    /// Collects (saving, path, replacement) for every shrink site of `d`,
    /// which expands node `n`.
    fn sites(
        &self,
        d: &Deriv,
        n: &Node,
        path: &mut Vec<usize>,
        out: &mut Vec<(usize, Vec<usize>, Deriv)>,
    ) {
        let size = d.len();
        let push = |repl: Deriv, out: &mut Vec<_>| {
            let saving = size.saturating_sub(repl.len());
            if saving > 0 {
                out.push((saving, path.clone(), repl));
            }
        };
        match (d, n) {
            (Deriv::Opt(Some(_)), Node::Opt(_)) => push(Deriv::Opt(None), out),
            (Deriv::Rep(v), Node::Star(_)) if !v.is_empty() => push(Deriv::Rep(Vec::new()), out),
            (Deriv::Rep(v), Node::Plus(_)) if v.len() > 1 => {
                push(Deriv::Rep(vec![v[0].clone()]), out)
            }
            (Deriv::Rule(..), Node::Ref(_)) | (Deriv::Alt(..), Node::Alt(_)) => {
                push(self.minimal(n), out)
            }
            _ => {}
        }
        let mut child = |i: usize, dc: &Deriv, nc: &Node, out: &mut Vec<_>| {
            path.push(i);
            self.sites(dc, nc, path, out);
            path.pop();
        };
        match (d, n) {
            (Deriv::Rule(r, b), Node::Ref(_)) => child(0, b, &self.g.rules[*r].body, out),
            (Deriv::Seq(v), Node::Seq(ns)) => {
                for (i, (dc, nc)) in v.iter().zip(ns).enumerate() {
                    child(i, dc, nc, out);
                }
            }
            (Deriv::Alt(k, b), Node::Alt(ns)) => child(0, b, &ns[*k], out),
            (Deriv::Opt(Some(b)), Node::Opt(nc)) => child(0, b, nc, out),
            (Deriv::Rep(v), Node::Star(nc) | Node::Plus(nc)) => {
                for (i, dc) in v.iter().enumerate() {
                    child(i, dc, nc, out);
                }
            }
            _ => {}
        }
    }

    /// The smallest derivation of `n`: cheapest alternatives (ties by
    /// grammar order), no options, minimum repetitions, minimal token texts.
    fn minimal(&self, n: &Node) -> Deriv {
        match n {
            Node::Eof => Deriv::Eof,
            Node::Ref(r) => match self.g.rules[*r].kind {
                Kind::Parser => Deriv::Rule(*r, Box::new(self.minimal(&self.g.rules[*r].body))),
                Kind::Token | Kind::Fragment => {
                    let mut s = String::new();
                    lex_min(self.g, &self.g.rules[*r].body, &mut s);
                    Deriv::Tok(s)
                }
            },
            Node::Seq(v) => Deriv::Seq(v.iter().map(|c| self.minimal(c)).collect()),
            Node::Alt(v) => {
                let (i, _) = v
                    .iter()
                    .enumerate()
                    .min_by_key(|(i, c)| (self.node_cost(c), *i))
                    .unwrap_or((0, &v[0]));
                Deriv::Alt(i, Box::new(self.minimal(&v[i])))
            }
            Node::Opt(_) | Node::Star(_) => {
                if matches!(n, Node::Opt(_)) {
                    Deriv::Opt(None)
                } else {
                    Deriv::Rep(Vec::new())
                }
            }
            Node::Plus(c) => Deriv::Rep(vec![self.minimal(c)]),
            Node::Lit(_) | Node::Set { .. } | Node::Any => unreachable!("lexer construct"),
        }
    }
}

/// Deterministic minimal text of a lexer node: first alternative, no
/// options, minimum repetitions, first character of a set.
fn lex_min(g: &Grammar, n: &Node, out: &mut String) {
    match n {
        Node::Lit(s) => out.push_str(s),
        Node::Set { negated, items } => out.push(set_pool(items, *negated)[0]),
        Node::Any => out.push('a'),
        Node::Ref(r) => lex_min(g, &g.rules[*r].body, out),
        Node::Seq(v) => v.iter().for_each(|c| lex_min(g, c, out)),
        Node::Alt(v) => lex_min(g, &v[0], out),
        Node::Plus(c) => lex_min(g, c, out),
        Node::Opt(_) | Node::Star(_) | Node::Eof => {}
    }
}

/// Pushes `ch` as is (`None`), lower-cased or upper-cased.
fn push_case(out: &mut String, ch: char, lower: Option<bool>) {
    match lower {
        None => out.push(ch),
        Some(true) => out.extend(ch.to_lowercase()),
        Some(false) => out.extend(ch.to_uppercase()),
    }
}

/// Characters the generator uses for a Unicode property. Each is a member
/// of the property in Unicode 15.0 (the version of the antlr4rust v0.6.0
/// tool jar, docs/evidence/planning/round-s): ASCII letters and `é` (U+00E9)
/// and `λ` (U+03BB) are general category Ll/Lu, hence ID_Start and
/// ID_Continue (UAX #31); digits are Nd and `_` is Pc, hence ID_Continue.
fn property_chars(name: &str) -> Option<Vec<char>> {
    let letters = ('a'..='z').chain('A'..='Z').chain(['é', 'λ']);
    Some(match name {
        "ID_Start" => letters.collect(),
        "ID_Continue" => letters.chain('0'..='9').chain(['_']).collect(),
        "Pc" => vec!['_'],
        "Sc" => vec!['$', '€'],
        _ => return None,
    })
}

fn check_properties(n: &Node) -> Result<(), String> {
    match n {
        Node::Set { items, .. } => items.iter().try_for_each(|i| match i {
            SetItem::Property(p) if property_chars(p).is_none() => {
                Err(format!("no characters for \\p{{{p}}}"))
            }
            _ => Ok(()),
        }),
        Node::Seq(v) | Node::Alt(v) => v.iter().try_for_each(check_properties),
        Node::Opt(c) | Node::Star(c) | Node::Plus(c) => check_properties(c),
        _ => Ok(()),
    }
}

fn in_set(items: &[SetItem], ch: char) -> bool {
    items.iter().any(|i| match i {
        SetItem::Range(lo, hi) => (*lo..=*hi).contains(&ch),
        SetItem::Property(p) => property_chars(p).is_some_and(|v| v.contains(&ch)),
    })
}

/// The characters a set can produce: its own members (ranges capped at 64
/// characters), or for a negated set the printable ASCII characters and
/// `é`, `λ` that are not members.
fn set_pool(items: &[SetItem], negated: bool) -> Vec<char> {
    let mut pool: Vec<char> = if negated {
        (' '..='~')
            .chain(['é', 'λ'])
            .filter(|&c| !in_set(items, c))
            .collect()
    } else {
        let mut v = Vec::new();
        for i in items {
            match i {
                SetItem::Range(lo, hi) => v.extend((*lo..=*hi).take(64)),
                SetItem::Property(p) => v.extend(property_chars(p).unwrap_or_default()),
            }
        }
        v
    };
    pool.dedup();
    assert!(!pool.is_empty(), "empty character set");
    pool
}

/// Minimum token count of every parser rule (a token reference costs 1,
/// `EOF` 0), by fixed point: costs only fall and are bounded below.
fn rule_costs(g: &Grammar) -> Vec<usize> {
    let mut cost = vec![INF; g.rules.len()];
    loop {
        let mut changed = false;
        for (i, r) in g.rules.iter().enumerate() {
            if r.kind != Kind::Parser {
                continue;
            }
            let c = node_cost(g, &cost, &r.body);
            if c < cost[i] {
                cost[i] = c;
                changed = true;
            }
        }
        if !changed {
            return cost;
        }
    }
}

fn node_cost(g: &Grammar, cost: &[usize], n: &Node) -> usize {
    match n {
        Node::Eof | Node::Opt(_) | Node::Star(_) => 0,
        Node::Ref(r) => match g.rules[*r].kind {
            Kind::Parser => cost[*r],
            Kind::Token | Kind::Fragment => 1,
        },
        Node::Seq(v) => v
            .iter()
            .map(|c| node_cost(g, cost, c))
            .fold(0, |a, b| (a + b).min(INF)),
        Node::Alt(v) => v.iter().map(|c| node_cost(g, cost, c)).min().unwrap_or(INF),
        Node::Plus(c) => node_cost(g, cost, c),
        Node::Lit(_) | Node::Set { .. } | Node::Any => 1,
    }
}

/// Minimum cost of `n` with at least one direct reference to `target`.
fn forced_cost(g: &Grammar, cost: &[usize], n: &Node, target: usize) -> usize {
    match n {
        Node::Ref(r) if *r == target => cost[*r],
        Node::Seq(v) => {
            let all: usize = v.iter().map(|c| node_cost(g, cost, c)).sum();
            v.iter()
                .map(|c| {
                    let f = forced_cost(g, cost, c, target);
                    if f >= INF {
                        INF
                    } else {
                        all - node_cost(g, cost, c) + f
                    }
                })
                .min()
                .unwrap_or(INF)
        }
        Node::Alt(v) => v
            .iter()
            .map(|c| forced_cost(g, cost, c, target))
            .min()
            .unwrap_or(INF),
        Node::Opt(c) | Node::Star(c) | Node::Plus(c) => forced_cost(g, cost, c, target),
        _ => INF,
    }
}

/// Minimum token count of a derivation of each parser rule that expands
/// rule `t` somewhere, by fixed point (`INF` when it cannot).
fn reach_costs(g: &Grammar, cost: &[usize], t: usize) -> Vec<usize> {
    let mut reach = vec![INF; g.rules.len()];
    reach[t] = cost[t];
    loop {
        let mut changed = false;
        for (i, r) in g.rules.iter().enumerate() {
            if r.kind != Kind::Parser || i == t {
                continue;
            }
            let c = reach_cost(g, cost, &reach, &r.body);
            if c < reach[i] {
                reach[i] = c;
                changed = true;
            }
        }
        if !changed {
            return reach;
        }
    }
}

/// Minimum cost of `n` through the target of `reach`.
fn reach_cost(g: &Grammar, cost: &[usize], reach: &[usize], n: &Node) -> usize {
    match n {
        Node::Ref(r) if g.rules[*r].kind == Kind::Parser => reach[*r],
        Node::Seq(v) => {
            let all: usize = v.iter().map(|c| node_cost(g, cost, c)).sum();
            v.iter()
                .map(|c| {
                    let f = reach_cost(g, cost, reach, c);
                    if f >= INF {
                        INF
                    } else {
                        all - node_cost(g, cost, c) + f
                    }
                })
                .min()
                .unwrap_or(INF)
        }
        Node::Alt(v) => v
            .iter()
            .map(|c| reach_cost(g, cost, reach, c))
            .min()
            .unwrap_or(INF),
        Node::Opt(c) | Node::Star(c) | Node::Plus(c) => reach_cost(g, cost, reach, c),
        _ => INF,
    }
}

/// Minimum token count of a whole sentence from `start` that expands each
/// parser rule, by fixed point over the rules that reference it.
fn context_costs(g: &Grammar, cost: &[usize], start: usize) -> Vec<usize> {
    let mut ctx = vec![INF; g.rules.len()];
    ctx[start] = cost[start];
    let parsers: Vec<usize> = (0..g.rules.len())
        .filter(|&i| g.rules[i].kind == Kind::Parser)
        .collect();
    loop {
        let mut changed = false;
        for &q in &parsers {
            if ctx[q] >= INF {
                continue;
            }
            let mut targets = Vec::new();
            direct_refs(&g.rules[q].body, &mut targets);
            targets.sort_unstable();
            targets.dedup();
            for r in targets {
                if g.rules[r].kind != Kind::Parser {
                    continue;
                }
                let f = forced_cost(g, cost, &g.rules[q].body, r);
                if f >= INF {
                    continue;
                }
                let c = ctx[q] - cost[q] + f;
                if c < ctx[r] {
                    ctx[r] = c;
                    changed = true;
                }
            }
        }
        if !changed {
            return ctx;
        }
    }
}

fn direct_refs(n: &Node, out: &mut Vec<usize>) {
    match n {
        Node::Ref(r) => out.push(*r),
        Node::Seq(v) | Node::Alt(v) => v.iter().for_each(|c| direct_refs(c, out)),
        Node::Opt(c) | Node::Star(c) | Node::Plus(c) => direct_refs(c, out),
        _ => {}
    }
}

/// Lower-cased texts of literal-only token rules (a literal or an
/// alternative of literals, possibly through references), each mapped to
/// the first rule that matches it.
fn literal_tokens(g: &Grammar) -> Literals {
    fn lits(g: &Grammar, n: &Node, out: &mut Vec<String>) -> bool {
        match n {
            Node::Lit(s) => {
                out.push(s.to_lowercase());
                true
            }
            Node::Alt(v) => v.iter().all(|c| lits(g, c, out)),
            Node::Ref(r) => lits(g, &g.rules[*r].body, out),
            _ => false,
        }
    }
    let mut map = Literals::new();
    for (i, r) in g.rules.iter().enumerate() {
        if r.kind != Kind::Token || r.hidden {
            continue;
        }
        let mut v = Vec::new();
        if lits(g, &r.body, &mut v) {
            for s in v {
                map.entry(s).or_insert(i);
            }
        }
    }
    map
}
