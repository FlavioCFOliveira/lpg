use std::sync::Arc;
#[allow(dead_code)] enum Rare { Dec(i128, u8), I128(i128), U128(u128), I256([u64;4]), Point{srid:u32, x:f64,y:f64,z:f64}, ZonedTime{nanos:i64, off:i32}, Zoned{secs:i64,nanos:u32,off:i32, tz: Arc<str>} }
#[allow(dead_code)] enum V16 { Null, Bool(bool), Int(i64), Float(f64), Str(Arc<str>), Bytes(Arc<[u8]>), List(Arc<[V16]>), Node(u64), Edge(u64), Date(i32), LocalDateTime(i64), DurYM(i64), DurDS(i64), Rare(Box<Rare>) }
#[allow(dead_code)] enum V24 { Null, Bool(bool), Int(i64), Float(f64), Str(Arc<str>), Bytes(Arc<[u8]>), List(Arc<[V24]>), Node(u64), Edge(u64), Date(i32), LocalDateTime(i64), Zoned{secs:i64,nanos:u32,off:i32}, DurDS{secs:i64,nanos:u32}, Rare(Box<Rare>) }
#[allow(dead_code)] enum VStr { Null, Int(i64), Str(String), List(Vec<VStr>), Map(std::collections::BTreeMap<String,VStr>) }
fn main() {
  println!("V16 (Arc<str>, boxed rare) = {} bytes", std::mem::size_of::<V16>());
  println!("V24 (inline zoned datetime) = {} bytes", std::mem::size_of::<V24>());
  println!("Option<V16> = {} bytes", std::mem::size_of::<Option<V16>>());
  println!("VStr (String/Vec/BTreeMap) = {} bytes", std::mem::size_of::<VStr>());
  println!("Rare = {} bytes", std::mem::size_of::<Rare>());
}
