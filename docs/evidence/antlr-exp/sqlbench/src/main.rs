use sqlparser::{dialect::GenericDialect, parser::Parser};
use std::time::Instant;
fn main() {
    let qs = [
      ("sql_q01_201B", "SELECT f.id AS friendId, f.lastName AS lastName FROM person p JOIN knows k ON k.src = p.id JOIN person f ON f.id = k.dst WHERE p.id = $1 AND f.firstName = $2 AND f.id <> p.id ORDER BY lastName ASC, friendId ASC LIMIT 20"),
      ("sql_q04_agg", "SELECT p.id AS personId, t.name AS tag, count(*) AS n, sum(m.length) AS total FROM message m JOIN person p ON m.creator = p.id JOIN has_tag h ON h.msg = m.id JOIN tag t ON t.id = h.tag WHERE m.creationDate >= $1 AND m.creationDate < $2 AND (t.name = 'a' OR t.name = 'b') GROUP BY p.id, t.name ORDER BY n DESC LIMIT 100"),
      ("sql_q05_expr", "SELECT CASE WHEN n.a > 1 THEN n.a ELSE 0 END AS v FROM node n WHERE ((n.a + 1) * 2 - n.b / 3 > 10 AND NOT n.c IS NULL) OR (n.d = 'x' || n.e)"),
    ];
    let d = GenericDialect {};
    for (name, q) in qs {
        let t0 = Instant::now(); let ok = Parser::parse_sql(&d, q).is_ok(); let first = t0.elapsed();
        let n = 20000u32; let t = Instant::now();
        for _ in 0..n { std::hint::black_box(Parser::parse_sql(&d, q).unwrap()); }
        println!("{name} ok={ok} bytes={} first={:?} warm_mean={:?}", q.len(), first, t.elapsed()/n);
    }
}
