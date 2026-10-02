#![forbid(unsafe_code)]
use std::fs::{File, OpenOptions, TryLockError};
fn main() {
    let p = std::env::temp_dir().join("lpg-locktest.lock");
    let a = OpenOptions::new().create(true).read(true).write(true).open(&p).unwrap();
    a.try_lock().expect("first lock");
    let b = File::open(&p).unwrap();
    match b.try_lock() { Err(TryLockError::WouldBlock) => println!("same-process second fd: WouldBlock"), r => println!("same-process second fd: {:?}", r.is_ok()) }
    drop(b); // closing the other fd must not release a's lock (flock is per-OFD)
    let c = File::open(&p).unwrap();
    match c.try_lock() { Err(TryLockError::WouldBlock) => println!("after closing other fd: still WouldBlock"), r => println!("after closing other fd: {:?}", r.is_ok()) }
    if std::env::args().nth(1).as_deref() == Some("child") { return; }
    let out = std::process::Command::new(std::env::current_exe().unwrap()).arg("probe").env("PROBE","1").output();
    let _ = out;
    let st = std::process::Command::new("flock").args(["-n", p.to_str().unwrap(), "true"]).status().unwrap();
    println!("other process flock -n exit code: {:?}", st.code());
}
