//! Fuzz target `dump_read` (`lpg::fuzz::dump_read`): never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::dump_read(data);
});
