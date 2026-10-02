use std::sync::Arc;
#[allow(dead_code)] enum W { Null, Bool(bool), Int(i64), Float(f64), Str(Arc<String>), Bytes(Arc<Vec<u8>>), List(Arc<Vec<W>>), Node(u64), Edge(u64), Date(i32), LocalDateTime(i64), Rare(Box<[u64;6]>) }
fn main(){ println!("W (thin Arc<String>) = {} bytes", std::mem::size_of::<W>()); }
