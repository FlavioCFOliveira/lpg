//! Parses arbitrary bytes and binds every program the parser accepts
//! (rmp #43): the binder must never panic. The checks are in
//! `lpg::fuzz::binder`; `LPG_FUZZ_DIR` names the database directory.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    lpg::fuzz::binder(data);
});
