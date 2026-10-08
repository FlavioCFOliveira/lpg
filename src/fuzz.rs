//! Entry points of the fuzz targets in `fuzz/` (`cargo +nightly fuzz run
//! <target>`). Compiled only under `cfg(fuzzing)`, which cargo-fuzz sets.

/// Decodes `data` as a record, and its labels alone; both must return
/// `None` on malformed bytes, never panic.
pub fn record_decode(data: &[u8]) {
    let _ = crate::types::record::decode(data);
    let _ = crate::types::record::decode_labels(data);
}
