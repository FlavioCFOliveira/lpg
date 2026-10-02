//! Byte helpers, varints, hashing, deterministic RNG and process probes.

#[inline(always)]
pub fn rd16(p: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([p[o], p[o + 1]])
}
#[inline(always)]
pub fn wr16(p: &mut [u8], o: usize, v: u16) {
    p[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
#[inline(always)]
pub fn rd32(p: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]])
}
#[inline(always)]
pub fn wr32(p: &mut [u8], o: usize, v: u32) {
    p[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
#[inline(always)]
pub fn rd64(p: &[u8], o: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&p[o..o + 8]);
    u64::from_le_bytes(b)
}
#[inline(always)]
pub fn wr64(p: &mut [u8], o: usize, v: u64) {
    p[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
#[inline(always)]
pub fn rd32be(p: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]])
}
#[inline(always)]
pub fn rd64be(p: &[u8], o: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&p[o..o + 8]);
    u64::from_be_bytes(b)
}

/// LEB128 unsigned varint.
#[inline(always)]
pub fn put_var(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}
#[inline(always)]
pub fn get_var(p: &[u8], off: &mut usize) -> u64 {
    let mut v = 0u64;
    let mut s = 0;
    loop {
        let b = p[*off];
        *off += 1;
        v |= ((b & 0x7f) as u64) << s;
        if b < 0x80 {
            return v;
        }
        s += 7;
    }
}

/// splitmix64 finaliser, used as a result checksum mixer.
#[inline(always)]
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic splitmix64 generator.
pub struct Rng(pub u64);
impl Rng {
    #[inline]
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next() as u128 * n as u128) >> 64) as u64
    }
    #[inline]
    pub fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn proc_field(path: &str, key: &str) -> u64 {
    let s = std::fs::read_to_string(path).unwrap_or_default();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            return digits.parse().unwrap_or(0);
        }
    }
    0
}
/// Bytes this process caused to be fetched from the storage layer (Linux task I/O accounting).
pub fn io_read_bytes() -> u64 {
    proc_field("/proc/self/io", "read_bytes:")
}
pub fn rss_kib() -> u64 {
    proc_field("/proc/self/status", "VmRSS:")
}
pub fn hwm_kib() -> u64 {
    proc_field("/proc/self/status", "VmHWM:")
}
