//! Fuzz target `jsonl_read` (`lpg::fuzz::jsonl_read`): never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::jsonl_read(data);
});
