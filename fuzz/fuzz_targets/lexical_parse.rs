//! Fuzz target `lexical_parse` (`lpg::fuzz::lexical_parse`): never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::lexical_parse(data);
});
