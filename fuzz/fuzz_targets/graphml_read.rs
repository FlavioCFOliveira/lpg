//! Fuzz target `graphml_read` (`lpg::fuzz::graphml_read`): never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::graphml_read(data);
});
