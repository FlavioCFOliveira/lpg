//! `lpg` is an embeddable Labeled Property Graph database queried with GQL
//! (ISO/IEC 39075:2024).
//!
//! # Platform
//!
//! Linux is the only supported platform. The crate uses Rust edition 2024
//! with `std`, and `unsafe` code is forbidden.

#![forbid(unsafe_code)]

pub mod obs;
pub mod ops;
pub mod storage;
pub mod tx;

#[cfg(test)]
mod lab;
#[cfg(test)]
mod test_support;
