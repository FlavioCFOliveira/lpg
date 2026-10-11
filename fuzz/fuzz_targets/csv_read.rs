//! Fuzz target `csv_read` (`lpg::fuzz::csv_read`): never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::csv_read(data);
});
