use std::time::Instant;
// Measures the round-trip cost of moving a resumable cursor to a blocking thread per pull.
struct Cursor { state: u64 }
impl Cursor { fn pull(&mut self, n: u64) -> u64 { for _ in 0..n { self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1); } self.state } }
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let iters = 200_000u32;
    // 1. inline pull on the async worker (baseline, no hand-off)
    let mut c = Cursor { state: 1 };
    let t = Instant::now();
    for _ in 0..iters { std::hint::black_box(c.pull(1)); tokio::task::yield_now().await; }
    println!("inline pull + yield_now: {:?}/pull", t.elapsed() / iters);
    // 2. spawn_blocking hand-off per pull, cursor moved in and out
    let mut c = Some(Cursor { state: 1 });
    let t = Instant::now();
    for _ in 0..iters {
        let mut cur = c.take().unwrap();
        let (cur, v) = tokio::task::spawn_blocking(move || { let v = cur.pull(1); (cur, v) }).await.unwrap();
        std::hint::black_box(v); c = Some(cur);
    }
    println!("spawn_blocking hand-off: {:?}/pull", t.elapsed() / iters);
    // 3. dedicated producer thread + bounded channel (capacity 1), blocking_send
    let (tx, mut rx) = tokio::sync::mpsc::channel::<u64>(1);
    std::thread::spawn(move || { let mut c = Cursor { state: 1 }; for _ in 0..iters { if tx.blocking_send(c.pull(1)).is_err() { break; } } });
    let t = Instant::now();
    let mut n = 0u32; while let Some(v) = rx.recv().await { std::hint::black_box(v); n += 1; }
    println!("dedicated thread + mpsc(1): {:?}/item over {n}", t.elapsed() / iters);
}
