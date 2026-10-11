//! Entry points of the fuzz targets in `fuzz/` (`cargo +nightly fuzz run
//! <target>`). Compiled only under `cfg(fuzzing)`, which cargo-fuzz sets.

/// Decodes `data` as a record, and its labels alone; both must return
/// `None` on malformed bytes, never panic.
pub fn record_decode(data: &[u8]) {
    let _ = crate::types::record::decode(data);
    let _ = crate::types::record::decode_labels(data);
}

/// Tokenizes `data` (invalid UTF-8 replaced by U+FFFD) and checks that
/// the tokens tile the text in order, each on character boundaries, or
/// that the error points at a character of the text.
pub fn lexer(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    match crate::query::lexer::tokenize(&text) {
        Ok(tokens) => {
            let mut end = 0;
            for t in &tokens {
                let (start, stop) = (t.span.start.offset as usize, t.span.end.offset as usize);
                assert!(end <= start && start <= stop && stop <= text.len());
                assert!(text.is_char_boundary(start) && text.is_char_boundary(stop));
                end = stop;
            }
        }
        Err(e) => {
            let at = e.at.offset as usize;
            assert!(at < text.len() && text.is_char_boundary(at));
        }
    }
}

/// Parses `data` (invalid UTF-8 replaced by U+FFFD): the parser returns
/// a program or a 42001 error positioned in the text, never panics and
/// never overflows the stack.
pub fn parser(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    if let Err(e) = crate::query::parser::parse_program(&text) {
        assert_eq!(e.code(), "42001");
        let p = e.position().expect("a syntax error has a position");
        assert!(p.offset <= text.len() && text.is_char_boundary(p.offset));
    }
}

/// Parses `data` (invalid UTF-8 replaced by U+FFFD) and binds every
/// program the parser accepts, in a fresh transaction of one database
/// created at the first call (rmp #43): the binder returns a bound program
/// or an error, never panics. The database lives in `LPG_FUZZ_DIR` (a
/// directory under `/tmp/lpgram` that the caller creates and deletes).
pub fn binder(data: &[u8]) {
    use crate::storage::engine::Engine;
    use std::sync::OnceLock;
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    let text = String::from_utf8_lossy(data);
    let Ok(program) = crate::query::parser::parse_program(&text) else {
        return;
    };
    let e = ENGINE.get_or_init(|| {
        let dir = std::env::var("LPG_FUZZ_DIR").expect("LPG_FUZZ_DIR names an empty directory");
        Engine::create(
            std::path::Path::new(&dir),
            std::sync::Arc::new(crate::storage::FsStorage),
            64 << 20,
            "fuzz",
            std::sync::Arc::new(NoMetrics),
        )
        .expect("create the fuzz database")
    });
    let mut tx = e.begin(crate::tx::Isolation::Serializable).expect("begin");
    let session = crate::query::binder::SessionCtx::new("fuzz");
    let _ = crate::query::binder::bind(&text, &program, &mut tx, &session);
    tx.abort();
}

/// A metrics sink that keeps nothing.
struct NoMetrics;

impl crate::obs::MetricsSink for NoMetrics {
    fn counter(&self, _: &'static str, _: u64) {}
    fn gauge(&self, _: &'static str, _: u64) {}
    fn histogram(&self, _: &'static str, _: u64) {}
}
