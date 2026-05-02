//! Re-export behavior modules for `use quasar_spl::accounts::prelude::*`.
//!
//! This brings short behavior module names into scope so that
//! `#[account(token(...))]` resolves without fully-qualified paths.

pub use super::{associated_token, mint, token, token_close, token_sweep};
