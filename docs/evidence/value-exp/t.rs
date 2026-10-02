use std::time::Instant;
fn main(){ let n=20_000_000u32; let t=Instant::now(); let mut acc=0u128; for _ in 0..n { acc = acc.wrapping_add(std::hint::black_box(Instant::now()).elapsed().as_nanos()); } println!("Instant::now()+elapsed pair: {:.1} ns (acc {})", t.elapsed().as_nanos() as f64 / n as f64, acc%7);
 let flag = std::hint::black_box(false); let t=Instant::now(); let mut c=0u64; for i in 0..n { if flag { c+= i as u64 } else { c = c.wrapping_add(1) } } println!("untaken branch loop: {:.2} ns ({})", t.elapsed().as_nanos() as f64 / n as f64, c%3); }
