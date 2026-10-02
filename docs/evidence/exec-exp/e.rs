// Row-at-a-time (slotted frame, dyn dispatch per row) vs batch-at-a-time pull (dyn dispatch per batch).
use std::sync::Arc; use std::time::Instant;
#[derive(Clone, Debug)] #[allow(dead_code)]
enum V { Null, Int(i64), Float(f64), Str(Arc<str>) }
// ---- row model: frame of slots ----
trait RowOp { fn next(&mut self, f: &mut [V]) -> bool; }
struct RScan { i: i64, n: i64 }
impl RowOp for RScan { fn next(&mut self, f: &mut [V]) -> bool { if self.i >= self.n { return false } f[0] = V::Int(self.i); self.i += 1; true } }
struct RFilter { c: Box<dyn RowOp> }
impl RowOp for RFilter { fn next(&mut self, f: &mut [V]) -> bool { while self.c.next(f) { if let V::Int(x) = f[0] { if x % 3 == 0 { return true } } } false } }
struct RProj { c: Box<dyn RowOp> }
impl RowOp for RProj { fn next(&mut self, f: &mut [V]) -> bool { if !self.c.next(f) { return false } f[1] = match f[0] { V::Int(x) => V::Int(x * 2), _ => V::Null }; true } }
// ---- batch model: columns of values + length ----
const B: usize = 1024;
struct Batch { cols: Vec<Vec<V>> }
trait BatchOp { fn next(&mut self, b: &mut Batch) -> bool; }
struct BScan { i: i64, n: i64 }
impl BatchOp for BScan { fn next(&mut self, b: &mut Batch) -> bool { if self.i >= self.n { return false } let e = (self.i + B as i64).min(self.n); let c = &mut b.cols[0]; c.clear(); c.extend((self.i..e).map(V::Int)); self.i = e; true } }
struct BFilter { c: Box<dyn BatchOp> }
impl BatchOp for BFilter { fn next(&mut self, b: &mut Batch) -> bool { loop { if !self.c.next(b) { return false } b.cols[0].retain(|v| matches!(v, V::Int(x) if x % 3 == 0)); if !b.cols[0].is_empty() { return true } } } }
struct BProj { c: Box<dyn BatchOp> }
impl BatchOp for BProj { fn next(&mut self, b: &mut Batch) -> bool { if !self.c.next(b) { return false } let (a, r) = b.cols.split_at_mut(1); r[0].clear(); r[0].extend(a[0].iter().map(|v| match v { V::Int(x) => V::Int(x * 2), _ => V::Null })); true } }
fn main() {
    let n: i64 = 50_000_000;
    for _ in 0..3 {
        let mut p: Box<dyn RowOp> = Box::new(RProj { c: Box::new(RFilter { c: Box::new(RScan { i: 0, n }) }) });
        let mut f = vec![V::Null, V::Null]; let t = Instant::now(); let mut s = 0i64;
        while p.next(&mut f) { if let V::Int(x) = f[1] { s = s.wrapping_add(x) } }
        let rt = t.elapsed();
        let mut q: Box<dyn BatchOp> = Box::new(BProj { c: Box::new(BFilter { c: Box::new(BScan { i: 0, n }) }) });
        let mut b = Batch { cols: vec![Vec::with_capacity(B), Vec::with_capacity(B)] }; let t = Instant::now(); let mut s2 = 0i64;
        while q.next(&mut b) { for v in &b.cols[1] { if let V::Int(x) = v { s2 = s2.wrapping_add(*x) } } }
        let bt = t.elapsed();
        assert_eq!(s, s2);
        println!("row-at-a-time: {:.2} ns/input row | batch(1024): {:.2} ns/input row | ratio {:.2}", rt.as_nanos() as f64 / n as f64, bt.as_nanos() as f64 / n as f64, rt.as_nanos() as f64 / bt.as_nanos() as f64);
    }
}
