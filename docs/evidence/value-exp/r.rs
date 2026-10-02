#![forbid(unsafe_code)]
fn main(){ let n=20_000_000u64; let t=std::time::Instant::now(); let mut acc=0u64; for _ in 0..n { let a=core::arch::x86_64::_rdtsc(); acc=acc.wrapping_add(core::arch::x86_64::_rdtsc().wrapping_sub(a)); } println!("rdtsc pair: {:.1} ns (acc {})", t.elapsed().as_nanos() as f64/n as f64, acc%7); }
