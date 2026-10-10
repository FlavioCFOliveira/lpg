//! Feeds arbitrary bytes to the GQL parser (`src/query/`): it must never
//! panic or overflow the stack (rmp #41). The checks are in
//! `lpg::fuzz::parser`.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::parser(data);
});
