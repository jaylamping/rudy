//! Re-exports `tests/common` for `tests/can/*.rs`. This file lives in `tests/can/`
//! so `#[path]` is resolved from that directory (`../common/mod.rs` → `tests/common/mod.rs`).

#[path = "../common/mod.rs"]
mod shared_common;

pub use shared_common::*;
