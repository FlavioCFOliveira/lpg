//! Transactions.

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "first non-test callers come with the API tasks")
)]
pub(crate) mod commit;
