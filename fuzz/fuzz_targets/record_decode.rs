//! Decodes arbitrary bytes as a record (`src/types/record.rs`). Decoding
//! must return `None` on malformed input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::record_decode(data);
});
