#[allow(warnings)] #[path = "gen/gqllexer.rs"] mod gqllexer;
#[allow(warnings)] #[path = "gen/gqlparser.rs"] mod gqlparser;
#[allow(warnings)] #[path = "gen/gqllistener.rs"] mod gqllistener;
#[allow(warnings)] #[path = "gen/gqlvisitor.rs"] mod gqlvisitor;
use antlr4rust::atn_state::ATNState;
use antlr4rust::recognizer::Recognizer;
use antlr4rust::{InputStream, common_token_stream::CommonTokenStream, parser::Parser};
use std::time::Instant;

use std::rc::Rc; use std::cell::Cell;
struct Count(Rc<Cell<u32>>);
struct Diag(Rc<Cell<u32>>, Rc<Cell<u32>>);
impl<'a, T: antlr4rust::recognizer::Recognizer<'a>> antlr4rust::error_listener::ErrorListener<'a, T> for Diag {
    fn report_ambiguity(&self, _r: &T, _d: &antlr4rust::dfa::DFA, _s: isize, _t: isize, _e: bool, _a: &bit_set::BitSet, _c: &antlr4rust::atn_config_set::ATNConfigSet) { self.0.set(self.0.get()+1); if std::env::var("AMBDBG").is_ok() { let ri = _r.get_atn().states[_d.atn_start_state as usize].get_rule_index(); eprintln!("AMBIG rule={} alts={:?}", _r.get_rule_names()[ri as usize], _a); } }
    fn report_attempting_full_context(&self, _r: &T, _d: &antlr4rust::dfa::DFA, _s: isize, _t: isize, _a: &bit_set::BitSet, _c: &antlr4rust::atn_config_set::ATNConfigSet) { self.1.set(self.1.get()+1); }
}
thread_local!{ static AMB: (Rc<Cell<u32>>, Rc<Cell<u32>>) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0))); }
impl<'a, T: antlr4rust::recognizer::Recognizer<'a>> antlr4rust::error_listener::ErrorListener<'a, T> for Count {
    fn syntax_error(&self, _r: &T, _o: Option<&<T::TF as antlr4rust::token_factory::TokenFactory<'a>>::Inner>, _l: isize, _c: isize, _m: &str, _e: Option<&antlr4rust::errors::ANTLRError>) { self.0.set(self.0.get()+1); }
}
fn parse(q: &str) -> bool {
    let n = Rc::new(Cell::new(0));
    let mut lexer = gqllexer::GQLLexer::new(InputStream::new(q));
    lexer.remove_error_listeners();
    lexer.add_error_listener(Box::new(Count(n.clone())));
    let mut p = gqlparser::GQLParser::new(CommonTokenStream::new(lexer));
    p.remove_error_listeners();
    p.add_error_listener(Box::new(Count(n.clone())));
    AMB.with(|(a,f)| p.add_error_listener(Box::new(Diag(a.clone(), f.clone()))));
    if std::env::var("SLL").is_ok() { p.get_interpreter().set_prediction_mode(antlr4rust::PredictionMode::SLL); }
    let r = p.gqlProgram();
    r.is_ok() && n.get() == 0
}

fn main() {
    let mut qs: Vec<(String, String)> = Vec::new();
    for e in std::fs::read_dir(std::env::args().nth(1).unwrap()).unwrap() {
        let e = e.unwrap();
        qs.push((e.file_name().to_string_lossy().into(), std::fs::read_to_string(e.path()).unwrap()));
    }
    qs.sort();
    let iters: usize = std::env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(2000);
    for (name, q) in &qs {
        AMB.with(|(a,f)| {a.set(0); f.set(0);});
        let t0 = Instant::now();
        let ok = parse(q);
        let first = t0.elapsed();
        let (amb, full) = AMB.with(|(a,f)| (a.get(), f.get()));
        let t = Instant::now();
        for _ in 0..iters { std::hint::black_box(parse(q)); }
        let per = t.elapsed() / iters as u32;
        println!("{:<70} ok={} bytes={:>4} ambig={:>3} fullctx={:>3} first={:>9.1?} warm_mean={:>8.1?}", name, ok, q.len(), amb, full, first, per);
    }
}
